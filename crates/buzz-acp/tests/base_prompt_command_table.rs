//! The base prompt's `bee` command table must equal the CLI's clap tree.
//!
//! A hand-maintained copy told every seat `bee sessions` offered six
//! subcommands when the CLI had forty-one, so a lead spent its opening tool
//! calls rediscovering `work`, `hire`, `verdict` and the rest. On failure the
//! message carries the regenerated table: paste it over the old one.

const TABLE_HEADER: &str = "| Group | Key commands |\n|-------|-------------|\n";

/// Suffix kept on the `bee sessions` row, pointing at the prompt's own section.
const SESSIONS_POINTER: &str = " (coding sessions — see below)";

fn render_expected_table() -> String {
    let mut table = String::from(TABLE_HEADER);
    for (group, subcommands) in buzz_cli::command_table::command_groups() {
        let listed = subcommands
            .iter()
            .map(|sub| format!("`{sub}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let pointer = if group == "sessions" {
            SESSIONS_POINTER
        } else {
            ""
        };
        table.push_str(&format!("| `bee {group}` | {listed}{pointer} |\n"));
    }
    table
}

fn table_in_prompt(prompt: &str) -> &str {
    let start = prompt
        .find(TABLE_HEADER)
        .expect("base_prompt.md carries the `| Group | Key commands |` table");
    let len = prompt[start..]
        .split_inclusive('\n')
        .take_while(|line| line.starts_with('|'))
        .map(str::len)
        .sum::<usize>();
    &prompt[start..start + len]
}

#[test]
fn base_prompt_command_table_matches_the_cli() {
    let expected = render_expected_table();
    let actual = table_in_prompt(buzz_acp::BASE_PROMPT);
    assert!(
        actual == expected,
        "crates/buzz-acp/src/base_prompt.md's `bee` command table has drifted from \
         the CLI. Replace it with:\n\n{expected}"
    );
}
