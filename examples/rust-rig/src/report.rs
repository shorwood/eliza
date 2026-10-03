use anyhow::Result;

use crate::errors::RigExampleError;

/// Succeed only when every example passed.
///
/// # Errors
///
/// Returns the accumulated failure count when it is nonzero.
pub(super) fn ensure_success(failures: usize) -> Result<()> {
    match failures {
        0 => Ok(()),
        count => Err(RigExampleError::Failures { count }.into()),
    }
}

pub(super) fn report(name: &str, result: Result<String>) -> usize {
    match result {
        Ok(output) => {
            println!("OK   {name}: {output}");
            0
        }
        Err(error) => {
            eprintln!("FAIL {name}: {error:#}");
            1
        }
    }
}
