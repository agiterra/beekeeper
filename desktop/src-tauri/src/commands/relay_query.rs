//! `query_relay_filters`: the generic Nostr read the webview uses for one-shot
//! multi-filter queries over the relay's HTTP bridge (`POST /query`).
//!
//! The TS side (`shared/api`) batches and coalesces independent reads into
//! one call here so a cold start costs a handful of `/query` requests instead
//! of one REQ per filter. The command is a thin, validating wrapper over
//! [`crate::relay::query_relay`], which already waits on the shared
//! rate-limit gate and turns an HTTP 429 into a `relay rate-limited:` error
//! the TS gate understands.

use std::future::Future;

use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;

/// Refuse a filter array the relay would reject anyway, with a plain-English
/// reason. Accepts only a non-empty array whose every entry is a JSON object
/// (a Nostr filter); the relay validates the filter *contents*.
pub fn validate_query_filters(filters: &[Value]) -> Result<(), String> {
    if filters.is_empty() {
        return Err("query_relay_filters: no filters given (expected at least one)".to_string());
    }
    for (index, filter) in filters.iter().enumerate() {
        if !filter.is_object() {
            return Err(format!(
                "query_relay_filters: filter #{} is not an object (every filter must be a JSON object)",
                index + 1
            ));
        }
    }
    Ok(())
}

/// Validate, then hand the filters to `query`. Split from the Tauri command
/// so the delegation can be exercised without a live `AppState`.
async fn run_query<F, Fut>(filters: Vec<Value>, query: F) -> Result<Vec<nostr::Event>, String>
where
    F: FnOnce(Vec<Value>) -> Fut,
    Fut: Future<Output = Result<Vec<nostr::Event>, String>>,
{
    validate_query_filters(&filters)?;
    query(filters).await
}

/// Run one or more Nostr filters as a single `POST /query` against the
/// active community's relay and return the matching events.
///
/// `filters` is a JSON array of Nostr filter objects. An empty array or a
/// non-object entry is refused before any network call. A 429 surfaces as a
/// `relay rate-limited:` error, which the TS invoke wrapper turns into the
/// shared back-off gate.
#[tauri::command]
pub async fn query_relay_filters(
    state: State<'_, AppState>,
    filters: Vec<Value>,
) -> Result<Vec<nostr::Event>, String> {
    let state = state.inner();
    run_query(filters, |filters| async move {
        crate::relay::query_relay(state, &filters).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn empty_array_is_refused() {
        let err = validate_query_filters(&[]).unwrap_err();
        assert!(err.contains("no filters given"), "{err}");
    }

    #[test]
    fn non_object_entry_is_refused_by_position() {
        let filters = vec![json!({"kinds": [1]}), json!("kinds"), json!({"kinds": [2]})];
        let err = validate_query_filters(&filters).unwrap_err();
        assert!(err.contains("filter #2 is not an object"), "{err}");
        assert!(validate_query_filters(&[json!([1, 2])]).is_err());
        assert!(validate_query_filters(&[json!(null)]).is_err());
    }

    #[test]
    fn well_formed_filters_pass_validation() {
        let filters = vec![json!({"kinds": [1], "limit": 5}), json!({})];
        assert_eq!(validate_query_filters(&filters), Ok(()));
    }

    #[tokio::test]
    async fn well_formed_call_reaches_the_query_function_with_its_filters() {
        let calls = AtomicUsize::new(0);
        let filters = vec![
            json!({"kinds": [0], "authors": ["ab"]}),
            json!({"kinds": [1]}),
        ];
        let expected = filters.clone();
        let result = run_query(filters, |received| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(received, expected);
            async { Ok(Vec::new()) }
        })
        .await;
        assert_eq!(result.map(|events| events.len()), Ok(0));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refused_input_never_reaches_the_query_function() {
        let calls = AtomicUsize::new(0);
        let result = run_query(vec![json!(42)], |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(Vec::new()) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn query_errors_pass_through_verbatim() {
        let result = run_query(vec![json!({"kinds": [1]})], |_| async {
            Err("relay rate-limited: retry in 1s".to_string())
        })
        .await;
        assert_eq!(
            result.map(|_| ()),
            Err("relay rate-limited: retry in 1s".to_string())
        );
    }
}
