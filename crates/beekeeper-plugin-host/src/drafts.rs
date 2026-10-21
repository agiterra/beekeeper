//! What a plugin's draft may say, and what the host says for it.
//!
//! The rule shared by every world that hands a plugin's output to a signer:
//! **the plugin proposes data; the side that signs owns authority.** A
//! plugin proposes a draft's data tags and content. It does not decide where
//! the event goes, whose scope it lands in or whom it routes to:
//!
//! - **Scope is constructed, not checked.** The host discards the plugin's
//!   scope proposal and writes the draft's one scope tag itself — `a` for a
//!   project, `h` for a channel — from the invocation's authorized scope. A proposal that names anything else, or
//!   names it twice, refuses the whole output — so the signed event can never
//!   carry two scope tags that a first-match reader and a last-match reader
//!   would resolve differently.
//! - **Tags are an allowlist per kind.** Each kind the embedder lets a plugin
//!   draft lists the data tag keys a plugin may propose; every other key —
//!   `p`, `e`, `h`, `q`, a second `a`, anything new — is refused, never
//!   passed through.
//! - **Routing and provenance are reserved.** [`RESERVED_TAG_KEYS`] is one
//!   fixed set for every world and kind. No kind's allowlist may name one —
//!   a host refuses to load with such a kind — and the seal refuses a
//!   proposed reserved key even if an allowlist somehow listed it. The
//!   scope key of the invocation's own scope is the one exception: a plugin
//!   may echo the host's value for it, exactly once.
//! - **Caps hold twice:** on what the plugin proposed and on what the host
//!   built from it, so construction cannot push a draft past them.
//! - **References are reported, not authorized here.** A kind's validator
//!   reports the people a draft names in its content (an assignee). For
//!   kind 44248 that name is data: it sets a folded field, and grants no
//!   access, carries no `p` tag and notifies nobody, so it gets exactly the
//!   checks a directly signed op gets (well-formed pubkey). A future kind
//!   whose reference *bears authority* must be checked where the effect
//!   happens — ingest, or whatever acts on it — not only by a lookup here
//!   before signing, which a directly signed event never passes through.
//!
//! A signer (the member's client) applies the same rule again before it
//! signs: it rebuilds the scope tag from the scope it asked for. It does not
//! rely on the host, and nobody relies on a person reading a draft.

use std::sync::Arc;

use crate::Scope;

/// The tag a project-scoped draft's scope is written in. The host sets it; a
/// plugin may only echo it exactly.
pub const SCOPE_TAG: &str = "a";
/// The tag a channel-scoped draft's scope is written in (NIP-29 `h`). Same
/// rule as [`SCOPE_TAG`].
pub const CHANNEL_SCOPE_TAG: &str = "h";

/// Tag keys that route an event, scope it, point it at someone or something,
/// or state where it came from. They are the host's to write and never a
/// plugin's to propose, in any world and for any kind:
///
/// - `a`, `h` — scope (project coordinate, channel);
/// - `p`, `e`, `q`, `k` and NIP-22's `A`, `E`, `K`, `P` — references that
///   route, thread or notify;
/// - `L`, `l` — NIP-32 labels, which application events use for their
///   namespace and type, i.e. provenance;
/// - `-` — NIP-70's protected marker, which says who may publish the event;
/// - `expiration` — NIP-40, which decides when relays drop it;
/// - `delegation` — NIP-26, which claims another key's authority;
/// - `proxy` — NIP-48, which claims an origin outside Nostr.
///
/// One set for every world: Planning's allowlist (NIP-TD `td-*` keys) names
/// none of them — an assignee is content, not a `p` tag.
pub const RESERVED_TAG_KEYS: &[&str] = &[
    "a",
    "h",
    "p",
    "e",
    "q",
    "k",
    "A",
    "E",
    "K",
    "P",
    "L",
    "l",
    "-",
    "expiration",
    "delegation",
    "proxy",
];

/// `true` when `key` is in [`RESERVED_TAG_KEYS`].
pub fn is_reserved_tag(key: &str) -> bool {
    RESERVED_TAG_KEYS.contains(&key)
}
/// Tags per draft, proposed or constructed.
pub const MAX_DRAFT_TAGS: usize = 64;
/// Values per tag, the key included.
pub const MAX_TAG_VALUES: usize = 8;
/// Bytes per tag value.
pub const MAX_TAG_VALUE_BYTES: usize = 1_024;
/// Bytes across every value of every tag of one draft.
pub const MAX_DRAFT_TAG_BYTES: usize = 16 * 1_024;
/// Bytes of a draft's content.
pub const MAX_DRAFT_CONTENT_BYTES: usize = 64 * 1_024;

/// How far behind the host clock a draft may be stamped. Relay ingest takes
/// ±900 s of its clock, judged after the reply, the signing and the publish;
/// the 300 s left over is that transport margin, so a draft the host passes
/// is not refused as stale on arrival. Every signer holds the same window
/// (`beekeeper_nip_td::DRAFT_PAST_SECS`; a test here keeps them equal).
pub const DRAFT_PAST_SECS: u64 = 600;
/// How far ahead of the host clock a draft may be stamped: just inside
/// ingest's 900 s, which a later ingest only makes safer.
pub const DRAFT_FUTURE_SECS: u64 = 890;

/// Refuse a draft stamped outside `now − DRAFT_PAST_SECS ..= now +
/// DRAFT_FUTURE_SECS` (inclusive). The stamp is never rewritten: it orders
/// the op against its rivals, so changing it would change what it means.
pub fn check_created_at(created_at: u64, now: u64) -> Result<(), String> {
    if created_at.saturating_add(DRAFT_PAST_SECS) < now
        || created_at > now.saturating_add(DRAFT_FUTURE_SECS)
    {
        return Err(format!(
            "created_at {created_at} is outside {DRAFT_PAST_SECS} s behind ..= {DRAFT_FUTURE_SECS} s ahead of the host clock ({now})"
        ));
    }
    Ok(())
}

/// What a valid draft names outside its own data: the people its content
/// names. Reported for the embedder's log and its caller, not a gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftReferences {
    /// Pubkeys (64 lowercase hex) the draft names, e.g. an assignee.
    pub people: Vec<String>,
}

/// Checks one kind's sealed draft — its host-constructed tags and content —
/// and reports what it references, or the reason it is refused. The embedder
/// registers one per kind it lets any plugin draft; a kind with none can
/// never be drafted, whatever the registry grants.
pub type DraftValidator =
    Arc<dyn Fn(&[Vec<String>], &str) -> Result<DraftReferences, String> + Send + Sync>;

/// One kind a plugin may draft: the data tags it may propose and the
/// validator its sealed draft must pass.
#[derive(Clone)]
pub struct DraftKind {
    data_tags: Vec<String>,
    validate: DraftValidator,
}

impl DraftKind {
    /// `data_tags` are the keys a plugin may propose. [`SCOPE_TAG`] is never
    /// one of them: listing it changes nothing, the host still constructs it.
    pub fn new(data_tags: &[&str], validate: DraftValidator) -> Self {
        Self {
            data_tags: data_tags
                .iter()
                .filter(|key| **key != SCOPE_TAG)
                .map(|key| (*key).to_owned())
                .collect(),
            validate,
        }
    }

    /// The data tag keys a plugin may propose.
    pub fn data_tags(&self) -> &[String] {
        &self.data_tags
    }

    /// The first data tag key that is reserved ([`RESERVED_TAG_KEYS`]), if
    /// any. A host refuses to load with a kind that has one.
    pub fn reserved_data_tag(&self) -> Option<&str> {
        self.data_tags
            .iter()
            .map(String::as_str)
            .find(|key| is_reserved_tag(key))
    }

    pub(crate) fn validate(
        &self,
        tags: &[Vec<String>],
        content: &str,
    ) -> Result<DraftReferences, String> {
        (self.validate)(tags, content)
    }
}

fn check_caps(stage: &str, tags: &[Vec<String>]) -> Result<(), String> {
    if tags.len() > MAX_DRAFT_TAGS {
        return Err(format!(
            "{stage} tags: {} tags; the limit is {MAX_DRAFT_TAGS}",
            tags.len()
        ));
    }
    let mut total = 0usize;
    for tag in tags {
        if tag.is_empty() || tag.len() > MAX_TAG_VALUES {
            return Err(format!(
                "{stage} tags: a tag has {} values; each needs 1-{MAX_TAG_VALUES}",
                tag.len()
            ));
        }
        for value in tag {
            if value.len() > MAX_TAG_VALUE_BYTES {
                return Err(format!(
                    "{stage} tags: a value is {} bytes; the limit is {MAX_TAG_VALUE_BYTES}",
                    value.len()
                ));
            }
            total += value.len();
        }
    }
    if total > MAX_DRAFT_TAG_BYTES {
        return Err(format!(
            "{stage} tags: {total} bytes; the limit is {MAX_DRAFT_TAG_BYTES}"
        ));
    }
    Ok(())
}

/// Build a project-scoped draft's tags from what a plugin proposed: the
/// host's `a` tag first, then the proposed data tags in order. Refuses a
/// proposal that names another scope, names the scope more than once,
/// carries a key the kind does not allow or a reserved key, or breaks a cap
/// before or after construction. [`seal_scoped_tags`] with
/// [`Scope::Project`].
pub fn seal_tags(
    data_tags: &[String],
    coordinate: &str,
    proposed: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    seal(data_tags, SCOPE_TAG, coordinate, proposed)
}

/// [`seal_tags`] for any scope: the host writes `scope`'s own tag (`a` for a
/// project, `h` for a channel) first, and the plugin may echo exactly that
/// tag and value, once. Every other reserved key — including the other
/// world's scope key — is refused.
pub fn seal_scoped_tags(
    data_tags: &[String],
    scope: &Scope,
    proposed: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    let (key, value) = scope.tag();
    seal(data_tags, key, value, proposed)
}

fn seal(
    data_tags: &[String],
    scope_key: &str,
    scope_value: &str,
    proposed: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    check_caps("proposed", proposed)?;
    let mut sealed = Vec::with_capacity(proposed.len() + 1);
    sealed.push(vec![scope_key.to_owned(), scope_value.to_owned()]);
    let mut echoed = false;
    for tag in proposed {
        let key = tag[0].as_str();
        if key == scope_key {
            if tag.len() != 2 || tag[1] != scope_value {
                return Err(format!(
                    "proposes scope {:?}; the host sets a draft's scope to the invocation's {scope_value:?}",
                    tag.get(1).map(String::as_str).unwrap_or("")
                ));
            }
            if echoed {
                return Err("proposes the scope tag more than once".to_owned());
            }
            echoed = true;
            continue;
        }
        if !data_tags.iter().any(|allowed| allowed == key) {
            return Err(format!(
                "proposes a {key:?} tag; this kind's drafts may carry only {data_tags:?} (the host adds the scope)"
            ));
        }
        if is_reserved_tag(key) {
            // Only reachable when an allowlist names a reserved key, which a
            // host refuses at load; this is the backstop for any other caller.
            return Err(format!(
                "proposes a {key:?} tag, which is reserved: routing, scope and provenance are the host's to write"
            ));
        }
        sealed.push(tag.clone());
    }
    check_caps("constructed", &sealed)?;
    Ok(sealed)
}

/// Re-check the caps on tags the host extended after sealing (e.g. an
/// application event's provenance labels).
pub(crate) fn check_constructed_caps(tags: &[Vec<String>]) -> Result<(), String> {
    check_caps("constructed", tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosts_stamp_window_is_the_one_every_signer_holds() {
        assert_eq!(DRAFT_PAST_SECS, beekeeper_nip_td::DRAFT_PAST_SECS);
        assert_eq!(DRAFT_FUTURE_SECS, beekeeper_nip_td::DRAFT_FUTURE_SECS);
        const NOW: u64 = 1_800_000_000;
        for at in [0, NOW - 1_800, NOW - 601, NOW + 891, NOW + 86_400] {
            assert!(check_created_at(at, NOW).is_err(), "{at}");
            assert!(beekeeper_nip_td::check_draft_created_at(at, NOW).is_err());
        }
        for at in [NOW - 600, NOW, NOW + 890] {
            assert!(check_created_at(at, NOW).is_ok(), "{at}");
            assert!(beekeeper_nip_td::check_draft_created_at(at, NOW).is_ok());
        }
    }

    const HERE: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:here";
    const ELSEWHERE: &str =
        "30621:2222222222222222222222222222222222222222222222222222222222222222:elsewhere";

    fn keys() -> Vec<String> {
        vec!["td-op".to_owned(), "td-list".to_owned()]
    }

    fn tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn the_host_writes_the_scope_whether_or_not_the_plugin_echoed_it() {
        let without = seal_tags(&keys(), HERE, &[tag(&["td-op", "x"])]).unwrap();
        let with = seal_tags(&keys(), HERE, &[tag(&["td-op", "x"]), tag(&["a", HERE])]).unwrap();
        assert_eq!(without, with);
        assert_eq!(without[0], tag(&["a", HERE]));
        assert_eq!(without.iter().filter(|t| t[0] == "a").count(), 1);
    }

    #[test]
    fn a_foreign_or_repeated_or_extended_scope_is_refused_not_replaced() {
        for proposed in [
            vec![tag(&["a", ELSEWHERE])],
            vec![tag(&["a", HERE]), tag(&["a", ELSEWHERE])],
            vec![tag(&["a", ELSEWHERE]), tag(&["a", HERE])],
            vec![tag(&["a", HERE]), tag(&["a", HERE])],
            vec![tag(&["a", HERE, "wss://elsewhere"])],
            vec![tag(&["a"])],
        ] {
            assert!(seal_tags(&keys(), HERE, &proposed).is_err(), "{proposed:?}");
        }
    }

    #[test]
    fn only_the_kinds_data_tags_pass() {
        for key in ["p", "e", "h", "q", "k", "td-vis", "A", ""] {
            let error = seal_tags(&keys(), HERE, &[tag(&[key, "v"])]).unwrap_err();
            assert!(error.contains("may carry only"), "{key}: {error}");
        }
    }

    #[test]
    fn caps_hold_on_the_proposal_and_on_what_the_host_built() {
        let full: Vec<Vec<String>> = (0..MAX_DRAFT_TAGS).map(|_| tag(&["td-op", "x"])).collect();
        // 64 proposed tags pass the proposal check; adding the scope makes 65.
        let error = seal_tags(&keys(), HERE, &full).unwrap_err();
        assert!(error.starts_with("constructed tags"), "{error}");
        let mut over = full.clone();
        over.push(tag(&["td-op", "x"]));
        assert!(seal_tags(&keys(), HERE, &over)
            .unwrap_err()
            .starts_with("proposed tags"));
        // The same 64 with the scope echoed among them stay at 64.
        let mut echoed = full[..MAX_DRAFT_TAGS - 1].to_vec();
        echoed.push(tag(&["a", HERE]));
        assert_eq!(
            seal_tags(&keys(), HERE, &echoed).unwrap().len(),
            MAX_DRAFT_TAGS
        );
        let long = "x".repeat(MAX_TAG_VALUE_BYTES + 1);
        assert!(seal_tags(&keys(), HERE, &[tag(&["td-op", &long])]).is_err());
        let wide = vec!["td-op".to_owned(); MAX_TAG_VALUES + 1];
        assert!(seal_tags(&keys(), HERE, &[wide]).is_err());
        assert!(seal_tags(&keys(), HERE, &[vec![]]).is_err());
        let bulky: Vec<Vec<String>> = (0..20)
            .map(|_| tag(&["td-op", &"y".repeat(1_000)]))
            .collect();
        assert!(seal_tags(&keys(), HERE, &bulky)
            .unwrap_err()
            .contains("bytes"));
    }

    const CHANNEL: &str = "6f1c2b9e-3d4a-4f5b-8c7d-9e0f1a2b3c4d";
    const OTHER_CHANNEL: &str = "00000000-0000-4000-8000-000000000000";

    fn channel() -> Scope {
        Scope::Channel(CHANNEL.to_owned())
    }

    #[test]
    fn a_channel_scope_is_written_as_h_first_and_may_be_echoed_once() {
        let without = seal_scoped_tags(&keys(), &channel(), &[tag(&["td-op", "x"])]).unwrap();
        let with = seal_scoped_tags(
            &keys(),
            &channel(),
            &[tag(&["td-op", "x"]), tag(&["h", CHANNEL])],
        )
        .unwrap();
        assert_eq!(without, with);
        assert_eq!(without[0], tag(&["h", CHANNEL]));
        assert_eq!(without.iter().filter(|t| t[0] == "h").count(), 1);
        assert!(!without.iter().any(|t| t[0] == "a"));
    }

    #[test]
    fn a_foreign_repeated_or_extended_channel_is_refused() {
        for proposed in [
            vec![tag(&["h", OTHER_CHANNEL])],
            vec![tag(&["h", CHANNEL]), tag(&["h", CHANNEL])],
            vec![tag(&["h", CHANNEL]), tag(&["h", OTHER_CHANNEL])],
            vec![tag(&["h", CHANNEL, "wss://elsewhere"])],
            vec![tag(&["h"])],
        ] {
            let error = seal_scoped_tags(&keys(), &channel(), &proposed).unwrap_err();
            assert!(error.contains("scope"), "{proposed:?}: {error}");
        }
        // The other world's scope key is not an echo; it is refused.
        assert!(seal_scoped_tags(&keys(), &channel(), &[tag(&["a", HERE])]).is_err());
        assert!(seal_scoped_tags(
            &keys(),
            &Scope::Project(HERE.to_owned()),
            &[tag(&["h", CHANNEL])]
        )
        .is_err());
    }

    #[test]
    fn a_project_scope_seals_the_same_through_either_entry_point() {
        let proposed = [tag(&["a", HERE]), tag(&["td-op", "x"])];
        assert_eq!(
            seal_tags(&keys(), HERE, &proposed).unwrap(),
            seal_scoped_tags(&keys(), &Scope::Project(HERE.to_owned()), &proposed).unwrap()
        );
    }

    #[test]
    fn a_reserved_key_is_refused_even_when_an_allowlist_names_it() {
        for key in RESERVED_TAG_KEYS {
            let mut allowed = keys();
            allowed.push((*key).to_owned());
            for scope in [Scope::Project(HERE.to_owned()), channel()] {
                if scope.tag().0 == *key {
                    continue;
                }
                let error = seal_scoped_tags(&allowed, &scope, &[tag(&[key, "v"])]).unwrap_err();
                assert!(error.contains("reserved"), "{key}: {error}");
            }
        }
        // Accept path: the same allowlist still passes its data keys.
        let mut allowed = keys();
        allowed.push("p".to_owned());
        assert!(seal_scoped_tags(&allowed, &channel(), &[tag(&["td-op", "x"])]).is_ok());
    }

    #[test]
    fn protection_expiry_delegation_and_proxy_are_reserved() {
        for key in ["-", "expiration", "delegation", "proxy"] {
            assert!(is_reserved_tag(key), "{key}");
            let kind = DraftKind::new(
                &["td-op", key],
                Arc::new(|_, _| Ok(DraftReferences::default())),
            );
            assert_eq!(kind.reserved_data_tag(), Some(key));
        }
    }

    #[test]
    fn planning_names_no_reserved_key_and_a_kind_that_does_reports_it() {
        let planning = DraftKind::new(
            &beekeeper_nip_td::project_todo::PROJECT_TODO_DATA_TAG_KEYS,
            Arc::new(|_, _| Ok(DraftReferences::default())),
        );
        assert_eq!(planning.reserved_data_tag(), None);
        let bad = DraftKind::new(
            &["td-op", "p"],
            Arc::new(|_, _| Ok(DraftReferences::default())),
        );
        assert_eq!(bad.reserved_data_tag(), Some("p"));
    }

    #[test]
    fn listing_the_scope_tag_as_data_does_not_let_a_plugin_set_it() {
        let kind = DraftKind::new(
            &["a", "td-op"],
            Arc::new(|_, _| Ok(DraftReferences::default())),
        );
        assert_eq!(kind.data_tags(), &["td-op".to_owned()]);
        assert!(seal_tags(kind.data_tags(), HERE, &[tag(&["a", ELSEWHERE])]).is_err());
    }
}
