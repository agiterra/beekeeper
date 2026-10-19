//! The `application_event` trigger: which kind:50000 events may fire it.
//!
//! An application event fires a workflow only when **all** of these hold:
//! - it is signed by the relay's own key (a member can publish a 50000 with
//!   any type it likes; a type name is not provenance);
//! - it carries a `beekeeper:source` tag naming the plugin installation that
//!   produced it (the relay adds that tag, and only for webhook deliveries);
//! - the installation matches the trigger's `installation` (`any` or a UUID);
//! - its type label matches the trigger's `type` (exact, or `prefix.*`).
//!
//! The optional `filter` is evaluated afterwards, with the event's data
//! flattened into the trigger context (see [`flatten_data`]).

use std::collections::HashMap;

use beekeeper_core::application_event::{
    valid_application_event_type, APPLICATION_EVENT_NAMESPACE,
};
use uuid::Uuid;

use crate::error::WorkflowError;

/// Tag the relay adds to a plugin-produced event: `["beekeeper:source",
/// <plugin>, <installation uuid>]`.
pub const SOURCE_TAG: &str = "beekeeper:source";
/// `installation` value that accepts every installation.
pub const ANY_INSTALLATION: &str = "any";
/// Deepest `data` nesting flattened into the trigger context.
pub const MAX_FLATTEN_DEPTH: usize = 3;
/// Longest scalar value (bytes) flattened into the trigger context; longer
/// values are left out rather than cut, so a filter never sees half a value.
pub const MAX_FLATTEN_VALUE_BYTES: usize = 1024;

/// Validate the trigger's `type` and `installation` at definition time.
pub(crate) fn validate_trigger(event_type: &str, installation: &str) -> Result<(), WorkflowError> {
    match event_type.strip_suffix(".*") {
        Some(prefix) => {
            if prefix.split('.').count() < 2
                || !valid_application_event_type(&format!("{prefix}.x"))
            {
                return Err(WorkflowError::InvalidDefinition(
                    "application_event type glob must be at least two valid components \
                     followed by .* (for example com.github.pull-request.*)"
                        .into(),
                ));
            }
        }
        None => {
            if !valid_application_event_type(event_type) {
                return Err(WorkflowError::InvalidDefinition(
                    "application_event type must be a valid application type (at least three \
                     lowercase dot-separated components) or a prefix ending in .*"
                        .into(),
                ));
            }
        }
    }
    if installation != ANY_INSTALLATION && Uuid::parse_str(installation).is_err() {
        return Err(WorkflowError::InvalidDefinition(
            "application_event installation must be a plugin installation UUID or 'any'".into(),
        ));
    }
    Ok(())
}

/// Whether `event_type` matches the trigger's `pattern`.
pub fn type_matches(pattern: &str, event_type: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(prefix) => event_type
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|rest| !rest.is_empty()),
        None => pattern == event_type,
    }
}

/// The provenance facts of an application event that passed [`check_event`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationEventFacts {
    /// The event's application type (its `l` label).
    pub app_type: String,
    /// The plugin named by the `beekeeper:source` tag.
    pub plugin: String,
    /// The installation named by the `beekeeper:source` tag.
    pub installation: String,
}

/// Decide whether `event` may fire an `application_event` trigger with the
/// given `type` pattern and `installation` selector.
///
/// `relay_pubkey` is the relay's signing key; `None` (not wired) refuses
/// every event. On refusal the reason is returned, for the skip record.
pub fn check_event(
    event: &nostr::Event,
    relay_pubkey: Option<&nostr::PublicKey>,
    type_pattern: &str,
    installation: &str,
) -> Result<ApplicationEventFacts, &'static str> {
    let Some(relay_pubkey) = relay_pubkey else {
        return Err("relay key is not configured on the workflow engine");
    };
    if &event.pubkey != relay_pubkey {
        return Err("application event is not signed by the relay");
    }
    let mut source = None;
    let mut app_type = None;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        match parts.first().map(String::as_str) {
            Some(SOURCE_TAG) => {
                if source.is_some() {
                    return Err("application event carries more than one source tag");
                }
                if parts.len() != 3 || parts[1].is_empty() || Uuid::parse_str(&parts[2]).is_err() {
                    return Err("application event source tag is malformed");
                }
                source = Some((parts[1].clone(), parts[2].clone()));
            }
            Some("l") if parts.len() == 3 && parts[2] == APPLICATION_EVENT_NAMESPACE => {
                app_type = Some(parts[1].clone());
            }
            _ => {}
        }
    }
    let Some((plugin, event_installation)) = source else {
        return Err("application event has no source tag");
    };
    let Some(app_type) = app_type else {
        return Err("application event has no type label");
    };
    if installation != ANY_INSTALLATION && !installation.eq_ignore_ascii_case(&event_installation) {
        return Err("application event is from another installation");
    }
    if !type_matches(type_pattern, &app_type) {
        return Err("application event type does not match");
    }
    Ok(ApplicationEventFacts {
        app_type,
        plugin,
        installation: event_installation,
    })
}

/// Trigger-context fields for an application event: `app_type`,
/// `installation`, `plugin`, and `data.<path>` for every scalar of the
/// content's `data` object up to [`MAX_FLATTEN_DEPTH`] levels, skipping
/// values over [`MAX_FLATTEN_VALUE_BYTES`]. Arrays are not flattened.
pub fn trigger_fields(facts: &ApplicationEventFacts, content: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    fields.insert("app_type".to_owned(), facts.app_type.clone());
    fields.insert("installation".to_owned(), facts.installation.clone());
    fields.insert("plugin".to_owned(), facts.plugin.clone());
    if let Ok(serde_json::Value::Object(content)) = serde_json::from_str(content) {
        if let Some(serde_json::Value::Object(data)) = content.get("data") {
            flatten_data("data", data, 1, &mut fields);
        }
    }
    fields
}

/// Flatten `object`'s scalars into `out` as `prefix.key[.key...]`.
pub fn flatten_data(
    prefix: &str,
    object: &serde_json::Map<String, serde_json::Value>,
    depth: usize,
    out: &mut HashMap<String, String>,
) {
    for (key, value) in object {
        let path = format!("{prefix}.{key}");
        let scalar = match value {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            serde_json::Value::Null => Some(String::new()),
            serde_json::Value::Object(inner) => {
                if depth < MAX_FLATTEN_DEPTH {
                    flatten_data(&path, inner, depth + 1, out);
                }
                None
            }
            serde_json::Value::Array(_) => None,
        };
        if let Some(scalar) = scalar {
            if scalar.len() <= MAX_FLATTEN_VALUE_BYTES {
                out.insert(path, scalar);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const INSTALLATION: &str = "5b0b1c7e-6a77-4d3c-9b1f-2a7e3c1d0f11";
    const OTHER_INSTALLATION: &str = "0e5f6a14-0000-4000-8000-000000000001";

    fn app_event(signer: &Keys, app_type: &str, source: Option<&str>) -> nostr::Event {
        let mut tags = vec![
            Tag::parse(["h", "c42dcf94-1a36-4ce5-b663-5903aa10fbdb"]).expect("h"),
            Tag::parse(["L", APPLICATION_EVENT_NAMESPACE]).expect("L"),
            Tag::parse(["l", app_type, APPLICATION_EVENT_NAMESPACE]).expect("l"),
        ];
        if let Some(installation) = source {
            tags.push(Tag::parse([SOURCE_TAG, "github", installation]).expect("source"));
        }
        EventBuilder::new(
            Kind::Custom(beekeeper_core::kind::KIND_APPLICATION_EVENT as u16),
            r#"{"schema":1,"data":{"repository":{"full_name":"agiterra/beekeeper","private":false,"owner":{"id":7,"deep":{"x":1}}},"number":42,"labels":["a"],"long":"LONG"}}"#,
        )
        .tags(tags)
        .sign_with_keys(signer)
        .expect("sign")
    }

    /// (name, event, type pattern, installation, expected).
    type MatchCase<'a> = (&'a str, nostr::Event, &'a str, &'a str, Result<(), &'a str>);

    #[test]
    fn matching_table() {
        let relay = Keys::generate();
        let member = Keys::generate();
        let pr = "com.github.pull-request.opened";
        let cases: Vec<MatchCase> = vec![
            (
                "relay-signed, sourced, exact type, exact installation",
                app_event(&relay, pr, Some(INSTALLATION)),
                pr,
                INSTALLATION,
                Ok(()),
            ),
            (
                "installation any",
                app_event(&relay, pr, Some(INSTALLATION)),
                pr,
                ANY_INSTALLATION,
                Ok(()),
            ),
            (
                "glob",
                app_event(&relay, pr, Some(INSTALLATION)),
                "com.github.pull-request.*",
                ANY_INSTALLATION,
                Ok(()),
            ),
            (
                "glob does not match the bare prefix's sibling",
                app_event(
                    &relay,
                    "com.github.pull-request-review.submitted",
                    Some(INSTALLATION),
                ),
                "com.github.pull-request.*",
                ANY_INSTALLATION,
                Err("application event type does not match"),
            ),
            (
                "exact type mismatch",
                app_event(&relay, "com.github.push", Some(INSTALLATION)),
                pr,
                ANY_INSTALLATION,
                Err("application event type does not match"),
            ),
            (
                "member-signed never fires",
                app_event(&member, pr, Some(INSTALLATION)),
                pr,
                ANY_INSTALLATION,
                Err("application event is not signed by the relay"),
            ),
            (
                "relay-signed without a source tag never fires",
                app_event(&relay, pr, None),
                pr,
                ANY_INSTALLATION,
                Err("application event has no source tag"),
            ),
            (
                "another installation",
                app_event(&relay, pr, Some(OTHER_INSTALLATION)),
                pr,
                INSTALLATION,
                Err("application event is from another installation"),
            ),
        ];
        for (name, event, pattern, installation, expected) in cases {
            let got =
                check_event(&event, Some(&relay.public_key()), pattern, installation).map(|_| ());
            assert_eq!(got, expected, "{name}");
        }
    }

    #[test]
    fn an_unwired_relay_key_refuses_everything() {
        let relay = Keys::generate();
        let event = app_event(&relay, "com.github.push.x", Some(INSTALLATION));
        assert!(check_event(&event, None, "com.github.push.x", ANY_INSTALLATION).is_err());
    }

    #[test]
    fn a_malformed_or_duplicated_source_tag_is_refused() {
        let relay = Keys::generate();
        let mut event = app_event(&relay, "a.b.c", Some(INSTALLATION));
        let mut tags: Vec<Tag> = event.tags.iter().cloned().collect();
        tags.push(Tag::parse([SOURCE_TAG, "github", INSTALLATION]).expect("dup"));
        event = EventBuilder::new(event.kind, event.content.clone())
            .tags(tags)
            .sign_with_keys(&relay)
            .expect("sign");
        assert_eq!(
            check_event(&event, Some(&relay.public_key()), "a.b.c", ANY_INSTALLATION),
            Err("application event carries more than one source tag")
        );
        let bad = EventBuilder::new(event.kind, "{}")
            .tags([
                Tag::parse(["l", "a.b.c", APPLICATION_EVENT_NAMESPACE]).expect("l"),
                Tag::parse([SOURCE_TAG, "github", "not-a-uuid"]).expect("source"),
            ])
            .sign_with_keys(&relay)
            .expect("sign");
        assert_eq!(
            check_event(&bad, Some(&relay.public_key()), "a.b.c", ANY_INSTALLATION),
            Err("application event source tag is malformed")
        );
    }

    #[test]
    fn type_patterns() {
        assert!(type_matches("a.b.c", "a.b.c"));
        assert!(!type_matches("a.b.c", "a.b.c.d"));
        assert!(type_matches("a.b.*", "a.b.c"));
        assert!(type_matches("a.b.*", "a.b.c.d"));
        assert!(!type_matches("a.b.*", "a.b"));
        assert!(!type_matches("a.b.*", "a.bc.d"));
    }

    #[test]
    fn trigger_validation() {
        for (event_type, installation) in [
            ("com.github.push", ANY_INSTALLATION),
            ("com.github.pull-request.*", INSTALLATION),
            ("com.github.*", ANY_INSTALLATION),
        ] {
            assert!(
                validate_trigger(event_type, installation).is_ok(),
                "{event_type} {installation}"
            );
        }
        for (event_type, installation) in [
            ("com.*", ANY_INSTALLATION),
            ("*", ANY_INSTALLATION),
            ("com.github", ANY_INSTALLATION),
            ("Com.github.push", ANY_INSTALLATION),
            ("com.github.push", ""),
            ("com.github.push", "everyone"),
        ] {
            assert!(
                validate_trigger(event_type, installation).is_err(),
                "{event_type} {installation:?}"
            );
        }
    }

    #[test]
    fn data_is_flattened_with_depth_and_size_bounds() {
        let relay = Keys::generate();
        let mut event = app_event(&relay, "com.github.push", Some(INSTALLATION));
        let long = "x".repeat(MAX_FLATTEN_VALUE_BYTES + 1);
        event.content = event.content.replace("LONG", &long);
        let facts = check_event(
            &event,
            Some(&relay.public_key()),
            "com.github.push",
            ANY_INSTALLATION,
        )
        .expect("match");
        let fields = trigger_fields(&facts, &event.content);
        assert_eq!(fields["app_type"], "com.github.push");
        assert_eq!(fields["installation"], INSTALLATION);
        assert_eq!(fields["plugin"], "github");
        assert_eq!(fields["data.repository.full_name"], "agiterra/beekeeper");
        assert_eq!(fields["data.repository.private"], "false");
        assert_eq!(fields["data.number"], "42");
        assert_eq!(fields["data.repository.owner.id"], "7");
        assert!(
            !fields.contains_key("data.repository.owner.deep.x"),
            "depth 4"
        );
        assert!(
            !fields.contains_key("data.labels"),
            "arrays are not flattened"
        );
        assert!(
            !fields.contains_key("data.long"),
            "over-long values are omitted"
        );
    }
}
