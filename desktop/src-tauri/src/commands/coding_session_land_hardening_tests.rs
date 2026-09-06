//! Land-boundary tests for the **2026-09-05 admission audit** — findings 89
//! and 91, and the receipt an admission carries.
//!
//! Split out of `coding_session_land_tests.rs` for the reason that file was
//! already split once: four cases took it past the repository's 1,000-line
//! ceiling. The seam is the subject, as it is for the founders sibling —
//! everything here is about *what the rule consulted*: which repositories the
//! mission is bound to (finding 91), how its kind 44245 policy resolved
//! (finding 89), and whether the answer says which of the two it read or
//! assumed.
//!
//! This surface matters on its own because it is the one caller whose
//! discovery is **not** narrowed: the relay searches only the repository's own
//! channels, so its binding check is belt-and-braces, while this screen asks
//! about the one mission in front of it and the rule's own check is the only
//! guard there is.
//!
//! A child module of the tests module, so it shares that file's fixtures
//! (`protect`, `mission`, `observed_gates_request`, `repository_of`, …)
//! through `use super::*` rather than growing a second copy of them.

use super::*;

/// **Finding 91, where the rule is the only guard.** The relay narrows its
/// search to the repository's own channels; this surface does not — it asks
/// about the one mission on screen. So the binding check inside the rule is
/// what stops a mission that works on another repository from admitting a
/// push here, and the refusal names both the mission and the coordinate.
#[test]
fn a_mission_bound_to_another_repository_does_not_admit_a_push_here() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.bound_repositories = Some(vec![format!(
        "30617:{}:tankloop",
        mission.founder.public_key().to_hex()
    )]);

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        !answer.admitted,
        "a shared founder is not a binding (finding 91)"
    );
    let refusal = answer.refusal_reason.unwrap_or_default();
    assert!(refusal.contains(SESSION), "{refusal}");
    assert!(refusal.contains(&repository_of(&mission)), "{refusal}");
    assert!(
        answer.bound_repositories_read,
        "this caller sent the binding, and the answer must say so"
    );

    // The control: the same request bound to the repository on screen does
    // admit, so the refusal above is about the binding and nothing else.
    let mut bound = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    bound.bound_repositories = Some(vec![repository_of(&mission)]);
    assert!(
        land_adapter(bound).expect("the boundary answers").admitted,
        "the same mission bound to this repository admits it"
    );
}

/// A caller that read no binding is answered as it always was — and the answer
/// discloses that this surface assumed one, rather than implying it checked.
#[test]
fn an_unread_binding_is_assumed_and_the_answer_says_so() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.bound_repositories = None;

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        answer.admitted,
        "the pre-finding-91 caller still gets an answer"
    );
    assert!(
        !answer.bound_repositories_read,
        "and it says the binding was assumed, not read"
    );
}

/// **Finding 89 on this surface.** A caller that read a policy record it could
/// not decode says `unreadable`, and the rule refuses rather than falling back
/// to the defaults — the one direction the old `Option` could not express.
#[test]
fn an_unreadable_policy_refuses_here_too() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.gate_policy = Some(
        crate::commands::coding_session_land::CodingSessionLandGatePolicy {
            verifier_required: Some(false),
            required_gates: None,
            event_id: Some(POLICY_EVENT_ID.to_owned()),
            resolution: Some("unreadable".to_owned()),
            unreadable_reason: Some("gates.verifierRequired is not a boolean".to_owned()),
        },
    );

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        !answer.admitted,
        "nothing admits under a policy nobody read"
    );
    let refusal = answer.refusal_reason.unwrap_or_default();
    assert!(refusal.contains(POLICY_EVENT_ID), "{refusal}");

    // A resolution word this build does not know is unreadable too — never
    // silently `present`.
    let mut unknown = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    unknown.gate_policy = Some(
        crate::commands::coding_session_land::CodingSessionLandGatePolicy {
            verifier_required: Some(false),
            required_gates: None,
            event_id: Some(POLICY_EVENT_ID.to_owned()),
            resolution: Some("provisional".to_owned()),
            unreadable_reason: None,
        },
    );
    assert!(
        !land_adapter(unknown)
            .expect("the boundary answers")
            .admitted,
        "a word nobody recognises must not become a policy"
    );
}

/// An admission says which policy it stood on, and a founder's says that none
/// was consulted — the audit's Q1 receipt, on the screen a person reads.
#[test]
fn an_admission_names_the_policy_it_stood_on() {
    let mission = mission("changes-requested", Some(HEAD_SHA), None);
    let observed = land_adapter(observed_gates_request(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    let evidence = observed.evidence.expect("arm (B) names its evidence");
    assert_eq!(evidence.arm, "observed-gates");
    assert_eq!(evidence.policy_resolution, "present");
    assert_eq!(evidence.policy_event_id.as_deref(), Some(POLICY_EVENT_ID));
    assert_eq!(evidence.policy_not_evaluated, None);

    let founder = land_adapter(founder_request(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    let evidence = founder.evidence.expect("arm (A) names its evidence");
    assert_eq!(evidence.arm, "founder");
    assert_eq!(
        evidence.policy_not_evaluated.as_deref(),
        Some("founder_exception"),
        "a founder's landing must never read as verifier-approved"
    );
    assert!(evidence.policy_resolution.is_empty());
    assert_eq!(evidence.policy_event_id, None);
}
