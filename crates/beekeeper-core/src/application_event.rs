//! Envelope rules for kind:50000 application events.
//!
//! An application event carries one channel `h` tag, the
//! [`APPLICATION_EVENT_NAMESPACE`] label namespace (`L`), one type label (`l`)
//! in that namespace, and JSON content `{"schema": n >= 1, "data": {...}}`.
//! Ingest enforces this for member-signed events; the relay's own producers
//! (webhook plugins) must build events that pass the same check, which is
//! why the rules live here rather than in the relay's ingest handler.

/// NIP-32 label namespace that marks an application event's type label.
pub const APPLICATION_EVENT_NAMESPACE: &str = "org.agiterra.application-event";

/// Why an application event's envelope was refused.
///
/// The `Display` text is the reason alone (for example "requires exactly one
/// h, L, and l tag"); callers add their own prefix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidApplicationEvent(&'static str);

impl InvalidApplicationEvent {
    /// The reason the envelope was refused.
    pub fn reason(&self) -> &'static str {
        self.0
    }
}

/// Check an event against the application-event envelope rules.
///
/// Does not check the event kind or signature; the caller has already
/// decided this is a kind:50000 event.
pub fn validate_application_event(event: &nostr::Event) -> Result<(), InvalidApplicationEvent> {
    let mut h = 0;
    let mut namespace = 0;
    let mut label = 0;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        match parts.first().map(String::as_str) {
            Some("h") => {
                h += 1;
                if parts.len() != 2 || uuid::Uuid::parse_str(&parts[1]).is_err() {
                    return Err(InvalidApplicationEvent("requires one valid channel h tag"));
                }
            }
            Some("L") => {
                namespace += 1;
                if parts.len() != 2 || parts[1] != APPLICATION_EVENT_NAMESPACE {
                    return Err(InvalidApplicationEvent("has an invalid namespace tag"));
                }
            }
            Some("l") => {
                label += 1;
                if parts.len() != 3
                    || parts[2] != APPLICATION_EVENT_NAMESPACE
                    || !valid_application_event_type(&parts[1])
                {
                    return Err(InvalidApplicationEvent("has an invalid type tag"));
                }
            }
            _ => {}
        }
    }
    if h != 1 || namespace != 1 || label != 1 {
        return Err(InvalidApplicationEvent(
            "requires exactly one h, L, and l tag",
        ));
    }
    let content: serde_json::Value = serde_json::from_str(&event.content)
        .map_err(|_| InvalidApplicationEvent("content must be JSON"))?;
    if content
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .is_none_or(|v| v == 0)
        || !content
            .get("data")
            .is_some_and(serde_json::Value::is_object)
    {
        return Err(InvalidApplicationEvent(
            "content requires an integer schema and object data",
        ));
    }
    Ok(())
}

/// Whether `value` is a well-formed application event type.
///
/// At most 128 bytes, at least three dot-separated components, each 1-63
/// bytes of lowercase ASCII letters, digits and inner hyphens.
pub fn valid_application_event_type(value: &str) -> bool {
    value.len() <= 128
        && value.split('.').count() >= 3
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const KIND: u16 = crate::kind::KIND_APPLICATION_EVENT as u16;

    fn application_event(tags: Vec<Tag>, content: &str) -> nostr::Event {
        EventBuilder::new(Kind::Custom(KIND), content)
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign application event")
    }

    fn application_tags() -> Vec<Tag> {
        vec![
            Tag::parse(["h", "c42dcf94-1a36-4ce5-b663-5903aa10fbdb"]).expect("h tag"),
            Tag::parse(["L", APPLICATION_EVENT_NAMESPACE]).expect("L tag"),
            Tag::parse([
                "l",
                "org.example.build.completed",
                APPLICATION_EVENT_NAMESPACE,
            ])
            .expect("l tag"),
        ]
    }

    #[test]
    fn accepts_a_well_formed_envelope() {
        let valid = application_event(application_tags(), r#"{"schema":1,"data":{"id":"b-1"}}"#);
        assert_eq!(validate_application_event(&valid), Ok(()));
    }

    #[test]
    fn rejects_missing_duplicate_and_malformed_tags() {
        let cases: Vec<(Vec<Tag>, &str)> = vec![
            (
                application_tags()[1..].to_vec(),
                "requires exactly one h, L, and l tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags.push(tags[0].clone());
                    tags
                },
                "requires exactly one h, L, and l tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags.push(tags[1].clone());
                    tags
                },
                "requires exactly one h, L, and l tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags.push(tags[2].clone());
                    tags
                },
                "requires exactly one h, L, and l tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags[0] = Tag::parse(["h", "invalid"]).expect("tag");
                    tags
                },
                "requires one valid channel h tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags[1] = Tag::parse(["L", "other.namespace"]).expect("tag");
                    tags
                },
                "has an invalid namespace tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags[2] = Tag::parse(["l", "bad", APPLICATION_EVENT_NAMESPACE]).expect("tag");
                    tags
                },
                "has an invalid type tag",
            ),
            (
                {
                    let mut tags = application_tags();
                    tags[2] = Tag::parse(["l", "org.example.build.completed", "wrong.namespace"])
                        .expect("tag");
                    tags
                },
                "has an invalid type tag",
            ),
        ];
        for (tags, reason) in cases {
            let error = validate_application_event(&application_event(
                tags.clone(),
                r#"{"schema":1,"data":{}}"#,
            ))
            .expect_err("malformed tags must be refused");
            assert_eq!(error.reason(), reason, "{tags:?}");
        }
    }

    #[test]
    fn rejects_bad_content() {
        for content in [
            "not json",
            "[]",
            r#"{"schema":0,"data":{}}"#,
            r#"{"schema":"1","data":{}}"#,
            r#"{"schema":1,"data":[]}"#,
            r#"{"data":{}}"#,
            r#"{"schema":1}"#,
        ] {
            assert!(
                validate_application_event(&application_event(application_tags(), content))
                    .is_err(),
                "{content}"
            );
        }
    }

    #[test]
    fn type_rules() {
        for good in [
            "org.example.build",
            "com.github.pull-request.opened",
            "a.b.c",
            "x1.y-2.z3",
        ] {
            assert!(valid_application_event_type(good), "{good}");
        }
        let long_part = "a".repeat(64);
        // Every component is valid; only the total length (191 bytes) is not.
        let too_long = format!("{0}.{0}.{0}", "a".repeat(63));
        for bad in [
            "a.b",
            "a..b",
            "A.b.c",
            "a.-b.c",
            "a.b-.c",
            "a.b_c.d",
            &format!("{long_part}.b.c"),
            &too_long,
        ] {
            assert!(!valid_application_event_type(bad), "{bad}");
        }
        assert!(valid_application_event_type(&format!(
            "{}.b.c",
            "a".repeat(63)
        )));
        // 128 bytes exactly is the ceiling, not over it.
        assert!(valid_application_event_type(&format!(
            "{}.{}.c",
            "a".repeat(63),
            "b".repeat(62)
        )));
    }
}
