use super::*;
use crate::errors::ModelError;

/**************************************/
/* NativeError: Projection tests      */
/**************************************/

/// Internal source details never become a public report.
#[test]
fn native_error_internal_detail_is_private() {
    let failure = NativeError::from_diagnostic(&ModelError::Empty, ProblemClass::Internal, None);
    assert_eq!(failure.message(), "Internal server error");
    assert!(failure.report.as_details().detail().is_none());
}
