//! Batch Worker.
//!
//! The worker is responsible for polling the broker for jobs, deserializing them and execute
//! them. It should never ever crash and sould be resilient to panic-friendly job handlers. Its
//! `Broker` implementation is completely customizable by the user.
//!
//! # Trade-offs
//!
//! The most important thing to know about the worker is that it favours safety over performance.
//! For each incoming job, it will spawn a new process whose only goal is to perform the job.
//! Even if this is slower than just executing the function in a threadpool, it allows much more
//! control: timeouts wouldn't even be possible if we were running the jobs in-process. It also
//! protects against unpredictable crashes

extern crate batch_core as batch;
#[macro_use]
extern crate failure;
extern crate futures;
#[macro_use]
extern crate log;
extern crate tokio;
extern crate wait_timeout;

use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{self, Read};
use std::process;
use std::result::Result;
use std::sync::mpsc;

use batch::{Container, Delivery};
use failure::Error;
use futures::{Future, Stream};
use wait_timeout::ChildExt;

pub struct Worker<Conn>
where
    Conn: batch::ToConsumer + Send + 'static
{
    connection: Conn,
    queues: HashSet<String>,
    state: Container,
    callbacks: HashMap<String, fn(&[u8], batch::Container) -> Box<Future<Item = (), Error = Error> + Send>>
}

impl<Conn> Worker<Conn>
where
    Conn: batch::ToConsumer + Send + 'static
{
    pub fn new(connection: Conn) -> Self {
        Worker {
            connection,
            state: Container::new(),
            queues: HashSet::new(),
            callbacks: HashMap::new(),
        }
    }

    pub fn declare<D>(mut self) -> impl Future<Item = Self, Error = Error> + Send
    where
        D: batch::Declare + batch::Callbacks,
        Conn: batch::Declarator<D::Input, D::Output> + Send + 'static
    {
        D::declare(&mut self.connection)
            .and_then(|resource| {
                self.queues.insert(D::NAME.into());
                for (job, callback) in resource.callbacks() {
                    if let Some(previous) = self.callbacks.insert(job.clone(), callback) {
                        if previous as fn(_, _) -> _ != callback as fn(_, _) -> _ {
                            bail!("Two different callbacks were registered for the `{}` job.", job)
                        }
                    }
                }
                Ok(self)
            })
    }

    pub fn manage<F, T>(&mut self, init: F) -> &mut Self
    where
        T: Send + 'static,
        F: Fn() -> T + 'static
    {
        self.state.set_local(init);
        self
    }

    pub fn run(self) -> impl Future<Item = (), Error = Error> + Send {
        if let Ok(job) = env::var("BATCHRS_WORKER_IS_EXECUTOR") {
            let (tx, rx) = mpsc::channel::<Result<(), Error>>();
            let tx2 = tx.clone();
            let f = self.execute(job)
                .map(move |_| tx.send(Ok(())).unwrap())
                .map_err(move |e| tx2.send(Err(e)).unwrap());
            tokio::spawn(f);
            rx.recv().unwrap().unwrap();
            process::exit(0);
        }
        self.supervise()
    }

    fn supervise(mut self) -> impl Future<Item = (), Error = Error> + Send {
        self.connection.to_consumer(self.queues.clone().into_iter())
            .and_then(move |consumer| {
                consumer.for_each(move |delivery| {
                    debug!("delivery; job_id={}", delivery.properties().id);
                    // TODO: use tokio_threadpool::blocking instead of spawn a task for each execution?
                    let task = futures::lazy(move || -> Box<Future<Item = (), Error = Error> + Send> {
                        match spawn(&delivery) {
                            Err(e) => {
                                error!("spawn: {}; job_id={}", e, delivery.properties().id);
                                Box::new(delivery.reject())
                            }
                            Ok(ExecutionStatus::Failed(f)) => {
                                warn!("execution; status={:?} job_id={}", ExecutionStatus::Failed(f), delivery.properties().id);
                                Box::new(delivery.reject())
                            },
                            Ok(ExecutionStatus::Success) => {
                                debug!("execution; status={:?} job_id={}", ExecutionStatus::Success, delivery.properties().id);
                                Box::new(delivery.ack())
                            }
                        }
                    }).map_err(|e| error!("An error occured while informing the broker of the execution status: {}", e));
                    tokio::spawn(task);
                    Ok(())
                })
            })
            .map(|_| ())
    }

    fn execute(mut self, job: String) -> impl Future<Item = (), Error = Error> + Send {
        self.state.freeze();
        let mut input = vec![];
        // It is safe to unwrap because we know this function will be executed in a child process.
        io::stdin().read_to_end(&mut input).unwrap();
        let handler = self.callbacks.get(&job).unwrap();
        (*handler)(&input, self.state)
    }
}

#[derive(Debug)]
enum ExecutionStatus {
    Success,
    Failed(ExecutionFailure),
}

#[derive(Debug)]
enum ExecutionFailure {
    Timeout,
    Crash,
    Error
}

fn spawn(delivery: &impl Delivery) -> Result<ExecutionStatus, Error> {
    use std::io::Write;

    let current_exe = env::current_exe()?;
    let mut child = process::Command::new(&current_exe)
        .env("BATCHRS_WORKER_IS_EXECUTOR", &delivery.properties().task)
        .stdin(process::Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().expect("failed to get stdin");
        stdin.write_all(delivery.payload())?;
        stdin.flush()?;
    }
    let (_, timeout) = delivery.properties().timelimit;
    if let Some(duration) = timeout {
        drop(child.stdin.take());
        if let Some(status) = child
            .wait_timeout(duration)?
        {
            if status.success() {
                Ok(ExecutionStatus::Success)
            } else if status.unix_signal().is_some() {
                Ok(ExecutionStatus::Failed(ExecutionFailure::Crash))
            } else {
                Ok(ExecutionStatus::Failed(ExecutionFailure::Error))
            }
        } else {
            child
                .kill()?;
            child
                .wait()?;
            Ok(ExecutionStatus::Failed(ExecutionFailure::Timeout))
        }
    } else {
        let status = child
            .wait()?;
        if status.success() {
            Ok(ExecutionStatus::Success)
        } else if status.code().is_some() {
            Ok(ExecutionStatus::Failed(ExecutionFailure::Error))
        } else {
            Ok(ExecutionStatus::Failed(ExecutionFailure::Crash))
        }
    }
}
