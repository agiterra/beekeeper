//! Shared management retains the CLI's existing typed HTTP retry policy.
use super::*;
use crate::error::{exit_code, is_retryable_error};

#[test]
fn management_http_status_round_trip_preserves_retry_and_exit_semantics() {
    for status in [400, 401, 403, 404, 429, 502, 503, 504] {
        let body = "fixture relay response";
        let expected = CliError::Relay {
            status,
            body: body.into(),
        };
        let retry = is_retryable_error(&expected);
        let exit = exit_code(&expected);
        let shared = management_transport_error(expected);
        assert!(matches!(&shared, beekeeper_agent_management::Error::Relay {
            status: retained, body: retained_body
        } if *retained == status && retained_body == body));
        let actual = CliError::from(shared);
        assert_eq!(is_retryable_error(&actual), retry, "HTTP {status}");
        assert_eq!(exit_code(&actual), exit, "HTTP {status}");
        assert!(matches!(actual, CliError::Relay { status: retained, .. } if retained == status));
    }
}
#[test]
fn arbitrary_error_text_and_unconfirmed_outcome_are_not_guessed_as_retryable_status() {
    let text = CliError::from(management_transport_error(CliError::Other(
        "relay error 429: unknown outcome".into(),
    )));
    assert!(!is_retryable_error(&text));
    assert!(matches!(text, CliError::Other(_)));
    let uncertain = CliError::from(beekeeper_agent_management::Error::Unconfirmed(
        "retry exact retained operation after observing outcome".into(),
    ));
    assert!(!is_retryable_error(&uncertain));
    assert!(matches!(uncertain, CliError::Unconfirmed(_)));
}
