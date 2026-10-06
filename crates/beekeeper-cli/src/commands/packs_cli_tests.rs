use clap::Parser;

use crate::{Cli, Cmd, PacksCmd};

fn args() -> Vec<String> {
    [
        "bee",
        "packs",
        "set-source",
        "--project",
        "30621:OWNER:project",
        "--repo",
        "30617:OWNER:packs",
        "--ref",
        "refs/heads/main",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn source_conditions_parse_and_remain_opt_in() {
    for (flags, expected, unset) in [
        (vec![], None, false),
        (vec!["--if-unset".to_owned()], None, true),
        (
            vec!["--expected-source".to_owned(), "a".repeat(64)],
            Some("a".repeat(64)),
            false,
        ),
    ] {
        let mut argv = args();
        argv.extend(flags);
        let cli = Cli::try_parse_from(argv).expect("valid condition flags");
        let Cmd::Packs(PacksCmd::SetSource {
            expected_source,
            if_unset,
            ..
        }) = cli.command
        else {
            panic!("expected set-source");
        };
        assert_eq!(expected_source, expected);
        assert_eq!(if_unset, unset);
    }
}

#[test]
fn source_conditions_reject_two_conditions_and_noncanonical_ids() {
    for id in [
        "".to_owned(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        format!(" {}", "a".repeat(64)),
    ] {
        let mut argv = args();
        argv.extend(["--expected-source".to_owned(), id]);
        assert!(Cli::try_parse_from(argv).is_err());
    }
    for flags in [
        vec![
            "--expected-source".to_owned(),
            "a".repeat(64),
            "--if-unset".to_owned(),
        ],
        vec![
            "--if-unset".to_owned(),
            "--expected-source".to_owned(),
            "a".repeat(64),
        ],
    ] {
        let mut argv = args();
        argv.extend(flags);
        let error = Cli::try_parse_from(argv)
            .err()
            .expect("conflicting conditions");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
