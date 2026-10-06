//! Argument parsing and dispatch for project pack sources.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{client::BeekeeperClient, error::CliError};

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
    /// Announce the project's agents repository, seed it, and point the
    /// project at it
    #[command(
        after_help = "Three steps in one, the same three the desktop app performs when it creates a project:\n  1. announce 30617 `<slug>-beekeeper-agents` under your key, inside the project\n  2. seed it — by default the flat layout (`team.yml`, `roles/<role>.md` including this build's shipped role templates by reference, `plans/`, both `archive/`s) — with one signed commit pushed to refs/heads/main\n  3. publish the 30624: `ref: refs/heads/main, path: .` for the flat layout, or the sha that landed for --layout pack\n\nA failure at any step stops the sequence and prints what already landed — nothing ever points at a repository with no roles in it. Requires the git credential helper (`just install-git-credentials`)."
    )]
    Init {
        /// Project coordinate `30621:<owner-hex>:<slug>`
        #[arg(long)]
        project: String,
        /// Repository id to announce (default: `<project-slug>-beekeeper-agents`
        /// for --layout flat, `<project-slug>-packs` for --layout pack)
        #[arg(long)]
        repo_id: Option<String>,
        /// Seed from this directory instead of writing the seed: a team root
        /// for --layout flat, a directory of role packs for --layout pack
        /// (default for pack: the nearest personas/roles)
        #[arg(long)]
        from: Option<PathBuf>,
        /// Directory inside the repository to write them to (default:
        /// `.` for --layout flat, personas/roles for --layout pack)
        #[arg(long)]
        path: Option<String>,
        /// The layout to seed: `flat` (`roles/<role>.md`, `team.yml`,
        /// `plans/` — the agents repository) or `pack` (one pack directory
        /// per role, the shipped layout)
        #[arg(long, default_value = "flat")]
        layout: String,
        /// Path to the template catalog the flat seed references. Defaults
        /// to `$BUZZ_TEMPLATES_DIR`, then the templates this `bee`'s app
        /// bundle ships (or, for a development build, its own checkout's)
        #[arg(long)]
        templates: Option<PathBuf>,
        /// Replace the project's existing role source instead of refusing:
        /// the 64-hex kind:30624 event id you read. The new source is
        /// published conditionally on it, so a source someone re-pointed
        /// meanwhile is refused rather than overwritten
        #[arg(long)]
        expect_source: Option<String>,
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
        /// Template catalog to compose against (`<name>/<semver>/TEMPLATE.md`).
        /// Defaults to `$BUZZ_TEMPLATES_DIR`; without either, a role that
        /// includes a shipped template reports a compose refusal.
        #[arg(long, env = "BUZZ_TEMPLATES_DIR")]
        templates: Option<PathBuf>,
    },
}

pub(crate) async fn dispatch(sub: PacksCmd, client: &BeekeeperClient) -> Result<(), CliError> {
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
            layout,
            templates,
            expect_source,
            dry_run,
        } => {
            let layout = super::packs::PackLayout::parse(&layout)?;
            super::packs::cmd_init(
                client,
                &super::packs::PackInitRequest {
                    project: &project,
                    repo_id: repo_id.as_deref(),
                    from: from.as_deref(),
                    path: path.as_deref(),
                    layout,
                    templates: templates.as_deref(),
                    expect_source: expect_source.as_deref(),
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
            templates,
        } => {
            super::packs::cmd_status(
                client,
                &project,
                role.as_deref(),
                packs_dir.as_deref(),
                templates.as_deref(),
            )
            .await
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
