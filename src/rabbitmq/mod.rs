mod common;
mod consumer;
mod delivery;
mod publisher;
mod stream;
mod types;

pub use self::consumer::{Consumer, ConsumerHandle};
pub use self::delivery::Delivery;
pub use self::publisher::Publisher;
pub use self::types::{exchange, queue, Exchange, ExchangeBuilder, Queue, QueueBuilder};

#[cfg(test)]
mod tests {
    use super::*;
    use job::Priority;

    #[test]
    fn default_queue() {
        use futures::{future, Future, Stream};
        use lapin::channel::{BasicProperties, BasicPublishOptions};
        use lapin::types::{AMQPValue, FieldTable};
        use std::collections::VecDeque;
        use std::thread;
        use std::time;
        use tokio::reactor::Handle;

        let _ = ::env_logger::try_init();
        let ex = "batch.tests.default";
        let rk = "default-hello";
        let body = "{}";
        let jobs = vec![
            (("job-1", ex, rk, body.as_bytes()), Priority::Normal),
            (("job-2", ex, rk, body.as_bytes()), Priority::Critical),
            (("job-3", ex, rk, body.as_bytes()), Priority::Trivial),
            (("job-4", ex, rk, body.as_bytes()), Priority::High),
            (("job-5", ex, rk, body.as_bytes()), Priority::Low),
        ];
        let expected = VecDeque::from(vec!["job-1", "job-2", "job-3", "job-4", "job-5"]);

        let conn_url = "amqp://localhost/%2f";
        let exchanges = vec![exchange(ex).build()];
        let queues = vec![queue("tests.default").bind(ex, rk).build()];
        let handle = Handle::current();
        let task =
            Publisher::new_with_handle(conn_url, exchanges.clone(), queues.clone(), handle.clone())
                .and_then(move |publisher| {
                    info!("Publishing messages");
                    let tasks = jobs.into_iter().map(move |(job, priority)| {
                        let mut headers = FieldTable::new();
                        headers.insert("lang".to_string(), AMQPValue::LongString("rs".to_string()));
                        headers
                            .insert("task".to_string(), AMQPValue::LongString(job.0.to_string()));
                        let properties = BasicProperties {
                            priority: Some(priority.to_u8()),
                            headers: Some(headers),
                            ..Default::default()
                        };
                        publisher.send(
                            job.1,
                            job.2,
                            job.3,
                            &BasicPublishOptions::default(),
                            properties,
                        )
                    });
                    future::join_all(tasks)
                })
                .and_then(move |_| {
                    info!("Published all messages");
                    Consumer::new_with_handle(conn_url, exchanges, queues, 1, handle)
                })
                .and_then(move |consumer| {
                    info!("Starting recursive loop fn");
                    future::loop_fn(
                        (consumer.into_future(), expected.clone()),
                        |(f, mut order)| {
                            info!("Iterating over consumer deliveries");
                            info!(" -> {:?}", order);
                            f.map_err(|(e, _)| e)
                                .and_then(move |(next, consumer)| {
                                    let head = order.pop_front().unwrap();
                                    let tail = order;
                                    let delivery = next.unwrap();
                                    assert_eq!(delivery.task(), head);
                                    let handle = consumer.handle();
                                    handle.ack(delivery.tag()).map(|_| (consumer, tail))
                                })
                                .and_then(|(consumer, order)| {
                                    info!("End of iteration:");
                                    info!(" -> {:?}", order);
                                    info!(" -> {:?}", order.is_empty());
                                    if order.is_empty() {
                                        Ok(future::Loop::Break(()))
                                    } else {
                                        Ok(future::Loop::Continue((consumer.into_future(), order)))
                                    }
                                })
                        },
                    )
                })
                .map_err(|e| panic!("Couldn't complete test: {}", e));
        ::tokio::run(task);
    }

    #[test]
    fn priority_queue() {
        use futures::{future, Future, Stream};
        use lapin::channel::{BasicProperties, BasicPublishOptions};
        use lapin::types::{AMQPValue, FieldTable};
        use std::collections::VecDeque;
        use std::thread;
        use std::time;
        use tokio::reactor::Handle;

        let _ = ::env_logger::try_init();
        let ex = "batch.tests.priorities";
        let rk = "prioritised-hello";
        let body = "{}";
        let jobs = vec![
            (("job-1", ex, rk, body.as_bytes()), Priority::Normal),
            (("job-2", ex, rk, body.as_bytes()), Priority::Critical),
            (("job-3", ex, rk, body.as_bytes()), Priority::Trivial),
            (("job-4", ex, rk, body.as_bytes()), Priority::High),
            (("job-5", ex, rk, body.as_bytes()), Priority::Low),
        ];
        let expected = VecDeque::from(vec!["job-2", "job-4", "job-1", "job-5", "job-3"]);

        let conn_url = "amqp://localhost/%2f";
        let exchanges = vec![exchange(ex).build()];
        let queues = vec![
            queue("tests.priorities")
                .enable_priorities()
                .bind(ex, rk)
                .build(),
        ];
        let handle = Handle::current();
        let task =
            Publisher::new_with_handle(conn_url, exchanges.clone(), queues.clone(), handle.clone())
                .and_then(move |publisher| {
                    info!("Publishing messages");
                    let tasks = jobs.into_iter().map(move |(job, priority)| {
                        let mut headers = FieldTable::new();
                        headers.insert("lang".to_string(), AMQPValue::LongString("rs".to_string()));
                        headers
                            .insert("task".to_string(), AMQPValue::LongString(job.0.to_string()));
                        let properties = BasicProperties {
                            priority: Some(priority.to_u8()),
                            headers: Some(headers),
                            ..Default::default()
                        };
                        publisher.send(
                            job.1,
                            job.2,
                            job.3,
                            &BasicPublishOptions::default(),
                            properties,
                        )
                    });
                    future::join_all(tasks)
                })
                .and_then(move |_| {
                    info!("Published all messages");
                    Consumer::new_with_handle(conn_url, exchanges, queues, 1, handle)
                })
                .and_then(move |consumer| {
                    info!("Starting recursive loop fn");
                    future::loop_fn(
                        (consumer.into_future(), expected.clone()),
                        |(f, mut order)| {
                            info!("Iterating over consumer deliveries");
                            info!(" -> {:?}", order);
                            f.map_err(|(e, _)| e)
                                .and_then(move |(next, consumer)| {
                                    let head = order.pop_front().unwrap();
                                    let tail = order;
                                    let delivery = next.unwrap();
                                    assert_eq!(delivery.task(), head);
                                    let handle = consumer.handle();
                                    handle.ack(delivery.tag()).map(|_| (consumer, tail))
                                })
                                .and_then(|(consumer, order)| {
                                    info!("End of iteration:");
                                    info!(" -> {:?}", order);
                                    info!(" -> {:?}", order.is_empty());
                                    if order.is_empty() {
                                        Ok(future::Loop::Break(()))
                                    } else {
                                        Ok(future::Loop::Continue((consumer.into_future(), order)))
                                    }
                                })
                        },
                    )
                })
                .map_err(|e| panic!("Couldn't complete test: {}", e));
        ::tokio::run(task);
    }
}
