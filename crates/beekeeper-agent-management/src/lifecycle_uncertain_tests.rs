//! A rewound session's first create is not a live responsibility (ledger 401).
//! The C4 `builder` could not Start: its four rewound Claude sessions each
//! showed a stopped latest generation and a `disconnected` generation 1.
use super::*;
use beekeeper_core::coding_session_payload::SessionStatus;

fn row(session: &str, generation: u64, status: SessionStatus) -> Responsibility {
    Responsibility {
        channel_id: Uuid::nil(),
        target: CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "0c36bc37d6bd56c9".into(),
            session_id: session.into(),
            generation,
        },
        provider_authority: "a".repeat(64),
        metadata_event_id: format!("{session}-{generation}"),
        status,
    }
}

#[test]
fn an_observed_session_covers_its_rewound_first_create_and_an_unobserved_create_stays() {
    let mut rows = vec![row("rewound", 3, SessionStatus::Stopped)];
    merge_uncertain(
        &mut rows,
        &[
            row("rewound", 1, SessionStatus::Disconnected),
            row("never-observed", 1, SessionStatus::Disconnected),
        ],
    );
    let sessions: Vec<(&str, u64)> = rows
        .iter()
        .map(|r| (r.target.session_id.as_str(), r.target.generation))
        .collect();
    assert_eq!(sessions, vec![("rewound", 3), ("never-observed", 1)]);

    // Stops keep exact targets: the cumulative merge still adds generation 1.
    let mut stop = vec![row("rewound", 3, SessionStatus::Stopped)];
    merge_inventory(&mut stop, &[row("rewound", 1, SessionStatus::Disconnected)]);
    assert_eq!(stop.len(), 2);
}
