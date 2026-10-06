//! Transport mapping for ingestion failures, including conditional writes.

use axum::http::StatusCode;

/// Ingestion error — callers preserve its meaning across HTTP and WebSocket.
#[derive(Debug)]
pub enum IngestError {
    /// Invalid event — HTTP 400 or WebSocket refusal.
    Rejected(String),
    /// Missing authority — HTTP 403 or WebSocket refusal.
    AuthFailed(String),
    /// A conditional write no longer matches the stored source — HTTP 409.
    Conflict(String),
    /// Server failure, whose details must not be exposed to clients.
    Internal(String),
}

impl IngestError {
    /// Public message, HTTP status and metric category for the same refusal.
    pub(crate) fn response(&self) -> (StatusCode, String, &'static str) {
        match self {
            Self::Rejected(message) => (StatusCode::BAD_REQUEST, message.clone(), "invalid"),
            Self::AuthFailed(message) => (StatusCode::FORBIDDEN, message.clone(), "auth"),
            Self::Conflict(message) => (StatusCode::CONFLICT, message.clone(), "conflict"),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "error: internal server error".into(),
                "error",
            ),
        }
    }
}

pub(super) fn parameterized_write_error(error: beekeeper_db::DbError) -> IngestError {
    match error {
        beekeeper_db::DbError::PackSourceConflict(reason) => {
            IngestError::Conflict(format!("conflict: PACK_SOURCE_CONFLICT: {reason}"))
        }
        other => IngestError::Internal(format!("error: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_conflict_is_distinct_from_authority_and_storage_failure() {
        let conflict = parameterized_write_error(beekeeper_db::DbError::PackSourceConflict(
            "the project source changed".into(),
        ));
        assert!(matches!(conflict, IngestError::Conflict(_)));
        assert_eq!(
            conflict.response(),
            (
                StatusCode::CONFLICT,
                "conflict: PACK_SOURCE_CONFLICT: the project source changed".into(),
                "conflict"
            )
        );
        assert_eq!(
            IngestError::AuthFailed("restricted".into()).response().0,
            StatusCode::FORBIDDEN
        );
        let outage =
            parameterized_write_error(beekeeper_db::DbError::Sqlx(sqlx::Error::PoolTimedOut));
        assert!(matches!(outage, IngestError::Internal(_)));
        assert_eq!(outage.response().0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(outage.response().1, "error: internal server error");
    }
}
