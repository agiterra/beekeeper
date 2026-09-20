use nostr::{EventBuilder, EventId, Kind};

use super::{check_content, tag};

/// Kind 30620 — replaceable workflow definition.
///
/// The `d` tag carries the workflow id; `h` tag carries the channel id; the
/// content is the YAML definition. Same (pubkey, d) replaces the prior version.
pub fn build_workflow_definition(
    workflow_id: &str,
    channel_id: &str,
    yaml_definition: &str,
    expected_revision: Option<&str>,
) -> Result<EventBuilder, String> {
    check_content(yaml_definition)?;
    let mut tags = vec![tag(vec!["d", workflow_id])?, tag(vec!["h", channel_id])?];
    if let Some(revision) = expected_revision {
        EventId::from_hex(revision).map_err(|_| "invalid workflow revision".to_string())?;
        tags.push(tag(vec!["expected-revision", revision])?);
    }
    Ok(EventBuilder::new(Kind::Custom(30620), yaml_definition.to_string()).tags(tags))
}

/// Kind 5 — NIP-09 deletion targeting a kind:30620 workflow definition.
pub fn build_workflow_delete(
    workflow_id: &str,
    owner_pubkey_hex: &str,
) -> Result<EventBuilder, String> {
    let coord = format!("30620:{owner_pubkey_hex}:{workflow_id}");
    let tags = vec![tag(vec!["a", &coord])?];
    Ok(EventBuilder::new(Kind::Custom(5), "").tags(tags))
}

/// A manual trigger's bound commit, exactly as the relay accepts it.
///
/// Lane 184: the relay normalizes the `checkout` field of a kind:46020 and
/// refuses anything that is not a full 40-hex commit sha
/// (`invalid: checkout must be a full 40-hex commit sha`). Refusing the same
/// shape here means the operator is told before an event is signed, in the
/// same words, rather than after a round trip.
pub fn normalize_checkout_sha(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() != 40 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("checkout must be a full 40-hex commit sha".to_string());
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Kind 46020 — trigger a workflow run by id, optionally bound to a commit.
///
/// With `checkout`, the content is `{"checkout": "<40-hex>"}`, which is what
/// `bee workflows trigger --checkout` publishes and what
/// `TriggerContext::checkout` carries into the run record. Without it the
/// content stays empty, byte-identical to every trigger this app signed
/// before lane 184 — an action whose step declares `checkout: required` is
/// then refused by the relay, naming the field, instead of running against
/// whatever the recorded project folder happens to hold.
pub fn build_workflow_trigger(
    workflow_id: &str,
    checkout: Option<&str>,
) -> Result<EventBuilder, String> {
    let tags = vec![tag(vec!["d", workflow_id])?];
    let content = match checkout {
        None => String::new(),
        Some(value) => {
            let sha = normalize_checkout_sha(value)?;
            serde_json::json!({ "checkout": sha }).to_string()
        }
    };
    Ok(EventBuilder::new(Kind::Custom(46020), content).tags(tags))
}

/// Lowercase hex of the stored approval token hash, as the relay's approvals
/// listing returns it (`approval_ref`).
///
/// The relay resolves a kind:46030/46031 by its `d` tag (or an `e` tag naming
/// the kind:46010 request), never by a `t` tag: see
/// `crates/buzz-relay/src/handlers/command_executor.rs` `handle_approval_grant`.
fn approval_ref_tag(approval_ref: &str) -> Result<nostr::Tag, String> {
    let approval_ref = approval_ref.trim();
    if approval_ref.len() != 64 || !approval_ref.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid approval reference: expected 64 hex characters".to_string());
    }
    tag(vec!["d", &approval_ref.to_ascii_lowercase()])
}

/// Kind 46030 — grant an approval by its reference, with an optional note
/// and the scope it releases: `run` (this run) or `action` (every later run
/// of the same definition hash, spec § 5.4). The content is the `{note,
/// scope}` JSON the relay's `decode_approval_grant_content` reads.
pub fn build_approval_grant(
    approval_ref: &str,
    note: Option<&str>,
    scope: buzz_core_pkg::workflow_autorun::ApprovalScope,
) -> Result<EventBuilder, String> {
    let tags = vec![approval_ref_tag(approval_ref)?];
    let content = buzz_core_pkg::workflow_autorun::encode_approval_grant_content(note, scope);
    Ok(EventBuilder::new(Kind::Custom(46030), content).tags(tags))
}

/// Kind 46032 — revoke every autorun grant of a workflow.
pub fn build_autorun_revoke(workflow_id: &str, channel_id: &str) -> Result<EventBuilder, String> {
    let revoke = buzz_core_pkg::workflow_autorun::AutorunRevoke {
        schema: buzz_core_pkg::workflow_autorun::AUTORUN_SCHEMA.into(),
        workflow_id: workflow_id.trim().to_ascii_lowercase(),
        channel_id: channel_id.trim().to_ascii_lowercase(),
    };
    let (tags, content) = buzz_core_pkg::workflow_autorun::build_autorun_revoke(&revoke)?;
    let tags = tags
        .into_iter()
        .map(|parts| tag(parts.iter().map(String::as_str).collect()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(
        Kind::Custom(buzz_core_pkg::kind::KIND_WORKFLOW_AUTORUN_REVOKE as u16),
        content,
    )
    .tags(tags))
}

/// Kind 46031 — deny an approval by its reference (with optional note).
pub fn build_approval_deny(approval_ref: &str, note: Option<&str>) -> Result<EventBuilder, String> {
    let tags = vec![approval_ref_tag(approval_ref)?];
    Ok(EventBuilder::new(Kind::Custom(46031), note.unwrap_or("")).tags(tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unbound_trigger_keeps_its_empty_content() {
        let event = build_workflow_trigger("wf-1", None).expect("builder");
        let event = event
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign");
        assert_eq!(event.content, "");
    }

    #[test]
    fn a_bound_trigger_names_the_commit_the_relay_reads() {
        let raw = format!("  {}  ", "AB".repeat(20));
        let event = build_workflow_trigger("wf-1", Some(&raw))
            .expect("builder")
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign");
        assert_eq!(
            event.content,
            format!("{{\"checkout\":\"{}\"}}", "ab".repeat(20))
        );
    }

    #[test]
    fn a_short_or_non_hex_commit_is_refused_in_the_relays_own_words() {
        for bad in ["abc", &"a".repeat(39), &"z".repeat(40), ""] {
            assert_eq!(
                build_workflow_trigger("wf-1", Some(bad)).unwrap_err(),
                "checkout must be a full 40-hex commit sha"
            );
        }
    }
}
