//! Local Docker [`NodeBackend`] implementation.
//!
//! Reused by both the desktop app (running against the user's own Docker
//! daemon) and by the `localforge-agent` binary on a VPS (running against
//! the agent host's Docker daemon).
//!
//! The persistence root (`~/LocalForge` on desktop, `/var/lib/localforge`
//! on agents) is provided to [`LocalDockerBackend::connect`] so the same
//! code path works in both environments.

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
