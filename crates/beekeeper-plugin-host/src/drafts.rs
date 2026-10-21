//! What a plugin's draft may say, and what the host says for it.
//!
//! The rule shared by every world that hands a plugin's output to a signer:
//! **the plugin proposes data; the side that signs owns authority.** A
//! plugin proposes a draft's data tags and content. It does not decide where
//! the event goes, whose scope it lands in or whom it routes to:
//!
//! - **Scope is constructed, not checked.** The host discards the plugin's
//!   scope proposal and writes the draft's one `a` tag itself, from the
//!   invocation's authorized scope. A proposal that names anything else, or
//!   names it twice, refuses the whole output — so the signed event can never
//!   carry two scope tags that a first-match reader and a last-match reader
//!   would resolve differently.
//! - **Tags are an allowlist per kind.** Each kind the embedder lets a plugin
//!   draft lists the data tag keys a plugin may propose; every other key —
//!   `p`, `e`, `h`, `q`, a second `a`, anything new — is refused, never
//!   passed through.
//! - **Caps hold twice:** on what the plugin proposed and on what the host
//!   built from it, so construction cannot push a draft past them.
//! - **References are the embedder's to authorize.** A kind's validator
//!   reports the people a draft names in its content (an assignee); the
//!   embedder decides whether each is a legitimate target in the scope before
//!   any draft reaches a signer.
//!
//! A signer (the member's client) applies the same rule again before it
//! signs: it rebuilds the scope tag from the scope it asked for. It does not
//! rely on the host, and nobody relies on a person reading a draft.

use std::sync::Arc;

/// The tag a draft's scope is written in. The host sets it; a plugin may
/// only echo it exactly.
pub const SCOPE_TAG: &str = "a";
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

/// What a valid draft names outside its own data: the people its content
/// routes to. The embedder authorizes each against the invocation's scope.
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

/// Build a draft's tags from what a plugin proposed: the host's scope tag
/// first, then the proposed data tags in order. Refuses a proposal that
/// names another scope, names the scope more than once, carries a key the
/// kind does not allow, or breaks a cap before or after construction.
pub fn seal_tags(
    data_tags: &[String],
    coordinate: &str,
    proposed: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    check_caps("proposed", proposed)?;
    let mut sealed = Vec::with_capacity(proposed.len() + 1);
    sealed.push(vec![SCOPE_TAG.to_owned(), coordinate.to_owned()]);
    let mut echoed = false;
    for tag in proposed {
        let key = tag[0].as_str();
        if key == SCOPE_TAG {
            if tag.len() != 2 || tag[1] != coordinate {
                return Err(format!(
                    "proposes scope {:?}; the host sets a draft's scope to the invocation's {coordinate:?}",
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
        sealed.push(tag.clone());
    }
    check_caps("constructed", &sealed)?;
    Ok(sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

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
