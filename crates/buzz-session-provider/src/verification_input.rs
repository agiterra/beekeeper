//! Whether a seat's checkout actually holds the commit its assignment names.
//!
//! **Ledger item 133.** A verdict is a claim about a specific tree. A hired
//! seat's worktree is cut from trunk when the seat is hired, and the wake that
//! starts a verifier or a runner on an assignment is minted by the lead's CLI
//! (`crates/buzz-cli/src/commands/sessions/crew_cmds.rs:265`,
//! `send_team_operation_wake`, a 44220 carrying a two-key pointer). Between
//! that wake and the seat's first token nothing compared the seat's `HEAD`
//! with the assignment's `baseSha`:
//!
//! - the relay does not validate 44244 content at ingest, so a commit-less or
//!   wrong-commit verifier assignment is storable by any publisher;
//! - [`crate::team_wake::turn_requires_report`] resolves the same assignment,
//!   but at *turn end* and only to decide whether the lead is owed a wake;
//! - the seat itself could be asked, and asking the subject is exactly the
//!   claim finding 26 caught being wrong.
//!
//! The provider is the only party that holds both the seat's working directory
//! and the decision to open the turn, so it is the only fence that covers every
//! wake path — a CLI check would cover one producer.
//!
//! What is fenced is deliberately narrow: a turn whose text is an assignment
//! pointer, for a seat whose role [`role_verifies_a_commit`]. READY, ordinary
//! prose, builder and lead assignments open exactly as before.

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamFoldContext,
    CodingSessionTeamTransactionBody,
};
use nostr::Event;

/// The assignment named no commit at all: `baseSha` was absent.
pub const VERIFICATION_INPUT_UNNAMED: &str = "VERIFICATION_INPUT_UNNAMED";

/// The seat's `HEAD` is not the commit the assignment named.
pub const VERIFICATION_INPUT_NOT_PRESENT: &str = "VERIFICATION_INPUT_NOT_PRESENT";

/// The seat's working tree carries changes, so `HEAD` alone does not describe
/// what would be verified.
pub const VERIFICATION_INPUT_TREE_DIRTY: &str = "VERIFICATION_INPUT_TREE_DIRTY";

/// The provider could not read the seat's checkout at all.
pub const VERIFICATION_INPUT_UNOBSERVED: &str = "VERIFICATION_INPUT_UNOBSERVED";

/// The seat's tree was not put on the assignment's commit: the establishment
/// this provider ran refused, or was abandoned after two interrupted attempts.
/// Carries git's own words.
pub const VERIFICATION_INPUT_NOT_ESTABLISHED: &str = "VERIFICATION_INPUT_NOT_ESTABLISHED";

/// The assignment itself could not be verified right now — the fold, the relay
/// query or the scope was unavailable. Refused rather than opened on a guess.
pub const VERIFICATION_INPUT_UNVERIFIED: &str = "VERIFICATION_INPUT_UNVERIFIED";

/// Roles whose turn is a claim about a particular commit.
///
/// A verifier refutes a report about a tree and a runner executes acceptance
/// commands against one; both publish a result that names a commit, and both
/// are wrong in a way nobody can see if the tree was not that commit. A
/// builder's assignment describes work to *do*, so it is not fenced: it starts
/// wherever the lead cut the seat.
pub fn role_verifies_a_commit(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "verifier" | "runner"
    )
}

/// The operation id of a turn whose text is the two-key assignment pointer.
///
/// The same strict shape [`crate::team_wake::turn_requires_report`] accepts:
/// exactly `{"type":"assignment","operationId":"<64 hex>"}`. Anything looser
/// would let ordinary prose that happens to be JSON claim an operation
/// identity, and anything stricter would miss the pointer both producers mint.
pub fn assignment_pointer(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let object = value.as_object()?;
    if object.len() != 2 || object.get("type")?.as_str()? != "assignment" {
        return None;
    }
    let operation_id = object.get("operationId")?.as_str()?;
    (operation_id.len() == 64 && operation_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| operation_id.to_ascii_lowercase())
}

/// What the canonical assignment behind a pointer asks this turn to verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputRequirement {
    /// The turn is not an assignment turn for a commit-bound role.
    NotRequired,
    /// The assignment is canonical and bound to this actor, role and command.
    Required {
        assignment_ref: String,
        /// The commit the assignment names, absent when it named none.
        base_sha: Option<String>,
    },
    /// The assignment could not be verified at this moment.
    Unknown(&'static str),
}

/// Resolve the assignment a turn's pointer names, from signed facts only.
///
/// Deliberately the same sequence as
/// [`crate::team_wake::turn_requires_report`] — canonical fold membership,
/// envelope validation, then the actor/role/command binding — minus the inbox
/// lookup. At turn end the initiating command is already stored and query
/// visible; at turn *open* it arrived over the socket moments ago and often is
/// not, so requiring it would refuse every fast turn.
///
/// Once the pointer is present, every failure is [`InputRequirement::Unknown`]
/// rather than "not required": the caller refuses on Unknown, so a delayed
/// relay or an unfoldable graph cannot quietly turn the fence off.
pub fn turn_input_requirement(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
    command_id: &str,
    actor: &str,
    role: &str,
    operation_id: &str,
) -> InputRequirement {
    if !role_verifies_a_commit(role) {
        return InputRequirement::NotRequired;
    }
    let Ok(fold) = fold_coding_session_team_transactions(events, context) else {
        return InputRequirement::Unknown("assignment_fold_unavailable");
    };
    if !fold.included_event_ids.iter().any(|id| id == operation_id) {
        return InputRequirement::Unknown("assignment_not_canonical");
    }
    let Some(event) = events
        .iter()
        .find(|event| event.id.to_hex() == operation_id)
    else {
        return InputRequirement::Unknown("assignment_not_query_visible");
    };
    let Ok(payload) = buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(event) else {
        return InputRequirement::Unknown("assignment_invalid");
    };
    let CodingSessionTeamTransactionBody::Assignment(assignment) = payload.body else {
        return InputRequirement::Unknown("assignment_type_mismatch");
    };
    if payload.delivery_command_id.as_deref() != Some(command_id)
        || assignment.assignee_actor != actor
        || assignment.assignee_role != role
    {
        return InputRequirement::Unknown("assignment_binding_mismatch");
    }
    InputRequirement::Required {
        assignment_ref: operation_id.to_owned(),
        base_sha: assignment.base_sha,
    }
}

/// What the provider saw in the seat's working directory.
///
/// Both fields are `None` when the observation failed, never "observed to be
/// absent" — the same discipline [`crate::git_probe::GitProbe`] keeps, and the
/// reason an unobservable checkout refuses instead of passing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeatTree {
    /// `HEAD` as a lowercase object id, 40 or 64 hex.
    pub head: Option<String>,
    /// Lines of `git status --porcelain`, so a refusal can say how much.
    pub dirty_lines: Option<usize>,
}

/// A refused turn: the code a consumer branches on and the sentence a person
/// reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRefusal {
    pub code: &'static str,
    pub message: String,
}

/// The remedy every refusal here names.
///
/// A host step and a new assignment — never "ask a person", because a seat
/// cannot act on that and a lead reading the receipt needs to know which of
/// the two facts it controls was wrong.
///
/// It says *re-issue* rather than "wake the seat again" because a refusal is
/// recorded durably before it is published, so the same `commandId` is
/// answered for good and a redelivery is silently ignored
/// ([`crate::state::StateStore::record_refusal`]). An assignment's wake shares the
/// assignment's own `deliveryCommandId`, so re-waking *this* assignment would
/// reuse the answered id and do nothing at all. A new assignment mints a new
/// delivery.
const REMEDY: &str = "establish that commit in this seat's checkout and re-issue the assignment \
     (this delivery is answered, so re-sending the same wake does nothing)";

/// What the fence decided about one turn.
///
/// Three outcomes, not two, and the third is the point: a turn refused
/// durably consumes its `commandId` for good, which is the right answer for a
/// fact — the assignment names no commit, the tree is at another one — and the
/// wrong answer for "the relay did not answer just now". Wakes already queue
/// minutes behind a busy seat, so a transport failure that consumed the
/// assignment would destroy real work under a polite message. Unknown is not
/// false (the rule `worktree_prune` states, and ledger 133).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnInput {
    /// Not fenced, or fenced and established: open the turn.
    Open,
    /// A fact the lead or the host can act on. Durable.
    Refused(InputRefusal),
    /// Not knowable at this moment. Nothing is published and nothing is
    /// consumed; the same `commandId` is still deliverable, and the provider
    /// leaves the channel's watermark alone so a later pass re-reads the
    /// command. Bounded by the command's own freshness horizon, after which
    /// `decide_turn` ignores it as `PastHorizon`.
    Undecided(&'static str),
    /// This provider owes the seat's tree an establishment and has not
    /// finished it yet.
    ///
    /// Deliberately its own outcome rather than a refusal: the wake is not
    /// wrong, it is *early*, and the work that will make it right is already
    /// recorded durably in this host's `assignmentInputs` row. Like
    /// [`TurnInput::Undecided`] it publishes nothing and consumes nothing, so
    /// the same `commandId` is still deliverable and the delivery is not spent
    /// on a tree that is about to move. What is different is the reason, and
    /// that the answer is owed by this computer rather than by a relay.
    Deferred(&'static str),
}

/// Reasons that describe the *transport* rather than the assignment.
///
/// Each one can become answerable without anybody doing anything, so none of
/// them may consume the command. `assignment_not_canonical`,
/// `assignment_invalid`, `assignment_type_mismatch` and
/// `assignment_binding_mismatch` are deliberately **not** here: the partition
/// query is complete-or-error, so when it succeeds the fold saw the whole
/// graph and its exclusion is a fact about the pointer, not a slow read.
pub fn reason_is_transport(reason: &str) -> bool {
    matches!(
        reason,
        "relay_query_unavailable"
            | "relay_identity_unavailable"
            | "verified_snapshot_unavailable"
            | "assignment_fold_unavailable"
            | "assignment_not_query_visible"
    )
}

/// Turn one `Unknown` reason into an outcome: undecided when the transport is
/// what failed, a durable refusal when the pointer is what is wrong.
pub fn unresolved(reason: &'static str) -> TurnInput {
    if reason_is_transport(reason) {
        return TurnInput::Undecided(reason);
    }
    TurnInput::Refused(InputRefusal {
        code: VERIFICATION_INPUT_UNVERIFIED,
        message: format!(
            "this assignment could not be verified ({reason}); the turn was not opened, so \
             nothing was verified against an unknown input. To continue, {REMEDY}"
        ),
    })
}

/// Refuse a turn whose seat's input this provider tried and failed to
/// establish, in git's own words.
///
/// The one bounded blocker a failed establishment publishes: the lead needs
/// the sentence git produced — a dirty tree, an object no remote has — because
/// every remedy for those is a thing a person or a host step does, and a
/// paraphrase would send them looking for the wrong thing. Bounded and
/// control-free, because it travels in signed content.
pub fn establishment_blocked(
    assignment_ref: &str,
    outcome: &str,
    detail: Option<&str>,
) -> InputRefusal {
    let words = detail
        .map(bounded_git_words)
        .filter(|words| !words.is_empty())
        .map_or_else(
            || "this computer recorded no reason".to_owned(),
            |words| format!("git said: {words}"),
        );
    InputRefusal {
        code: VERIFICATION_INPUT_NOT_ESTABLISHED,
        message: format!(
            "this computer tried to put the commit assignment {assignment_ref} names into the \
             seat's checkout and could not ({outcome}); {words}. The turn was not opened and \
             nothing was verified. To continue, {REMEDY}"
        ),
    }
}

/// Keep one diagnostic useful without letting git's output grow a record or a
/// published message.
#[must_use]
pub fn bounded_git_words(detail: &str) -> String {
    const MAX_BYTES: usize = 512;
    let cleaned = detail
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut end = cleaned.len().min(MAX_BYTES);
    while !cleaned.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    cleaned[..end].to_owned()
}

/// Refuse a turn whose seat has no working directory to read.
///
/// A fact about the host, and one it can fix, so it is durable like the others
/// — and deliberately wordless about *which* directory: this message is
/// published in signed content, which never carries a host path.
pub fn checkout_missing(assignment_ref: &str) -> InputRefusal {
    InputRefusal {
        code: VERIFICATION_INPUT_UNOBSERVED,
        message: format!(
            "this seat has no readable working directory, so whether it holds the commit \
             assignment {assignment_ref} names cannot be established; the turn was not opened. \
             Restore or relocate the seat's checkout, then {REMEDY}"
        ),
    }
}

/// Admit the turn, or refuse it naming the fact that is missing.
///
/// Returns the object id the turn would be verifying against, so the caller
/// can record it. Accepts a 40- or 64-hex id on either side and compares them
/// case-insensitively; it never matches a prefix, because "the commit I was
/// asked about" and "a commit whose id starts the same way" are different
/// claims.
pub fn check_seat_tree(
    assignment_ref: &str,
    base_sha: Option<&str>,
    tree: &SeatTree,
) -> Result<String, InputRefusal> {
    let Some(base_sha) = base_sha.map(str::trim).filter(|sha| !sha.is_empty()) else {
        return Err(InputRefusal {
            code: VERIFICATION_INPUT_UNNAMED,
            message: format!(
                "no verification input named: assignment {assignment_ref} carries no baseSha, so \
                 there is no commit this turn could be checked against. Re-issue the assignment \
                 naming the commit to verify"
            ),
        });
    };
    let base_sha = base_sha.to_ascii_lowercase();
    let Some(head) = tree.head.as_deref() else {
        return Err(InputRefusal {
            code: VERIFICATION_INPUT_UNOBSERVED,
            message: format!(
                "the seat's checkout could not be read, so whether it holds {base_sha} is unknown; \
                 the turn was not opened. Check the seat's working directory exists and is a \
                 repository, then {REMEDY}"
            ),
        });
    };
    let head = head.to_ascii_lowercase();
    if head != base_sha {
        return Err(InputRefusal {
            code: VERIFICATION_INPUT_NOT_PRESENT,
            message: format!(
                "the seat's HEAD is {head}, and assignment {assignment_ref} names {base_sha}; \
                 a verdict from this tree would be about the wrong commit. To continue, {REMEDY}"
            ),
        });
    }
    let Some(dirty_lines) = tree.dirty_lines else {
        return Err(InputRefusal {
            code: VERIFICATION_INPUT_UNOBSERVED,
            message: format!(
                "the seat's HEAD is {head} as assignment {assignment_ref} asks, but whether the \
                 tree is clean could not be read, so what would be verified is unknown; the turn \
                 was not opened. Check the seat's working directory, then {REMEDY}"
            ),
        });
    };
    if dirty_lines > 0 {
        return Err(InputRefusal {
            code: VERIFICATION_INPUT_TREE_DIRTY,
            message: format!(
                "the seat's tree carries {dirty_lines} uncommitted \
                 {}, so it is not {base_sha} even though HEAD names it; a verdict would be about \
                 an unpublished tree. Commit or clear the changes, then re-issue the assignment \
                 (this delivery is answered, so re-sending the same wake does nothing)",
                if dirty_lines == 1 {
                    "change"
                } else {
                    "changes"
                }
            ),
        });
    }
    Ok(base_sha)
}

#[cfg(test)]
mod tests {
    use buzz_core::coding_session_team_transaction::{
        CodingSessionTeamActiveSeat, CodingSessionTeamAssignment,
    };
    use nostr::Keys;
    use uuid::Uuid;

    use super::*;

    /// The commit the fixture signs with before removing the key, so the
    /// removal can assert it took out exactly what it put in.
    const PLACEHOLDER_BASE_SHA: &str = "ef00ef00ef00ef00ef00ef00ef00ef00ef00ef00";

    struct Fixture {
        events: Vec<Event>,
        context: CodingSessionTeamFoldContext,
        actor: String,
        assignment_ref: String,
    }

    /// One canonical assignment for `role`, bound to the delivery command
    /// `assignment-turn`, carrying `base_sha`.
    ///
    /// `None` is signed the long way round on purpose. Ledger 131 made
    /// `baseSha` **required at sign time** for a verifier or runner, so the
    /// SDK builder now refuses to mint one without it — and that is the right
    /// refusal. What this fence still has to handle is the assignment already
    /// on the relay, signed before that rule existed: the read paths tolerate
    /// its absence, so the fixture builds a conforming event and re-signs it
    /// with the key removed rather than pretending the builder would produce
    /// one.
    fn fixture(role: &str, base_sha: Option<&str>) -> Fixture {
        let founder = Keys::generate();
        let actor = Keys::generate().public_key().to_hex();
        let channel_ref = Uuid::new_v4();
        let session_ref = Uuid::new_v4().to_string();
        let genesis_ref = "aa".repeat(32);
        let body = CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: actor.clone(),
            assignee_role: role.to_owned(),
            objective: "Refute the report".into(),
            brief: "Check the claim against the tree it names.".into(),
            branch: None,
            // Replaced below when the caller asked for none.
            base_sha: Some(base_sha.unwrap_or(PLACEHOLDER_BASE_SHA).to_owned()),
            file_ownership: Vec::new(),
            acceptance_steps: vec!["cargo test -p buzz-session-provider".into()],
        });
        let payload =
            buzz_sdk::coding_session_team_transaction::coding_session_team_transaction_payload(
                session_ref.clone(),
                genesis_ref.clone(),
                None,
                Some("assignment-turn".into()),
                body,
            );
        let assignment =
            buzz_sdk::coding_session_team_transaction::build_coding_session_team_transaction(
                &channel_ref.to_string(),
                payload,
            )
            .expect("builder")
            .sign_with_keys(&founder)
            .expect("sign");
        let assignment = match base_sha {
            Some(_) => assignment,
            None => {
                let mut content: serde_json::Value =
                    serde_json::from_str(&assignment.content).expect("assignment content is json");
                // Null, not absent. Every optional field in these payloads
                // keeps its key — the decoder denies unknown fields and
                // supplies no default — so "an assignment that names no
                // commit" is `"baseSha": null` on the wire, which is what a
                // record signed before ledger 131 actually carries.
                let previous = content
                    .pointer_mut("/body")
                    .and_then(serde_json::Value::as_object_mut)
                    .map(|body| body.insert("baseSha".to_owned(), serde_json::Value::Null));
                assert_eq!(
                    previous,
                    Some(Some(serde_json::Value::String(
                        PLACEHOLDER_BASE_SHA.to_owned()
                    ))),
                    "the fixture must blank the key it put there, not some other one"
                );
                nostr::EventBuilder::new(assignment.kind, content.to_string())
                    .tags(assignment.tags.to_vec())
                    .sign_with_keys(&founder)
                    .expect("sign an assignment from before baseSha was required")
            }
        };
        let context = CodingSessionTeamFoldContext {
            channel_ref: channel_ref.to_string(),
            session_ref,
            genesis_ref,
            founder_pubkey: founder.public_key().to_hex(),
            active_seats: vec![CodingSessionTeamActiveSeat {
                actor_pubkey: actor.clone(),
                role: role.to_owned(),
            }],
            active_grants: Vec::new(),
            verifier_required: false,
        };
        Fixture {
            assignment_ref: assignment.id.to_hex(),
            events: vec![assignment],
            context,
            actor,
        }
    }

    #[test]
    fn only_a_commit_bound_role_is_fenced() {
        assert!(role_verifies_a_commit("verifier"));
        assert!(role_verifies_a_commit("runner"));
        assert!(role_verifies_a_commit(" Verifier "));
        for open in ["builder", "lead", "designer", "architect", ""] {
            assert!(!role_verifies_a_commit(open), "{open} must not be fenced");
        }
    }

    #[test]
    fn only_the_exact_two_key_pointer_is_an_assignment_turn() {
        let id = "ab".repeat(32);
        assert_eq!(
            assignment_pointer(&format!(
                "{{\"operationId\":\"{id}\",\"type\":\"assignment\"}}"
            )),
            Some(id.clone())
        );
        assert_eq!(
            assignment_pointer(&format!(
                "  {{\"type\":\"assignment\",\"operationId\":\"{}\"}}  ",
                id.to_ascii_uppercase()
            )),
            Some(id.clone()),
            "key order and case are not identity"
        );
        for not_a_pointer in [
            "Reply READY when initialized.".to_owned(),
            format!("{{\"operationId\":\"{id}\",\"type\":\"report\"}}"),
            format!("{{\"operationId\":\"{id}\",\"type\":\"assignment\",\"extra\":1}}"),
            "{\"operationId\":\"short\",\"type\":\"assignment\"}".to_owned(),
            format!("[{{\"operationId\":\"{id}\",\"type\":\"assignment\"}}]"),
        ] {
            assert_eq!(
                assignment_pointer(&not_a_pointer),
                None,
                "must not read {not_a_pointer} as a pointer"
            );
        }
    }

    #[test]
    fn a_canonical_assignment_carries_its_named_commit_through() {
        let base = "cd".repeat(20);
        let fixture = fixture("verifier", Some(&base));
        assert_eq!(
            turn_input_requirement(
                &fixture.events,
                &fixture.context,
                "assignment-turn",
                &fixture.actor,
                "verifier",
                &fixture.assignment_ref,
            ),
            InputRequirement::Required {
                assignment_ref: fixture.assignment_ref.clone(),
                base_sha: Some(base),
            }
        );
    }

    /// The absence a slice-2 assignment can no longer sign, and an older one
    /// already on the relay still carries: read as the `Option` it is, and
    /// refused later by [`check_seat_tree`] rather than treated as "fine".
    #[test]
    fn an_assignment_without_a_commit_still_resolves_and_is_refused_downstream() {
        let fixture = fixture("verifier", None);
        let requirement = turn_input_requirement(
            &fixture.events,
            &fixture.context,
            "assignment-turn",
            &fixture.actor,
            "verifier",
            &fixture.assignment_ref,
        );
        assert_eq!(
            requirement,
            InputRequirement::Required {
                assignment_ref: fixture.assignment_ref.clone(),
                base_sha: None,
            }
        );
        assert_eq!(
            check_seat_tree(
                &fixture.assignment_ref,
                None,
                &SeatTree {
                    head: Some("ab".repeat(20)),
                    dirty_lines: Some(0),
                },
            )
            .expect_err("must refuse")
            .code,
            VERIFICATION_INPUT_UNNAMED
        );
    }

    #[test]
    fn a_builder_seat_is_not_required_to_establish_anything() {
        let fixture = fixture("builder", None);
        assert_eq!(
            turn_input_requirement(
                &fixture.events,
                &fixture.context,
                "assignment-turn",
                &fixture.actor,
                "builder",
                &fixture.assignment_ref,
            ),
            InputRequirement::NotRequired
        );
    }

    /// Each way of not knowing is its own reason, and every one of them is
    /// `Unknown` rather than `NotRequired`: once a pointer is present, the
    /// caller refuses, so a missing fact can never quietly open the turn.
    #[test]
    fn every_unresolvable_assignment_is_unknown() {
        let fixture = fixture("verifier", Some(&"cd".repeat(20)));
        let cases = [
            (
                turn_input_requirement(
                    &[],
                    &fixture.context,
                    "assignment-turn",
                    &fixture.actor,
                    "verifier",
                    &fixture.assignment_ref,
                ),
                "assignment_not_canonical",
            ),
            (
                turn_input_requirement(
                    &fixture.events,
                    &fixture.context,
                    "another-command",
                    &fixture.actor,
                    "verifier",
                    &fixture.assignment_ref,
                ),
                "assignment_binding_mismatch",
            ),
            (
                turn_input_requirement(
                    &fixture.events,
                    &fixture.context,
                    "assignment-turn",
                    &"ff".repeat(32),
                    "verifier",
                    &fixture.assignment_ref,
                ),
                "assignment_binding_mismatch",
            ),
            (
                turn_input_requirement(
                    &fixture.events,
                    &fixture.context,
                    "assignment-turn",
                    &fixture.actor,
                    "runner",
                    &fixture.assignment_ref,
                ),
                "assignment_binding_mismatch",
            ),
        ];
        for (requirement, reason) in cases {
            assert_eq!(requirement, InputRequirement::Unknown(reason));
        }
    }

    /// Which reasons may consume the command and which may not — the split
    /// that keeps a slow relay from spending an assignment (ledger 133).
    #[test]
    fn the_transport_never_consumes_an_assignment() {
        for transport in [
            "relay_query_unavailable",
            "relay_identity_unavailable",
            "verified_snapshot_unavailable",
            "assignment_fold_unavailable",
            "assignment_not_query_visible",
        ] {
            assert!(reason_is_transport(transport));
            assert_eq!(unresolved(transport), TurnInput::Undecided(transport));
        }
        for pointer_fact in [
            "assignment_not_canonical",
            "assignment_invalid",
            "assignment_type_mismatch",
            "assignment_binding_mismatch",
            "assignment_scope_unresolved",
        ] {
            assert!(!reason_is_transport(pointer_fact));
            let TurnInput::Refused(refusal) = unresolved(pointer_fact) else {
                panic!("{pointer_fact} is a fact about the pointer and must refuse");
            };
            assert_eq!(refusal.code, VERIFICATION_INPUT_UNVERIFIED);
            assert!(refusal.message.contains(pointer_fact));
        }
    }

    /// The codes are the contract a desktop surface will render, so they are
    /// pinned here by value rather than only by name.
    #[test]
    fn the_refusal_codes_are_stable() {
        assert_eq!(VERIFICATION_INPUT_UNNAMED, "VERIFICATION_INPUT_UNNAMED");
        assert_eq!(
            VERIFICATION_INPUT_NOT_PRESENT,
            "VERIFICATION_INPUT_NOT_PRESENT"
        );
        assert_eq!(
            VERIFICATION_INPUT_TREE_DIRTY,
            "VERIFICATION_INPUT_TREE_DIRTY"
        );
        assert_eq!(
            VERIFICATION_INPUT_UNOBSERVED,
            "VERIFICATION_INPUT_UNOBSERVED"
        );
        assert_eq!(
            VERIFICATION_INPUT_UNVERIFIED,
            "VERIFICATION_INPUT_UNVERIFIED"
        );
    }
}
