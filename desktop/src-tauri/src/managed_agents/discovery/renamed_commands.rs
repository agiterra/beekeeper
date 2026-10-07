//! Commands named before the `buzz-*` → `beekeeper-*` binary rename.

use std::path::PathBuf;

use beekeeper_host_core::command_paths::command_looks_like_path;

/// The current name of a bundled binary that a record may still name by its
/// pre-rename `buzz-<x>` spelling: `beekeeper-<x>`. Only bare names qualify —
/// an explicit path is the user's own choice and is never rewritten.
pub(crate) fn renamed_bundled_command(command: &str) -> Option<String> {
    let command = command.trim();
    if command_looks_like_path(command) {
        return None;
    }
    command
        .strip_prefix("buzz-")
        .filter(|rest| !rest.is_empty())
        .map(|rest| format!("beekeeper-{rest}"))
}

/// Resolve `command`; when it is a bare `buzz-<x>` that cannot be found, try
/// `beekeeper-<x>`. The record migration rewrites stored names, so this is
/// the belt for a record it has not reached (a restored backup, a team file,
/// a value typed by hand).
pub(super) fn resolve_with_renamed_fallback(
    command: &str,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    // The current name first: a stale pre-rename build (an old
    // `target/debug/buzz-acp`, or `~/.local/bin/buzz-*`) must not quietly win
    // over the binary this build ships. The old name is the fallback, for an
    // install that still only has it.
    match renamed_bundled_command(command) {
        Some(renamed) => resolve(&renamed).or_else(|| resolve(command)),
        None => resolve(command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bare_buzz_names_have_a_renamed_spelling() {
        assert_eq!(
            renamed_bundled_command("buzz-acp").as_deref(),
            Some("beekeeper-acp")
        );
        assert_eq!(
            renamed_bundled_command("buzz-dev-mcp").as_deref(),
            Some("beekeeper-dev-mcp")
        );
        assert_eq!(
            renamed_bundled_command("buzz-agent").as_deref(),
            Some("beekeeper-agent")
        );
        assert_eq!(renamed_bundled_command("beekeeper-acp"), None);
        assert_eq!(renamed_bundled_command("goose"), None);
        assert_eq!(renamed_bundled_command("buzz-"), None);
        assert_eq!(renamed_bundled_command("/opt/old/buzz-acp"), None);
    }

    #[test]
    fn a_missing_old_name_resolves_to_the_new_binary() {
        let installed = |command: &str| {
            (command == "beekeeper-acp").then(|| PathBuf::from("/app/beekeeper-acp"))
        };
        assert_eq!(
            resolve_with_renamed_fallback("buzz-acp", installed),
            Some(PathBuf::from("/app/beekeeper-acp"))
        );
        assert_eq!(
            resolve_with_renamed_fallback("beekeeper-acp", installed),
            Some(PathBuf::from("/app/beekeeper-acp"))
        );
        assert_eq!(
            resolve_with_renamed_fallback("buzz-shell-host", installed),
            None
        );
    }

    #[test]
    fn the_current_binary_wins_over_a_stale_old_one() {
        let both = |command: &str| Some(PathBuf::from(format!("/bin/{command}")));
        assert_eq!(
            resolve_with_renamed_fallback("buzz-acp", both),
            Some(PathBuf::from("/bin/beekeeper-acp"))
        );
    }

    #[test]
    fn an_install_with_only_the_old_binary_still_resolves() {
        let only_old = |command: &str| {
            (command == "buzz-acp").then(|| PathBuf::from(format!("/bin/{command}")))
        };
        assert_eq!(
            resolve_with_renamed_fallback("buzz-acp", only_old),
            Some(PathBuf::from("/bin/buzz-acp"))
        );
    }

    #[test]
    fn the_buzz_agent_runtime_answers_to_its_id_its_binary_and_its_old_binary_name() {
        use super::super::{
            known_acp_runtime, managed_agent_avatar_url, normalize_agent_args,
            BEEKEEPER_AGENT_AVATAR_URL,
        };
        for command in [
            "buzz-agent",
            "beekeeper-agent",
            "/Applications/Beekeeper.app/Contents/MacOS/beekeeper-agent",
            "/opt/old/buzz-agent",
        ] {
            let runtime = known_acp_runtime(command).expect(command);
            assert_eq!(runtime.id, "buzz-agent", "{command}");
            assert_eq!(runtime.commands.first().copied(), Some("beekeeper-agent"));
            assert_eq!(
                managed_agent_avatar_url(command),
                Some(BEEKEEPER_AGENT_AVATAR_URL.to_string()),
                "{command}"
            );
            assert_eq!(
                normalize_agent_args(command, vec!["acp".into()]),
                Vec::<String>::new(),
                "{command}"
            );
        }
    }
}
