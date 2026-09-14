//! Local Docker [`NodeBackend`] shared by the desktop app and the agent; the persistence root is injected.

pub mod backups;
pub mod crash;
pub mod docker;
pub mod metrics;
pub mod persistence;
pub mod players;
pub mod schedules;
pub mod webhooks;

mod backend;
pub use backend::LocalDockerBackend;
pub use crash::{query_crash_events, spawn_crash_watcher};
pub use schedules::{spawn_scheduler, BackupTargetResolver};

pub use docker::{DockerError, DockerManager};
