//! Environment variables across the `BUZZ_*` → `BEEKEEPER_*` rename.
//!
//! Every variable Beekeeper reads is now spelled `BEEKEEPER_<X>`. The old
//! `BUZZ_<X>` spelling still arrives from operators' `.env` files, unit files,
//! shells, stored agent records and, during the transition, from parents and
//! children built before the rename. Three rules keep both working without
//! letting either change meaning:
//!
//! 1. **Children adopt.** A binary calls [`adopt_legacy_env`] first thing in
//!    its synchronous `main`. Every `BUZZ_<X>` whose `BEEKEEPER_<X>` is unset
//!    is copied across, so the rest of the program reads one spelling. The new
//!    name always wins, so a stale legacy value can never override a current
//!    one.
//! 2. **Parents dual-write.** An env map handed to a child goes through
//!    [`with_legacy_mirrors`] so an older child (an old `bee` on `PATH`, a
//!    digest-pinned Kubernetes image) still finds the names it knows.
//! 3. **Every list that removes, clears, reserves or fences a variable names
//!    both spellings**, built with [`both_spellings`]. Otherwise a stale
//!    inherited `BUZZ_AUTH_TAG` would survive a clear and then be adopted as
//!    `BEEKEEPER_AUTH_TAG`.

use std::collections::BTreeMap;

/// The prefix every Beekeeper environment variable now carries.
pub const PREFIX: &str = "BEEKEEPER_";

/// The prefix the same variables carried before the rename.
pub const LEGACY_PREFIX: &str = "BUZZ_";

/// Whether parents also emit the legacy spelling for their children.
///
/// True for the transition, so a child built before the rename still finds
/// its configuration. Turn it off once nothing older is in the field.
pub const EMIT_LEGACY_ENV_NAMES: bool = true;

/// `BUZZ_<X>` for a `BEEKEEPER_<X>` name; `None` for any other name.
pub fn legacy_twin(name: &str) -> Option<String> {
    name.strip_prefix(PREFIX)
        .filter(|rest| !rest.is_empty())
        .map(|rest| format!("{LEGACY_PREFIX}{rest}"))
}

/// `BEEKEEPER_<X>` for a `BUZZ_<X>` name; `None` for any other name.
pub fn canonical_name(name: &str) -> Option<String> {
    name.strip_prefix(LEGACY_PREFIX)
        .filter(|rest| !rest.is_empty())
        .map(|rest| format!("{PREFIX}{rest}"))
}

/// `name` followed by its other spelling, when it has one.
///
/// For building the lists that must cover both: clear lists, reserved keys,
/// `env_remove` loops.
pub fn both_spellings(name: &str) -> Vec<String> {
    let mut out = vec![name.to_owned()];
    if let Some(other) = legacy_twin(name).or_else(|| canonical_name(name)) {
        out.push(other);
    }
    out
}

/// What [`plan_adoption`] decided for one environment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Adoption {
    /// `(BEEKEEPER_<X>, value)` pairs to set, sorted by name.
    pub set: Vec<(String, String)>,
    /// Legacy names that were ignored because the new name was already set.
    pub shadowed: Vec<String>,
}

/// Decide which legacy variables to adopt from `vars`, without touching the
/// process environment. [`adopt_legacy_env`] applies the result.
pub fn plan_adoption<I>(vars: I) -> Adoption
where
    I: IntoIterator<Item = (String, String)>,
{
    let vars: BTreeMap<String, String> = vars.into_iter().collect();
    let mut adoption = Adoption::default();
    for (name, value) in &vars {
        let Some(canonical) = canonical_name(name) else {
            continue;
        };
        if vars.contains_key(&canonical) {
            adoption.shadowed.push(name.clone());
        } else {
            adoption.set.push((canonical, value.clone()));
        }
    }
    adoption
}

/// Copy every `BUZZ_<X>` to an unset `BEEKEEPER_<X>` in this process's
/// environment, and say so on stderr.
///
/// Call it first in a synchronous `main`, before argument parsing, a tokio
/// runtime or any other thread exists: `std::env::set_var` is only sound
/// while the process is single-threaded. Names are printed, never values.
/// Variables whose names or values are not valid Unicode are left alone.
pub fn adopt_legacy_env(binary: &str) {
    let adoption =
        plan_adoption(std::env::vars_os().filter_map(|(name, value)| {
            Some((name.into_string().ok()?, value.into_string().ok()?))
        }));
    for (name, value) in &adoption.set {
        std::env::set_var(name, value);
    }
    if let Some(line) = adoption_notice(binary, &adoption) {
        eprintln!("{line}");
    }
}

/// The stderr line [`adopt_legacy_env`] prints, or `None` when nothing was
/// adopted or shadowed.
pub fn adoption_notice(binary: &str, adoption: &Adoption) -> Option<String> {
    if adoption.set.is_empty() && adoption.shadowed.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !adoption.set.is_empty() {
        let names = adoption
            .set
            .iter()
            .map(|(name, _)| legacy_twin(name).unwrap_or_else(|| name.clone()))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("read as {PREFIX}*: {names}"));
    }
    if !adoption.shadowed.is_empty() {
        parts.push(format!(
            "ignored (the {PREFIX}* name is set): {}",
            adoption.shadowed.join(", ")
        ));
    }
    Some(format!(
        "{binary}: legacy {LEGACY_PREFIX}* environment variables {}. Rename them to {PREFIX}*.",
        parts.join("; ")
    ))
}

/// Add the legacy spelling of every `BEEKEEPER_<X>` in `env` that lacks one,
/// so an older child still finds it. A no-op when
/// [`EMIT_LEGACY_ENV_NAMES`] is off.
///
/// An explicit legacy entry already in the map is kept: the parent chose it.
pub fn with_legacy_mirrors<K, V>(env: impl IntoIterator<Item = (K, V)>) -> Vec<(String, String)>
where
    K: Into<String>,
    V: Into<String>,
{
    let mut out: Vec<(String, String)> = env
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
    if !EMIT_LEGACY_ENV_NAMES {
        return out;
    }
    let present: std::collections::BTreeSet<String> =
        out.iter().map(|(name, _)| name.clone()).collect();
    let mirrors: Vec<(String, String)> = out
        .iter()
        .filter_map(|(name, value)| {
            let twin = legacy_twin(name)?;
            (!present.contains(&twin)).then(|| (twin, value.clone()))
        })
        .collect();
    out.extend(mirrors);
    out
}

/// [`with_legacy_mirrors`] for an ordered map, returning the same type.
pub fn mirror_map(env: BTreeMap<String, String>) -> BTreeMap<String, String> {
    with_legacy_mirrors(env).into_iter().collect()
}

/// Rewrite the legacy keys of a stored env map to their current spelling.
///
/// For maps read back from disk or the wire: agent records, definitions,
/// personas, a Kubernetes `policy_env`. When both spellings are present the
/// current one wins and the legacy entry is dropped.
pub fn normalize_env_keys(map: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut legacy = Vec::new();
    for (name, value) in map {
        match canonical_name(&name) {
            Some(canonical) => legacy.push((canonical, value)),
            None => {
                out.insert(name, value);
            }
        }
    }
    for (canonical, value) in legacy {
        out.entry(canonical).or_insert(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn names_map_both_ways_and_only_for_the_two_prefixes() {
        assert_eq!(
            legacy_twin("BEEKEEPER_RELAY_URL").as_deref(),
            Some("BUZZ_RELAY_URL")
        );
        assert_eq!(
            canonical_name("BUZZ_RELAY_URL").as_deref(),
            Some("BEEKEEPER_RELAY_URL")
        );
        assert_eq!(legacy_twin("PATH"), None);
        assert_eq!(canonical_name("BEEKEEPER_RELAY_URL"), None);
        assert_eq!(
            canonical_name("BUZZ_"),
            None,
            "a bare prefix is not a variable"
        );
        assert_eq!(
            both_spellings("BEEKEEPER_AUTH_TAG"),
            vec!["BEEKEEPER_AUTH_TAG", "BUZZ_AUTH_TAG"]
        );
        assert_eq!(
            both_spellings("BUZZ_AUTH_TAG"),
            vec!["BUZZ_AUTH_TAG", "BEEKEEPER_AUTH_TAG"]
        );
        assert_eq!(
            both_spellings("NOSTR_PRIVATE_KEY"),
            vec!["NOSTR_PRIVATE_KEY"]
        );
    }

    #[test]
    fn a_legacy_variable_is_adopted_only_when_the_new_name_is_unset() {
        let adoption = plan_adoption(vars(&[
            ("BUZZ_RELAY_URL", "wss://old"),
            ("BUZZ_PRIVATE_KEY", "stale"),
            ("BEEKEEPER_PRIVATE_KEY", "current"),
            ("PATH", "/bin"),
        ]));
        assert_eq!(
            adoption.set,
            vec![("BEEKEEPER_RELAY_URL".to_owned(), "wss://old".to_owned())]
        );
        assert_eq!(adoption.shadowed, vec!["BUZZ_PRIVATE_KEY".to_owned()]);
        let notice = adoption_notice("beekeeper-relay", &adoption).expect("disclosed");
        assert!(notice.contains("BUZZ_RELAY_URL"), "{notice}");
        assert!(notice.contains("ignored"), "{notice}");
        assert!(
            !notice.contains("wss://old") && !notice.contains("stale"),
            "never values: {notice}"
        );
    }

    #[test]
    fn a_current_environment_adopts_nothing_and_says_nothing() {
        let adoption = plan_adoption(vars(&[("BEEKEEPER_RELAY_URL", "wss://x"), ("HOME", "/h")]));
        assert_eq!(adoption, Adoption::default());
        assert_eq!(adoption_notice("bee", &adoption), None);
    }

    #[test]
    fn mirrors_add_the_legacy_spelling_without_overriding_an_explicit_one() {
        let env = with_legacy_mirrors(vars(&[
            ("BEEKEEPER_RELAY_URL", "wss://x"),
            ("BEEKEEPER_AUTH_TAG", "tag"),
            ("BUZZ_AUTH_TAG", "explicit"),
            ("PATH", "/bin"),
        ]));
        let get = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        assert_eq!(get("BUZZ_RELAY_URL"), Some("wss://x"));
        assert_eq!(get("BUZZ_AUTH_TAG"), Some("explicit"));
        assert_eq!(get("BUZZ_PATH"), None);
        assert_eq!(env.iter().filter(|(n, _)| n == "BUZZ_AUTH_TAG").count(), 1);
    }

    #[test]
    fn stored_maps_normalize_with_the_current_spelling_winning() {
        let map: BTreeMap<String, String> = vars(&[
            ("BUZZ_AGENT_MODEL", "old-model"),
            ("BUZZ_AGENT_PROVIDER", "legacy-only"),
            ("BEEKEEPER_AGENT_MODEL", "new-model"),
            ("CUSTOM", "kept"),
        ])
        .into_iter()
        .collect();
        let out = normalize_env_keys(map);
        assert_eq!(
            out.get("BEEKEEPER_AGENT_MODEL").map(String::as_str),
            Some("new-model")
        );
        assert_eq!(
            out.get("BEEKEEPER_AGENT_PROVIDER").map(String::as_str),
            Some("legacy-only")
        );
        assert_eq!(out.get("CUSTOM").map(String::as_str), Some("kept"));
        assert!(
            out.keys().all(|key| !key.starts_with(LEGACY_PREFIX)),
            "{out:?}"
        );
    }
}
