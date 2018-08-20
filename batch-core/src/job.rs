//! A trait representing a job.

use std::fmt;
use std::time::Duration;

use failure::Error;
use futures::Future;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use { Container };

/// A job and its related metadata (name, queue, timeout, etc.)
///
/// In most cases, you should be deriving this trait instead of implementing it manually yourself.
///
/// # Examples
///
/// Using the provided defaults:
///
/// ```rust
/// #[macro_use]
/// extern crate batch;
/// #[macro_use]
/// extern crate lazy_static;
/// #[macro_use]
/// extern crate serde;
///
/// #[derive(Deserialize, Serialize, Job)]
/// #[job_routing_key = "emails"]
/// struct SendConfirmationEmail;
///
/// #
/// # fn main() {}
/// ```
///
/// Overriding the provided defaults:
///
/// ```rust
/// #[macro_use]
/// extern crate batch;
/// #[macro_use]
/// extern crate lazy_static;
/// #[macro_use]
/// extern crate serde;
///
/// #[derive(Deserialize, Serialize)]
/// struct SendPasswordResetEmail;
///
/// impl Job for SendPasswordResetEmail {
///     const NAME: &'static str = "send-password-reset-email";
///
///     fn perform(&self, ctx: ()) {
///         println!("Sending password reset email...");
///     }
/// }
///
/// #
/// # fn main() {}
/// ```
pub trait Job: Serialize + for<'a> Deserialize<'a> {
    /// A should-be-unique human-readable ID for this job.
    const NAME: &'static str;

    /// The number of times this job must be retried in case of error.
    ///
    /// You probably should be using the method `retries` instead.
    const RETRIES: u32 = 3;

    /// An optional duration representing the time allowed for this job's handler to complete.
    ///
    /// You probably should be using the method `timeout` instead.
    const TIMEOUT: Duration = Duration::from_secs(30 * 60);

    /// The priority associated to this job.
    ///
    /// You probably should be using the method `priority` instead.
    const PRIORITY: Priority = Priority::Normal;

    /// The return type of the `perform` method.
    type PerformFuture: Future<Item = (), Error = Error> + Send + 'static;

    /// Perform the background job.
    fn perform(self, context: Container) -> Self::PerformFuture;

    /// The number of times this job must be retried in case of error.
    ///
    /// This function is meant to be overriden by the user to return a value different from the associated
    /// constant depending on the contents of the actual job.
    fn retries(&self) -> u32 {
        Self::RETRIES
    }

    /// An optional duration representing the time allowed for this job's handler to complete.
    ///
    /// This function is meant to be overriden by the user to return a value different from the associated
    /// constant depending on the contents of the actual job.
    fn timeout(&self) -> Duration {
        Self::TIMEOUT
    }

    /// The priority associated to this job.
    ///
    /// This function is meant to be overriden by the user to return a value different from the associated
    /// constant depending on the contents of the actual job.
    fn priority(&self) -> Priority {
        Self::PRIORITY
    }
}

/// The different priorities that can be assigned to a `Job`.
///
/// The default value is `Priority::Normal`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    /// The lowest available priority for a job.
    Trivial = 1,
    /// A lower priority than `Priority::Normal` but higher than `Priority::Trivial`.
    Low = 3,
    /// The default priority for a job.
    Normal = 5,
    /// A higher priority than `Priority::Normal` but higher than `Priority::Critical`.
    High = 7,
    /// The highest available priority for a job.
    Critical = 9,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

/// Message properties.
#[derive(Clone, Serialize, Deserialize)]
pub struct Properties {
    /// The language in which the job was created. 
    pub lang: String,
    /// The name of the job.
    pub task: String,
    /// The ID of this job.
    ///
    /// To guarantee the uniqueness of this ID, a UUID version 4 used.
    pub id: Uuid,
    /// The ID of the greatest ancestor of this job, if there is one.
    pub root_id: Option<Uuid>,
    /// The ID of the direct ancestor of this job, if there is one.
    pub parent_id: Option<Uuid>,
    /// The ID of the group this job is part of, if there is one.
    pub group: Option<Uuid>,
    /// Timelimits for this job.
    ///
    /// The first duration represents the soft timelimit while the second duration represents the hard timelimit.
    pub timelimit: (Option<Duration>, Option<Duration>),
    /// The priority of this job.
    pub priority: Priority,
    /// The content type of the job once serialized.
    pub content_type: String,
    /// The content encoding of the job once serialized.
    pub content_encoding: String,
    __non_exhaustive: (),
}

impl Properties {
    /// Create a new `Properties` instance from a task name.
    pub fn new<T: ToString>(task: T) -> Self {
        Properties {
            lang: "rs".to_string(),
            task: task.to_string(),
            id: Uuid::new_v4(),
            root_id: None,
            parent_id: None,
            group: None,
            timelimit: (None, None),
            priority: Priority::default(),
            content_type: "application/json".to_string(),
            content_encoding: "utf-8".to_string(),
            __non_exhaustive: (),
        }
    }
}

impl fmt::Debug for Properties {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Properties")
            .field("content_encoding", &self.content_encoding)
            .field("content_type", &self.content_type)
            .field("lang", &self.lang)
            .field("task", &self.task)
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("timelimit", &self.timelimit)
            .field("root_id", &self.root_id)
            .field("parent_id", &self.parent_id)
            .field("group", &self.group)
            .finish()
    }
}
