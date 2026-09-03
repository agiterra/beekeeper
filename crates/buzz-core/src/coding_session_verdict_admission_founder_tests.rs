//! Finding 33 — a repository has founders, not an owner.
//!
//! The landed gate keyed both authority questions to the kind:30617 signer:
//! which missions were searched (`founder == repo owner`) and who might land
//! the result (`pusher == that founder`). `agiterra-beekeeper` is signed by
//! Andy and co-owned by Brian, so under that rule Brian's missions ruled on
//! nothing and Brian's landing pushes were refused. These cases pin the set.

use super::*;

use crate::channel::ProjectRole;
use crate::coding_session_team_transaction::CodingSessionTeamDispositionDecision;
use crate::repository_founders::RepositoryFounders;

/// The repository's kind:30617, signed by `signer`, naming `maintainers`.
fn announcement(signer: &Keys, maintainers: &[String]) -> Event {
    let mut tags = vec![Tag::parse(["d", "agiterra-beekeeper"]).expect("d tag")];
    if !maintainers.is_empty() {
        let mut values = vec!["maintainers".to_string()];
        values.extend(maintainers.iter().cloned());
        tags.push(Tag::parse(values).expect("maintainers tag"));
    }
    EventBuilder::new(Kind::from(30617u16), "")
        .tags(tags)
        .sign_with_keys(signer)
        .expect("announcement signs")
}

/// An approved mission whose founder is `founder`, over `HEAD_SHA`.
fn approved_mission_founded_by(founder: Keys) -> Mission {
    let mut mission = mission(
        Some(HEAD_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    // `mission()` generates its own founder; re-sign the fixture's ruling under
    // the founder this case is about, exactly as `mission()` does.
    let assignment = signed(&assignment(&mission.builder), &mission.lead, 100);
    let report = signed(
        &report(&assignment.id.to_hex(), Some("whoami/cli"), Some(HEAD_SHA)),
        &mission.builder,
        200,
    );
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        300,
    );
    mission.events = vec![assignment, report, disposition];
    mission.founder = founder;
    mission
}

/// The live shape: Andy signs the announcement, Brian is in `maintainers`,
/// Brian founds the mission and Brian pushes. Before this lane the candidate
/// was skipped on the first line of the loop.
#[test]
fn a_maintainer_founded_mission_admits_the_maintainers_push() {
    let andy = Keys::generate();
    let brian = Keys::generate();
    let brian_hex = brian.public_key().to_hex();
    let mission = approved_mission_founded_by(brian);
    let founders = RepositoryFounders::from_announcement(&announcement(
        &andy,
        std::slice::from_ref(&brian_hex),
    ));
    assert_eq!(founders.len(), 2);

    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&brian_hex, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    let VerdictAdmission::Admitted(evidence) = outcome else {
        panic!("a founder's own ruling admits their own push: {outcome:?}");
    };
    assert_eq!(evidence.head_sha, HEAD_SHA);
}

/// Without the tag the same mission is refused exactly as it is today: the
/// rule is not weakened for a single-founder repository.
#[test]
fn without_the_maintainers_tag_the_same_mission_is_refused() {
    let andy = Keys::generate();
    let brian = Keys::generate();
    let brian_hex = brian.public_key().to_hex();
    let mission = approved_mission_founded_by(brian);
    let founders = RepositoryFounders::from_announcement(&announcement(&andy, &[]));
    assert_eq!(founders.len(), 1);

    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&brian_hex, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    let VerdictAdmission::Refused(VerdictAdmissionRefusal::NoApprovingVerdict {
        searched_sessions,
        ..
    }) = outcome
    else {
        panic!("a mission founded by a non-founder rules on nothing: {outcome:?}");
    };
    assert_eq!(searched_sessions, 1);
}

/// The reservation half: the ruling is the signer's, and a *maintainer* who
/// founded no mission still lands it. Before this lane `admit_or_reserve`
/// compared the pusher with the mission's own founder alone.
#[test]
fn a_maintainer_may_land_a_ruling_the_signer_made() {
    let andy = Keys::generate();
    let brian_hex = "3d".repeat(32);
    let mission = approved_mission_founded_by(andy.clone());
    let founders = RepositoryFounders::from_announcement(&announcement(
        &andy,
        std::slice::from_ref(&brian_hex),
    ));

    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&brian_hex, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    assert!(
        outcome.is_admitted(),
        "an equal owner lands the other's ruling: {outcome:?}"
    );

    // …and a key that founded nothing still cannot.
    let stranger = Keys::generate().public_key().to_hex();
    let refused = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&stranger, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    let VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedButPushReserved {
        founders: count,
        ..
    }) = refused
    else {
        panic!("a stranger's push of an approved commit is reserved: {refused:?}");
    };
    assert_eq!(
        count, 2,
        "the refusal discloses how many founders there are"
    );
}

/// The coordinator's case, and the live one: the second founder holds no
/// `maintainers` row at all — they are an Owner on the project roster the
/// announcement's `project` back-reference names (commit `a56ad5d01`). Their
/// mission's verdict admits their push.
#[test]
fn a_project_roster_owner_founds_a_mission_that_admits() {
    let andy = Keys::generate();
    let brian = Keys::generate();
    let brian_hex = brian.public_key().to_hex();
    let mission = approved_mission_founded_by(brian);
    let founders = RepositoryFounders::from_announcement(&announcement(&andy, &[]))
        .with_roster_roles(vec![
            (andy.public_key().to_hex(), ProjectRole::Owner),
            (brian_hex.clone(), ProjectRole::Owner),
            (
                Keys::generate().public_key().to_hex(),
                ProjectRole::Collaborator,
            ),
        ]);
    assert_eq!(founders.len(), 2);
    assert_eq!(founders.roster_owners_read(), Some(1));

    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&brian_hex, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    assert!(
        outcome.is_admitted(),
        "a project Owner founds the repository too: {outcome:?}"
    );
}

/// A collaborator is not a founder: the roster grants them Member, not Owner,
/// and the founder set must not quietly widen to everyone with push.
#[test]
fn a_project_collaborator_is_not_a_founder() {
    let andy = Keys::generate();
    let collaborator = Keys::generate();
    let collaborator_hex = collaborator.public_key().to_hex();
    let mission = approved_mission_founded_by(collaborator);
    let founders = RepositoryFounders::from_announcement(&announcement(&andy, &[]))
        .with_roster_roles(vec![(collaborator_hex.clone(), ProjectRole::Collaborator)]);
    assert_eq!(founders.pubkeys(), &[andy.public_key().to_hex()]);

    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&collaborator_hex, founders.pubkeys(), HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    assert!(!outcome.is_admitted(), "{outcome:?}");
}

/// An empty founder set admits nothing — the fail-closed shape a caller that
/// could resolve no announcement at all must land in.
#[test]
fn an_empty_founder_set_admits_nothing() {
    let founder = Keys::generate();
    let founder_hex = founder.public_key().to_hex();
    let mission = approved_mission_founded_by(founder);
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&founder_hex, &[], HEAD_SHA),
        &VerdictAdmissionRules::FOUNDER_ONLY,
    );
    assert!(!outcome.is_admitted(), "{outcome:?}");
}
