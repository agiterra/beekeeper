//! Thin `buzz-admin project-purge` adapter.
//!
//! Operator-only and CLI-only: the deployment admin HTTP router is read-only
//! by contract, so this destructive path is deliberately reachable only from
//! a shell on the relay host.

pub use beekeeper_deletion::project::Command as ProjectPurgeCommand;

/// Delegate to the shared project-scoped purge implementation.
pub async fn run(command: ProjectPurgeCommand) -> anyhow::Result<i32> {
    beekeeper_deletion::project::run(command).await
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn run_requires_the_explicit_confirmation_flag() {
        // `--confirm` is a flag, so absence must parse but be refused at
        // runtime; presence must parse.
        let without = crate::Cli::try_parse_from([
            "buzz-admin",
            "project-purge",
            "run",
            "--coordinate",
            "30621:aa:proj",
            "--approved-digest",
            "deadbeef",
            "--purged-by",
            "operator",
        ]);
        assert!(without.is_ok());
        let with = crate::Cli::try_parse_from([
            "buzz-admin",
            "project-purge",
            "run",
            "--coordinate",
            "30621:aa:proj",
            "--approved-digest",
            "deadbeef",
            "--purged-by",
            "operator",
            "--confirm",
        ]);
        assert!(with.is_ok());
    }

    #[test]
    fn run_requires_a_digest_and_an_operator_identity() {
        for missing in [
            vec![
                "buzz-admin",
                "project-purge",
                "run",
                "--coordinate",
                "30621:aa:proj",
                "--purged-by",
                "operator",
                "--confirm",
            ],
            vec![
                "buzz-admin",
                "project-purge",
                "run",
                "--coordinate",
                "30621:aa:proj",
                "--approved-digest",
                "deadbeef",
                "--confirm",
            ],
            vec![
                "buzz-admin",
                "project-purge",
                "run",
                "--approved-digest",
                "deadbeef",
                "--purged-by",
                "operator",
                "--confirm",
            ],
        ] {
            assert!(
                crate::Cli::try_parse_from(missing.clone()).is_err(),
                "expected {missing:?} to be rejected"
            );
        }
    }

    /// The live-message acknowledgement is reachable from the real binary's
    /// parser, and is a flag distinct from `--confirm`.
    #[test]
    fn the_live_message_acknowledgement_is_wired_into_the_binary() {
        let mut base = vec![
            "buzz-admin",
            "project-purge",
            "run",
            "--coordinate",
            "30621:aa:proj",
            "--approved-digest",
            "deadbeef",
            "--purged-by",
            "operator",
            "--confirm",
        ];
        assert!(crate::Cli::try_parse_from(base.clone()).is_ok());
        base.push("--acknowledge-live-messages");
        assert!(crate::Cli::try_parse_from(base).is_ok());
        // Not a valid flag on the read-only command.
        assert!(crate::Cli::try_parse_from([
            "buzz-admin",
            "project-purge",
            "inventory",
            "--coordinate",
            "30621:aa:proj",
            "--acknowledge-live-messages",
        ])
        .is_err());
    }

    #[test]
    fn no_bulk_or_all_projects_form_is_exposed() {
        for command in [
            vec!["buzz-admin", "project-purge", "run", "--all"],
            vec!["buzz-admin", "project-purge", "drain"],
            vec!["buzz-admin", "project-purge", "sweep"],
        ] {
            assert!(crate::Cli::try_parse_from(command).is_err());
        }
    }
}
