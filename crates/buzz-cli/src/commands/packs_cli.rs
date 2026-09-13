//! Argument parsing and dispatch for project pack sources.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{client::BuzzClient, error::CliError};

/// Where a project's persona packs live — the kind:30624 record and what it
/// means on this machine.
///
/// Packs are trees of text, so they live in a git repository and the wire
/// carries the pointer. The relay checks publication authority and refuses
/// unauthorized source changes explicitly.
#[derive(Subcommand)]
pub enum PacksCmd {
    /// Publish the kind:30624 saying which repository holds this project's packs
    #[command(
        after_help = "Examples:\n  bee packs set-source --project 30621:<owner-hex>:agiterra --repo 30617:<owner-hex>:agiterra-packs --ref refs/heads/main\n  bee packs set-source --project 30621:<owner-hex>:agiterra --repo 30617:<owner-hex>:agiterra-packs --sha <40-hex> --path packs/roles\n\nExactly one of --ref and --sha: two pins would let two hosts stage two different trees from one signed record.\nUse --expected-source <event-id> to replace an observed source, or --if-unset to require no live source. With neither flag, publication remains unconditional (v1). A condition conflict exits 5 and is not retried."
    )]
    SetSource {
        /// Project coordinate `30621:<owner-hex>:<slug>`
        #[arg(long)]
        project: String,
        /// Packs repository coordinate `30617:<owner-hex>:<id>`
        #[arg(long)]
        repo: String,
        /// Stage the tip of this ref, e.g. `refs/heads/main`
        #[arg(long = "ref", conflicts_with = "sha")]
        ref_name: Option<String>,
        /// Stage exactly this commit (40 hex)
        #[arg(long)]
        sha: Option<String>,
        /// Directory holding one directory per role (default: personas/roles)
        #[arg(long)]
        path: Option<String>,
        /// An operator note, at most 512 bytes
        #[arg(long)]
        note: Option<String>,
        /// Replace only this exact source event (64 lowercase hex)
        #[arg(long, conflicts_with = "if_unset", value_parser = parse_expected_source)]
        expected_source: Option<String>,
        /// Publish only when the project has no live source
        #[arg(long)]
        if_unset: bool,
    },
    /// Announce a packs repository for a project, seed it, and point the
    /// project at the commit that landed
    #[command(
        after_help = "Three steps in one, the same three the desktop app's \"Create packs repository\" performs:\n  1. announce 30617 `<slug>-packs` under your key, inside the project\n  2. seed it from the role packs on disk with one signed commit, pushed to refs/heads/main\n  3. publish the 30624 pinned to the sha that actually landed\n\nA failure at any step stops the sequence and prints what already landed — nothing ever points at a repository with no packs in it. Requires the git credential helper (`just install-git-credentials`)."
    )]
    Init {
        /// Project coordinate `30621:<owner-hex>:<slug>`
        #[arg(long)]
        project: String,
        /// Repository id to announce (default: `<project-slug>-packs`)
        #[arg(long)]
        repo_id: Option<String>,
        /// Role packs to seed from (default: the nearest personas/roles)
        #[arg(long)]
        from: Option<PathBuf>,
        /// Directory inside the repository to write them to (default: personas/roles)
        #[arg(long)]
        path: Option<String>,
        /// Print the plan and touch nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Read a project's newest pack source, exactly as the relay served it
    GetSource {
        /// Project coordinate `30621:<owner-hex>:<slug>`
        #[arg(long)]
        project: String,
    },
    /// What this machine would stage: repository, commit, and role directories found
    #[command(
        after_help = "Reads the wire, then this disk. `cache_present: null` means this machine has never fetched these packs — a different fact from a role the repository does not carry."
    )]
    Status {
        /// Project coordinate `30621:<owner-hex>:<slug>`
        #[arg(long)]
        project: String,
        /// The seat role whose pack to report. The seat's role picks the
        /// pack; an actor's home role is never consulted.
        #[arg(long)]
        role: Option<String>,
        /// Override the packs cache directory this machine reads
        #[arg(long)]
        packs_dir: Option<PathBuf>,
    },
}

pub(crate) async fn dispatch(sub: PacksCmd, client: &BuzzClient) -> Result<(), CliError> {
    match sub {
        PacksCmd::SetSource {
            project,
            repo,
            ref_name,
            sha,
            path,
            note,
            expected_source,
            if_unset,
        } => {
            if expected_source.is_none() && !if_unset {
                return super::packs::cmd_set_source(
                    client,
                    &project,
                    &repo,
                    &super::packs::PackSourcePin {
                        ref_name: ref_name.as_deref(),
                        sha: sha.as_deref(),
                    },
                    path.as_deref(),
                    note.as_deref(),
                )
                .await;
            }
            super::packs::cmd_set_source_conditionally(
                client,
                &project,
                &repo,
                &super::packs::PackSourcePin {
                    ref_name: ref_name.as_deref(),
                    sha: sha.as_deref(),
                },
                path.as_deref(),
                note.as_deref(),
                match expected_source.as_deref() {
                    Some(id) => super::packs::PackSourceCondition::Expected(id),
                    None if if_unset => super::packs::PackSourceCondition::IfUnset,
                    None => super::packs::PackSourceCondition::Unconditional,
                },
            )
            .await
        }
        PacksCmd::Init {
            project,
            repo_id,
            from,
            path,
            dry_run,
        } => {
            super::packs::cmd_init(
                client,
                &super::packs::PackInitRequest {
                    project: &project,
                    repo_id: repo_id.as_deref(),
                    from: from.as_deref(),
                    path: path.as_deref(),
                    dry_run,
                },
            )
            .await
        }
        PacksCmd::GetSource { project } => super::packs::cmd_get_source(client, &project).await,
        PacksCmd::Status {
            project,
            role,
            packs_dir,
        } => {
            super::packs::cmd_status(client, &project, role.as_deref(), packs_dir.as_deref()).await
        }
    }
}

fn parse_expected_source(value: &str) -> Result<String, String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value.to_owned())
    } else {
        Err("--expected-source must be exactly 64 lowercase hex characters".into())
    }
}

#[cfg(test)]
#[path = "packs_cli_tests.rs"]
mod tests;
