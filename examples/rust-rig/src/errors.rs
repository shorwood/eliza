//! Errors owned by the Rig compatibility example.

use thiserror::Error;

/// Failure reported after all provider examples have run.
#[derive(Debug, Error)]
pub(super) enum RigExampleError {
    /// One or more provider examples failed.
    #[error("{count} Rig example(s) failed")]
    Failures {
        /// Number of failed unary or streaming calls.
        count: usize,
    },
}
