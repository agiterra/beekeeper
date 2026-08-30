//! `bee sessions catalog` — read the live kind:44222 provider catalog.
//!
//! The catalog is the only list of models this product offers. Everything that
//! picks a model — a create, a hire, the lead's rubric — is answerable against
//! it, and until this command existed the only way to see it was to query the
//! relay by hand and read raw JSON. A model id that is not in here is not on
//! offer; the honest response to one is to name it and refuse, never to map it
//! onto a neighbouring id that happens to be offered.
//!
//! # What "the catalog" means when several signers publish one
//!
//! Each provider host signs its own catalog and bumps its own revision, so
//! there is no single document and no shared clock between signers. This
//! command therefore reconciles nothing: it keeps each signer's **newest**
//! catalog (highest revision, ties broken by `created_at` then event id) and
//! prints every row it finds, with the signer on the row. Two signers
//! describing one `providerInstanceRef` differently is a real state of the
//! world and is shown as two rows rather than averaged into one.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use buzz_core::coding_session_catalog::{parse_catalog, Catalog, CatalogModel};
use buzz_core::kind::KIND_CODING_SESSION_PROVIDER_CATALOG;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

/// One signed catalog that parsed cleanly.
#[derive(Debug, Clone)]
pub struct CatalogRecord {
    /// Event id (64-char hex).
    pub event_id: String,
    /// Signing pubkey (64-char hex).
    pub signer: String,
    /// `created_at`, seconds since the epoch.
    pub created_at: i64,
    /// The parsed body.
    pub catalog: Catalog,
}

/// What one read of a channel's catalogs found.
#[derive(Debug, Clone, Default)]
pub struct CatalogSnapshot {
    /// Each signer's newest catalog, sorted by signer.
    pub records: Vec<CatalogRecord>,
    /// Bodies that did not parse, with the reason, sorted by event id.
    ///
    /// Reported rather than dropped: a provider whose catalog is unreadable is
    /// a provider whose models are invisible, and a reader that silently
    /// skipped it would describe the offer as smaller than it is without
    /// saying so.
    pub malformed: Vec<(String, String)>,
}

impl CatalogSnapshot {
    /// Every `(providerInstanceRef, model id)` pair any signer offers.
    ///
    /// The union, deliberately: the question a create or a rubric asks is
    /// "does anything here serve this id", and one signer offering it is
    /// enough for the answer to be yes.
    pub fn offered_pairs(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .records
            .iter()
            .flat_map(|record| record.catalog.providers.iter())
            .flat_map(|provider| {
                provider
                    .allowed_models
                    .iter()
                    .map(|model| (provider.provider_instance_ref.clone(), model.clone()))
            })
            .collect();
        pairs.sort();
        pairs.dedup();
        pairs
    }
}

/// Read every 44222 in a channel and keep the newest per signer.
pub async fn load_catalogs(
    client: &BuzzClient,
    channel_id: &str,
) -> Result<CatalogSnapshot, CliError> {
    validate_uuid(channel_id)?;
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_PROVIDER_CATALOG],
        "#h": [channel_id],
    });
    let events = client.query_all(filter).await?;
    Ok(snapshot_from_events(&events))
}

/// Fold raw events into a snapshot. Pure, so the rules are testable offline.
pub fn snapshot_from_events(events: &[Value]) -> CatalogSnapshot {
    let mut newest: BTreeMap<String, CatalogRecord> = BTreeMap::new();
    let mut malformed: Vec<(String, String)> = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_PROVIDER_CATALOG))
        {
            continue;
        }
        let (Some(event_id), Some(signer), Some(created_at), Some(content)) = (
            event.get("id").and_then(Value::as_str),
            event.get("pubkey").and_then(Value::as_str),
            event.get("created_at").and_then(Value::as_i64),
            event.get("content").and_then(Value::as_str),
        ) else {
            malformed.push((
                event
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("<no id>")
                    .to_owned(),
                "event is missing id, pubkey, created_at or content".to_owned(),
            ));
            continue;
        };
        let catalog = match parse_catalog(content) {
            Ok(catalog) => catalog,
            Err(error) => {
                malformed.push((event_id.to_owned(), error.to_string()));
                continue;
            }
        };
        let record = CatalogRecord {
            event_id: event_id.to_owned(),
            signer: signer.to_owned(),
            created_at,
            catalog,
        };
        match newest.get(signer) {
            Some(held) if !is_newer(&record, held) => {}
            _ => {
                newest.insert(signer.to_owned(), record);
            }
        }
    }
    malformed.sort();
    malformed.dedup();
    CatalogSnapshot {
        records: newest.into_values().collect(),
        malformed,
    }
}

/// Revision first, then `created_at`, then event id — the same order the
/// desktop breaks a tie on, so the two never disagree about which catalog is
/// current for a signer.
fn is_newer(candidate: &CatalogRecord, held: &CatalogRecord) -> bool {
    (
        candidate.catalog.revision,
        candidate.created_at,
        candidate.event_id.as_str(),
    ) > (
        held.catalog.revision,
        held.created_at,
        held.event_id.as_str(),
    )
}

/// A model's metadata row, or an empty object when nothing described it.
fn described<'a>(models: &'a [CatalogModel], id: &str) -> Option<&'a CatalogModel> {
    models.iter().find(|model| model.id == id)
}

/// `bee sessions catalog --channel <uuid>`.
pub async fn cmd_catalog(
    client: &BuzzClient,
    channel_id: &str,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let snapshot = load_catalogs(client, channel_id).await?;
    let mut rows: Vec<Value> = Vec::new();
    for record in &snapshot.records {
        for provider in &record.catalog.providers {
            for model in &provider.allowed_models {
                let facts = described(&provider.models, model);
                // Absent means nobody said. `null` is that fact on the wire;
                // it is never a stand-in for a default the reader may assume.
                let context_window = facts.and_then(|facts| facts.context_window);
                let family = facts.and_then(|facts| facts.family.clone());
                let vendor = facts.and_then(|facts| facts.vendor.clone());
                let deprecated = facts.and_then(|facts| facts.deprecated);
                rows.push(match format {
                    crate::OutputFormat::Compact => json!({
                        "provider": provider.provider_instance_ref,
                        "model": model,
                        "default": *model == provider.default_model,
                        "contextWindow": context_window,
                        "vendor": vendor,
                    }),
                    crate::OutputFormat::Json => json!({
                        "provider": provider.provider_instance_ref,
                        "driver": provider.driver,
                        "runtime": provider.runtime,
                        "model": model,
                        "default": *model == provider.default_model,
                        "contextWindow": context_window,
                        "family": family,
                        "vendor": vendor,
                        "deprecated": deprecated,
                        "signer": record.signer,
                        "revision": record.catalog.revision,
                        "eventId": record.event_id,
                    }),
                });
            }
        }
    }

    match format {
        crate::OutputFormat::Compact => println!("{}", Value::Array(rows)),
        crate::OutputFormat::Json => {
            let malformed: Vec<Value> = snapshot
                .malformed
                .iter()
                .map(|(event_id, reason)| json!({ "eventId": event_id, "reason": reason }))
                .collect();
            println!(
                "{}",
                json!({
                    "channel": channel_id,
                    "catalogs": snapshot
                        .records
                        .iter()
                        .map(|record| json!({
                            "signer": record.signer,
                            "revision": record.catalog.revision,
                            "eventId": record.event_id,
                        }))
                        .collect::<Vec<Value>>(),
                    "models": rows,
                    "malformed": malformed,
                })
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bodies are built through the publisher's own canonical serializer, not
    /// hand-written JSON: the reader compares bytes, so a fixture assembled by
    /// any other route would be testing the fixture rather than the rule.
    fn body(revision: u64, refs: &[(&str, &[&str])]) -> String {
        use buzz_core::coding_session_catalog::{
            to_canonical_json, Catalog, CatalogProvider, CATALOG_SCHEMA,
        };
        use buzz_core::coding_session_payload::Capabilities;

        let providers: Vec<CatalogProvider> = refs
            .iter()
            .map(|(instance_ref, models)| {
                let mut allowed: Vec<String> = models[1..]
                    .iter()
                    .map(|model| (*model).to_owned())
                    .collect();
                allowed.sort();
                allowed.insert(0, models[0].to_owned());
                CatalogProvider {
                    provider_instance_ref: (*instance_ref).to_owned(),
                    driver: "claude-agent-acp".into(),
                    runtime: "claude".into(),
                    default_model: models[0].to_owned(),
                    allowed_models: allowed,
                    capabilities: Capabilities::v1_claude(),
                    models: Vec::new(),
                }
            })
            .collect();
        to_canonical_json(&Catalog {
            schema: CATALOG_SCHEMA.to_owned(),
            revision,
            providers,
            projects: Vec::new(),
        })
        .expect("serialize")
    }

    fn event(id: &str, signer: &str, created_at: i64, content: String) -> Value {
        json!({
            "id": id,
            "pubkey": signer,
            "created_at": created_at,
            "kind": KIND_CODING_SESSION_PROVIDER_CATALOG,
            "content": content,
        })
    }

    #[test]
    fn the_newest_revision_per_signer_wins() {
        let snapshot = snapshot_from_events(&[
            event(
                "a1",
                "signer-1",
                100,
                body(1, &[("claude-primary", &["opus[1m]"])]),
            ),
            event(
                "a2",
                "signer-1",
                90,
                body(2, &[("claude-primary", &["sonnet", "haiku"])]),
            ),
        ]);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].catalog.revision, 2);
        assert_eq!(
            snapshot.offered_pairs(),
            vec![
                ("claude-primary".to_owned(), "haiku".to_owned()),
                ("claude-primary".to_owned(), "sonnet".to_owned()),
            ]
        );
    }

    /// Two hosts are two offers. Neither is the other's newer revision, and
    /// folding them into one would invent a catalog nobody signed.
    #[test]
    fn two_signers_are_two_catalogs() {
        let snapshot = snapshot_from_events(&[
            event(
                "a1",
                "signer-1",
                100,
                body(9, &[("claude-primary", &["opus[1m]"])]),
            ),
            event(
                "b1",
                "signer-2",
                10,
                body(1, &[("codex-primary", &["gpt-5.6-sol"])]),
            ),
        ]);
        assert_eq!(snapshot.records.len(), 2);
        assert_eq!(
            snapshot.offered_pairs(),
            vec![
                ("claude-primary".to_owned(), "opus[1m]".to_owned()),
                ("codex-primary".to_owned(), "gpt-5.6-sol".to_owned()),
            ]
        );
    }

    /// An unreadable catalog is reported, not skipped: a provider whose models
    /// cannot be read is a provider whose models are invisible, and quietly
    /// shrinking the offer is how a picker starts lying.
    #[test]
    fn a_malformed_body_is_named_rather_than_dropped() {
        let snapshot = snapshot_from_events(&[
            event("bad", "signer-1", 100, "{\"schema\":\"nope\"}".to_owned()),
            event(
                "ok",
                "signer-2",
                100,
                body(1, &[("claude-primary", &["sonnet"])]),
            ),
        ]);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.malformed.len(), 1);
        assert_eq!(snapshot.malformed[0].0, "bad");
        assert!(!snapshot.malformed[0].1.is_empty());
    }

    #[test]
    fn events_of_other_kinds_are_ignored() {
        let mut foreign = event(
            "x",
            "signer-1",
            1,
            body(1, &[("claude-primary", &["sonnet"])]),
        );
        foreign["kind"] = json!(44223);
        let snapshot = snapshot_from_events(&[foreign]);
        assert!(snapshot.records.is_empty());
        assert!(snapshot.malformed.is_empty());
    }

    #[test]
    fn a_tie_on_revision_and_time_is_broken_on_event_id() {
        let snapshot = snapshot_from_events(&[
            event("bbb", "s", 100, body(3, &[("claude-primary", &["sonnet"])])),
            event("aaa", "s", 100, body(3, &[("claude-primary", &["haiku"])])),
        ]);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].event_id, "bbb");
    }
}
