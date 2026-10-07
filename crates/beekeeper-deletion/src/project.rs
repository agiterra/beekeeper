//! Operator-only, CLI-only project-scoped purge.
//!
//! Reclaims the PostgreSQL storage a **soft-deleted** project still occupies,
//! reusing the whole-community engine's shape — inventory → digest-bound
//! approval → destructive transaction — without a second staged control
//! plane. The reasoning for that choice, and the exact definition of "already
//! soft-deleted", live with the store adapter in
//! [`beekeeper_db::project_purge`].
//!
//! There is deliberately **no HTTP surface**. The deployment admin router is
//! read-only by contract (it asserts POST/PUT/PATCH/DELETE all return 405);
//! this operation is reachable only from a shell on the relay host.
//!
//! Two steps, so the operator sees what will be destroyed before destroying
//! it:
//!
//! ```text
//! beekeeper-admin project-purge inventory --coordinate 30621:<owner>:<slug>
//! beekeeper-admin project-purge run       --coordinate 30621:<owner>:<slug> \
//!     --approved-digest <digest from the inventory> \
//!     --purged-by <operator> --confirm
//! ```
//!
//! `--approved-digest` is the confirmation gate, and it is a stronger one
//! than a bare `--yes`: the scope is recomputed *inside* the purge
//! transaction and the delete refuses unless it still hashes to exactly the
//! digest the operator read. `--purged-by` mirrors `deletions approve
//! --approved-by`; `--confirm` is the explicit acknowledgement that this is
//! irreversible.
//!
//! `--acknowledge-live-messages` is a second, narrower gate for the one class
//! of row this purge hard-deletes without a tombstone of its own: messages
//! inside an already-tombstoned channel. It is required whenever `inventory`
//! reports a non-zero `live_events_in_tombstoned_channels`.

use anyhow::{Context, Result};
use beekeeper_db::project_purge::ProjectCoordinate;
use clap::Subcommand;

/// CLI-only project purge commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Freeze and print the purge inventory for one project coordinate.
    ///
    /// Read-only. Reports the rows that would be deleted, the rows
    /// deliberately retained and why, and any blocker that makes the project
    /// un-purgeable (anything still live).
    Inventory {
        /// Project coordinate `30621:<64-hex-owner>:<slug>`.
        #[arg(long)]
        coordinate: String,
        /// Canonical community host. Defaults to RELAY_URL's authority.
        #[arg(long)]
        host: Option<String>,
    },
    /// Hard-delete one soft-deleted project's rows in a single transaction.
    ///
    /// Refuses unless the scope recomputed inside that transaction still
    /// hashes to `--approved-digest` and contains nothing live.
    Run {
        /// Project coordinate `30621:<64-hex-owner>:<slug>`.
        #[arg(long)]
        coordinate: String,
        /// Canonical community host. Defaults to RELAY_URL's authority.
        #[arg(long)]
        host: Option<String>,
        /// The `digest` printed by `project-purge inventory`.
        #[arg(long)]
        approved_digest: String,
        /// Operator identity recorded on the purge receipt.
        #[arg(long)]
        purged_by: String,
        /// Optional operator note recorded on the receipt.
        #[arg(long)]
        note: Option<String>,
        /// Required acknowledgement that this hard-deletes rows irreversibly.
        #[arg(long)]
        confirm: bool,
        /// Acknowledge hard-deleting messages that carry no tombstone of their
        /// own because only their channel was tombstoned.
        ///
        /// Required whenever `inventory` reports a non-zero
        /// `live_events_in_tombstoned_channels`; without it the purge refuses
        /// rather than silently including them.
        #[arg(long)]
        acknowledge_live_messages: bool,
    },
}

/// Execute one nested project-purge command.
pub async fn run(command: Command) -> Result<i32> {
    match command {
        Command::Inventory { coordinate, host } => {
            let coordinate = ProjectCoordinate::parse(&coordinate)?;
            let (db, community) = connect_tenant(host.as_deref()).await?;
            let inventory = db
                .project_purge_store()
                .inventory(community, &coordinate)
                .await?;
            crate::print_json(&inventory)?;
            Ok(0)
        }
        Command::Run {
            coordinate,
            host,
            approved_digest,
            purged_by,
            note,
            confirm,
            acknowledge_live_messages,
        } => {
            if !confirm {
                anyhow::bail!(
                    "refusing to purge without --confirm; \
                     run `project-purge inventory` first and pass its digest"
                );
            }
            let coordinate = ProjectCoordinate::parse(&coordinate)?;
            let (db, community) = connect_tenant(host.as_deref()).await?;
            let receipt = db
                .project_purge_store()
                .purge(
                    community,
                    &coordinate,
                    &approved_digest,
                    &purged_by,
                    note.as_deref(),
                    acknowledge_live_messages,
                )
                .await?;
            crate::print_json(&receipt)?;
            Ok(0)
        }
    }
}

/// Resolve the single community this invocation is fenced to.
///
/// Same row-zero seam as `beekeeper-admin`'s other commands: the host is derived
/// from `RELAY_URL` (or `--host`) and looked up in the durable `communities`
/// map. An unmapped, archived, or already-deleted host fails closed — there
/// is no cross-community sweep and no default tenant.
async fn connect_tenant(
    host: Option<&str>,
) -> Result<(beekeeper_db::Db, beekeeper_core::CommunityId)> {
    let relay_url = std::env::var("RELAY_URL").ok();
    let host = crate::resolve_submit_host(host, relay_url.as_deref())?;
    let host = beekeeper_core::tenant::relay_url_authority(&format!("ws://{host}"));
    let db = crate::connect_db().await?;
    let record = db
        .lookup_community_by_host(&host)
        .await
        .context("look up community host")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "host '{host}' is not mapped to a live community; \
                 pass --host or set RELAY_URL to a mapped host"
            )
        })?;
    Ok((db, record.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// `Command` is a [`Subcommand`], so parsing it standalone needs a root.
    #[derive(Parser)]
    struct Harness {
        #[command(subcommand)]
        command: Command,
    }

    fn parse(args: &[&str]) -> Result<Command, clap::Error> {
        Harness::try_parse_from(std::iter::once("harness").chain(args.iter().copied()))
            .map(|harness| harness.command)
    }

    const COORDINATE: &str = "30621:\
        aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:throwaway";

    fn run_args() -> Vec<&'static str> {
        vec![
            "run",
            "--coordinate",
            COORDINATE,
            "--approved-digest",
            "deadbeef",
            "--purged-by",
            "operator",
            "--confirm",
        ]
    }

    /// Both destructive gates default off, so neither can be acquired by
    /// forgetting a flag.
    #[test]
    fn destructive_flags_default_off() {
        let mut args = run_args();
        args.retain(|arg| *arg != "--confirm");
        match parse(&args).expect("parses without either gate") {
            Command::Run {
                confirm,
                acknowledge_live_messages,
                ..
            } => {
                assert!(!confirm);
                assert!(!acknowledge_live_messages);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn acknowledge_live_messages_is_a_separate_opt_in_from_confirm() {
        match parse(&run_args()).expect("parses with --confirm only") {
            Command::Run {
                confirm,
                acknowledge_live_messages,
                ..
            } => {
                assert!(confirm, "--confirm must not imply anything else");
                assert!(
                    !acknowledge_live_messages,
                    "--confirm must never imply --acknowledge-live-messages"
                );
            }
            other => panic!("expected Run, got {other:?}"),
        }

        let mut args = run_args();
        args.push("--acknowledge-live-messages");
        match parse(&args).expect("parses with both gates") {
            Command::Run {
                acknowledge_live_messages,
                ..
            } => assert!(acknowledge_live_messages),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    /// The read-only command must not accept any destructive gate, so an
    /// operator cannot reach the purge through `inventory`.
    #[test]
    fn inventory_exposes_no_destructive_flag() {
        for flag in ["--confirm", "--acknowledge-live-messages", "--purged-by"] {
            let args = vec!["inventory", "--coordinate", COORDINATE, flag, "x"];
            assert!(parse(&args).is_err(), "inventory must reject {flag}");
        }
        assert!(parse(&["inventory", "--coordinate", COORDINATE]).is_ok());
    }

    /// `run` must refuse before it opens a database connection, so a missing
    /// `--confirm` can never reach the purge transaction.
    #[tokio::test]
    async fn run_refuses_without_confirm_and_before_connecting() {
        let mut args = run_args();
        args.retain(|arg| *arg != "--confirm");
        let command = parse(&args).expect("parses");
        let error = run(command)
            .await
            .expect_err("must refuse without --confirm");
        let message = error.to_string();
        assert!(
            message.contains("refusing to purge without --confirm"),
            "unexpected refusal: {message}"
        );
        // A connection attempt surfaces as a DATABASE_URL / connect error, so
        // the confirmation message proves nothing was opened.
        assert!(!message.contains("DATABASE_URL"), "{message}");
    }

    /// A malformed coordinate is rejected by the shared normalizer before any
    /// tenant lookup — the CLI and the relay cannot drift on what a coordinate
    /// is, and a typo never opens a connection.
    #[tokio::test]
    async fn a_malformed_coordinate_is_rejected_before_connecting() {
        for command in [
            Command::Inventory {
                coordinate: "30621:not-hex:proj".to_owned(),
                host: None,
            },
            Command::Run {
                coordinate: "40621:aa:proj".to_owned(),
                host: None,
                approved_digest: "deadbeef".to_owned(),
                purged_by: "operator".to_owned(),
                note: None,
                confirm: true,
                acknowledge_live_messages: true,
            },
        ] {
            let error = run(command).await.expect_err("malformed must be rejected");
            let message = error.to_string();
            assert!(
                message.contains("well-formed project coordinate"),
                "unexpected error: {message}"
            );
            assert!(!message.contains("DATABASE_URL"), "{message}");
        }
    }
}
