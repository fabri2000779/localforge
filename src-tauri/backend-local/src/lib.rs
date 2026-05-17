//! Local Docker [`NodeBackend`] implementation.
//!
//! Reused by both the desktop app (running against the user's own Docker
//! daemon) and by the `localforge-agent` binary on a VPS (running against
//! the agent host's Docker daemon).
//!
//! The persistence root (`~/LocalForge` on desktop, `/var/lib/localforge`
//! on agents) is provided to [`LocalDockerBackend::connect`] so the same
//! code path works in both environments.

pub mod docker;
pub mod persistence;

mod backend;
pub use backend::LocalDockerBackend;

pub use docker::{DockerError, DockerManager};
