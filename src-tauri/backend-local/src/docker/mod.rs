//! Bollard-based Docker adapter — used by [`crate::LocalDockerBackend`].

mod manager;

pub use manager::{CreateContainerSpec, DockerError, DockerManager};
