//! Batch is a background job library.
//!
//! Batch allow a program (the *"Client"*) to defer a unit of work (the *"Job"*), until a background process (the
//! *"Worker"*)  picks it up and executes it. This is very common in web development where you don't want to slow down
//! an HTTP request for a behaviour that doesn't has to be done synchronously (e.g: sending a mail when a user signs
//! up).
//!
//! Batch provides you with adapters for popular message brokers and an implementation of a `Worker` that should work
//! well enough in most situations:
//!
//! * [RabbitMQ](rabbitmq/index.html)
//! * Faktory
//! * Amazon SQS
//!
//! # Example
//!
//! ```rust
//! extern crate batch;
//! #[macro_use]
//! extern crate serde;
//! extern crate tokio;
//!
//! use batch::job;
//! use batch::rabbitmq::{self, exchanges};
//! use tokio::prelude::*;
//!
//! exchanges! {
//!     Notifications {
//!         name = "notifications"
//!     }
//! }
//!
//! #[job(name = "batch-example.say-hello")]
//! fn say_hello(name: String) {
//!     println!("Hello {}!", name);
//! }
//!
//! fn main() {
//!     let conn = rabbitmq::Connection::open("amqp://guest:guest@localhost/");
//!     let notifications = conn.and_then(|mut conn| Notifications::declare(&mut conn));
//!     let send = notifications.and_then(|notifications| {
//!         notifications.with(say_hello(String::from("Ferris"))).deliver()
//!     });
//!
//! # if false {
//!     tokio::run(
//!         send.map_err(|e| eprintln!("Couldn't publish message: {}", e))
//!     );
//! # }
//! }
//! ```

#![doc(html_root_url = "https://docs.rs/batch/0.2.0")]
#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

#[cfg(feature = "codegen")]
extern crate batch_codegen;
extern crate batch_core;
#[cfg(feature = "rabbitmq")]
extern crate batch_rabbitmq;
#[cfg(feature = "rabbitmq")]
extern crate batch_rabbitmq_codegen;
#[cfg(feature = "worker")]
extern crate batch_worker;

#[cfg(feature = "codegen")]
pub use batch_codegen::job;

pub use batch_core::{
	dsl,
	Properties,
	Priority,
	Consumer,
	ToConsumer,
	Container,
	Callbacks,
	Declarator,
	Declare,
	Delivery,
	Dispatch,
	Job,
	Publisher,
	ToPublisher,
};

/// The prelude module for batch.
pub mod prelude {}

/// RabbitMQ adapter for batch.
#[cfg(feature = "rabbitmq")]
pub mod rabbitmq {
    pub use batch_rabbitmq::{
    	Connection,
    	Exchange,
    	ExchangeBuilder,
    	ExchangeKind,
    	Queue,
    	QueueBuilder,
    	Query,
    };

    pub use batch_rabbitmq_codegen::{
    	exchanges,
    	queues,
    };
}

#[cfg(feature = "worker")]
pub use batch_worker::Worker;
