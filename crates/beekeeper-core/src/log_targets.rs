//! Reading a log filter written before the crates were renamed.
//!
//! Tracing targets follow crate names, so when the `buzz-*` crates became
//! `beekeeper-*` (ledger 354) every deployed `RUST_LOG=buzz_relay=info` stopped
//! matching anything: an `EnvFilter` directive for a target that no longer
//! exists is silently inert, and the relay went quiet rather than failing.
//! The filter lives in operators' `.env` files, unit files and shells, none of
//! which a deploy rewrites on its own, so every binary reads the old names as
//! the new ones and **says so** at startup, naming what to change.

/// The renamed crates' library names, without the `buzz_` / `beekeeper_`
/// prefix, plus `datastore` (the custom `beekeeper_datastore` span target)
/// and `lib` (the desktop's `beekeeper_lib`).
const RENAMED: &[&str] = &[
    "acp",
    "admin",
    "agent",
    "audit",
    "auth",
    "backend_kubernetes",
    "cli",
    "conformance",
    "core",
    "datastore",
    "datastore_tracing",
    "db",
    "deletion",
    "dev_mcp",
    "lib",
    "media",
    "mirror_bridge",
    "pair_relay",
    "pairing_cli",
    "persona",
    "pubsub",
    "push_gateway",
    "relay",
    "relay_mesh",
    "sdk",
    "search",
    "session_provider",
    "shell_host",
    "terminal",
    "test_client",
    "voice",
    "workflow",
    "ws_client",
];

/// A filter string with its pre-rename targets rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedLogFilter {
    /// The filter to hand to `EnvFilter`.
    pub filter: String,
    /// Each legacy target that was rewritten, as `(old, new)`, in order of
    /// first appearance. Empty when the filter needed nothing.
    pub renamed: Vec<(String, String)>,
}

impl MigratedLogFilter {
    /// The one-line warning a binary prints when it rewrote anything, naming
    /// the variable so the operator can fix the source. `None` when nothing
    /// was rewritten.
    pub fn warning(&self, variable: &str) -> Option<String> {
        if self.renamed.is_empty() {
            return None;
        }
        let pairs = self
            .renamed
            .iter()
            .map(|(old, new)| format!("{old} -> {new}"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "{variable} names pre-rename log targets; reading them as the new ones \
             ({pairs}). Update {variable} where it is set."
        ))
    }
}

/// Rewrite every `buzz_<crate>` target in a tracing filter to `beekeeper_<crate>`.
///
/// Only the target of each comma-separated directive is touched: the part
/// before `=` or `[`, matched as a whole crate name or a `crate::module`
/// path. Levels, span and field filters, and targets of other crates pass
/// through byte for byte, so a filter that needs nothing comes back unchanged.
pub fn migrate_legacy_log_targets(filter: &str) -> MigratedLogFilter {
    let mut renamed: Vec<(String, String)> = Vec::new();
    let directives = filter
        .split(',')
        .map(|directive| {
            let lead = directive.len() - directive.trim_start().len();
            let (indent, body) = directive.split_at(lead);
            let target_end = body.find(['=', '[']).unwrap_or(body.len());
            let (target, rest) = body.split_at(target_end);
            match legacy_target(target.trim_end()) {
                Some(new_target) => {
                    let old = target.trim_end().to_owned();
                    if !renamed.iter().any(|(seen, _)| *seen == old) {
                        renamed.push((old, new_target.clone()));
                    }
                    let trailing = &target[target.trim_end().len()..];
                    format!("{indent}{new_target}{trailing}{rest}")
                }
                None => directive.to_owned(),
            }
        })
        .collect::<Vec<_>>();
    MigratedLogFilter {
        filter: directives.join(","),
        renamed,
    }
}

/// Read a filter variable (`RUST_LOG`, `BUZZ_OTEL_FILTER`) with its
/// pre-rename targets rewritten.
///
/// Prints the [`MigratedLogFilter::warning`] on stderr, prefixed with
/// `binary`, because every caller runs before its tracing subscriber exists.
/// `None` when the variable is unset or empty, so each caller keeps its own
/// default.
pub fn read_filter_var(variable: &str, binary: &str) -> Option<String> {
    let value = std::env::var(variable).ok()?;
    if value.trim().is_empty() {
        return None;
    }
    let migrated = migrate_legacy_log_targets(&value);
    if let Some(warning) = migrated.warning(variable) {
        eprintln!("{binary}: {warning}");
    }
    Some(migrated.filter)
}

/// `Some(beekeeper_<crate>[::path])` when `target` is a renamed crate's old name.
fn legacy_target(target: &str) -> Option<String> {
    let rest = target.strip_prefix("buzz_")?;
    let (krate, path) = match rest.find("::") {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    RENAMED
        .contains(&krate)
        .then(|| format!("beekeeper_{krate}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deployed_relay_filter_reads_as_the_new_targets() {
        let migrated = migrate_legacy_log_targets(
            "buzz_relay=info,buzz_db=info,buzz_auth=info,buzz_pubsub=info,tower_http=info",
        );
        assert_eq!(
            migrated.filter,
            "beekeeper_relay=info,beekeeper_db=info,beekeeper_auth=info,beekeeper_pubsub=info,tower_http=info"
        );
        assert_eq!(migrated.renamed.len(), 4);
        let warning = migrated
            .warning("RUST_LOG")
            .expect("a rewrite is disclosed");
        assert!(
            warning.contains("buzz_relay -> beekeeper_relay"),
            "{warning}"
        );
        assert!(warning.contains("Update RUST_LOG"), "{warning}");
    }

    #[test]
    fn a_current_or_foreign_filter_is_returned_byte_for_byte() {
        for filter in [
            "",
            "info",
            "beekeeper_relay=debug,tower_http=info",
            "warn,hyper=off",
            // Not a renamed crate: a target that merely starts with `buzz_`.
            "buzz_something_else=debug",
        ] {
            let migrated = migrate_legacy_log_targets(filter);
            assert_eq!(migrated.filter, filter);
            assert!(migrated.renamed.is_empty());
            assert_eq!(migrated.warning("RUST_LOG"), None);
        }
    }

    #[test]
    fn module_paths_spans_and_spacing_survive() {
        let migrated = migrate_legacy_log_targets(
            "info, buzz_relay::api=trace,buzz_db[query{id=1}]=debug,buzz_datastore",
        );
        assert_eq!(
            migrated.filter,
            "info, beekeeper_relay::api=trace,beekeeper_db[query{id=1}]=debug,beekeeper_datastore"
        );
        assert_eq!(
            migrated.renamed,
            vec![
                (
                    "buzz_relay::api".to_owned(),
                    "beekeeper_relay::api".to_owned()
                ),
                ("buzz_db".to_owned(), "beekeeper_db".to_owned()),
                (
                    "buzz_datastore".to_owned(),
                    "beekeeper_datastore".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_target_named_twice_is_reported_once() {
        let migrated = migrate_legacy_log_targets("buzz_acp=info,buzz_acp=debug");
        assert_eq!(migrated.filter, "beekeeper_acp=info,beekeeper_acp=debug");
        assert_eq!(migrated.renamed.len(), 1);
    }
}
