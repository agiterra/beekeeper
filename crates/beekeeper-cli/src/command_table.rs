//! The `bee` command tree as data, for documents that must not drift from it.
//!
//! `buzz-acp`'s base prompt carries a table of every `bee` group and its
//! subcommands. A hand-written copy of that table told every seat that
//! `bee sessions` had six subcommands when it had forty-one; this function is
//! what the prompt's drift test renders the expected table from.

use clap::CommandFactory;

/// Every visible top-level `bee` group paired with its visible subcommand
/// names, both in declaration order (the order `bee --help` prints).
///
/// Hidden commands and clap's generated `help` subcommand are omitted. A group
/// with no subcommands of its own is returned with an empty list.
pub fn command_groups() -> Vec<(String, Vec<String>)> {
    crate::Cli::command()
        .get_subcommands()
        .filter(|group| !group.is_hide_set())
        .map(|group| {
            let subcommands = group
                .get_subcommands()
                .filter(|sub| !sub.is_hide_set())
                .map(|sub| sub.get_name().to_owned())
                .collect();
            (group.get_name().to_owned(), subcommands)
        })
        .collect()
}
