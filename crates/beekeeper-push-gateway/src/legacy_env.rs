//! Read the pre-rename `BUZZ_*` environment variables as `BEEKEEPER_*`.
//!
//! A deliberately small local copy of the adoption loop in
//! `beekeeper_core::env_compat::adopt_legacy_env`: this crate does not
//! otherwise depend on `beekeeper-core`, and one loop is not worth the
//! dependency. Keep the behaviour identical to that function — the new name
//! always wins, the legacy variable is never removed, and only names (never
//! values) are printed.

use std::collections::BTreeMap;

/// Decide which `(BEEKEEPER_<X>, value)` pairs to set from `vars`, and which
/// legacy names are shadowed by an already-set new name.
fn plan<I>(vars: I) -> (Vec<(String, String)>, Vec<String>)
where
    I: IntoIterator<Item = (String, String)>,
{
    let vars: BTreeMap<String, String> = vars.into_iter().collect();
    let mut set = Vec::new();
    let mut shadowed = Vec::new();
    for (name, value) in &vars {
        let Some(rest) = name.strip_prefix("BUZZ_").filter(|rest| !rest.is_empty()) else {
            continue;
        };
        let canonical = format!("BEEKEEPER_{rest}");
        if let Some(current) = vars.get(&canonical) {
            // The same value under both names is a dual-writing parent, not
            // a conflict: nothing to adopt, nothing to report.
            if current != value {
                shadowed.push(name.clone());
            }
        } else {
            set.push((canonical, value.clone()));
        }
    }
    (set, shadowed)
}

/// Copy every `BUZZ_<X>` to an unset `BEEKEEPER_<X>` and say so on stderr.
///
/// Call it first in the synchronous `main`, before argument parsing, a tokio
/// runtime or any other thread: `std::env::set_var` is only sound while the
/// process is single-threaded.
pub fn adopt_legacy_env(binary: &str) {
    let (set, shadowed) =
        plan(std::env::vars_os().filter_map(|(name, value)| {
            Some((name.into_string().ok()?, value.into_string().ok()?))
        }));
    for (name, value) in &set {
        std::env::set_var(name, value);
    }
    let mut parts = Vec::new();
    if !set.is_empty() {
        let names = set
            .iter()
            .map(|(name, _)| name.replacen("BEEKEEPER_", "BUZZ_", 1))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("read as BEEKEEPER_*: {names}"));
    }
    if !shadowed.is_empty() {
        parts.push(format!(
            "ignored (the BEEKEEPER_* name is set): {}",
            shadowed.join(", ")
        ));
    }
    if !parts.is_empty() {
        eprintln!(
            "{binary}: legacy BUZZ_* environment variables {}. Rename them to BEEKEEPER_*.",
            parts.join("; ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::plan;

    #[test]
    fn a_legacy_variable_is_adopted_only_when_the_new_name_is_unset() {
        let vars = [
            ("BUZZ_RELAY_URL", "wss://old"),
            ("BUZZ_PRIVATE_KEY", "stale"),
            ("BEEKEEPER_PRIVATE_KEY", "current"),
            ("BUZZ_", "bare prefix"),
            ("BUZZ_AUTH_TAG", "same"),
            ("BEEKEEPER_AUTH_TAG", "same"),
            ("PATH", "/bin"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned()));
        let (set, shadowed) = plan(vars);
        assert_eq!(
            set,
            vec![("BEEKEEPER_RELAY_URL".to_owned(), "wss://old".to_owned())]
        );
        assert_eq!(shadowed, vec!["BUZZ_PRIVATE_KEY".to_owned()]);
    }
}
