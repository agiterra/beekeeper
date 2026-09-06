//! Finding 56 — **where** a verdict-gated push looks for a mission.
//!
//! Split out of [`super`] only to keep that file under the repository's
//! 1,000-line ceiling; it is one enum, its two bounds, the sentence a
//! refusal renders from it, and (since L35) the prediction-grade seat
//! projection that reads the same kind 44228 page the seat lookup does.

use nostr::Event;

use super::{
    VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS, VERDICT_ADMISSION_MAX_SESSIONS,
    VERDICT_ADMISSION_MAX_TRANSACTIONS,
};
use crate::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use crate::coding_session_team_transaction::CodingSessionTeamActiveSeat;
use crate::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;

/// Newest missions one push may be judged by when the pusher holds seats.
///
/// **Finding 56.** A seat's push is judged by the mission that seated it, not
/// by whatever lives on the channel the repository happens to be bound to. A
/// key may hold several seats at once; this is how many of them one push
/// reads. The bound fails in the refusing direction — a fifth, older seat is
/// simply not searched, so the worst it can do is deny a push it might have
/// admitted.
pub const VERDICT_ADMISSION_MAX_PUSHER_SEATS: usize = 4;

/// Newest session channels of a project one push may search.
///
/// The fallback lookup for a push by a key that holds no seat at all. Same
/// refusing direction as every other bound here.
pub const VERDICT_ADMISSION_MAX_PROJECT_CHANNELS: usize = 32;

/// **Where** the caller found the missions it is offering this rule.
///
/// Live run 4 refused a seat's push of a commit its own mission had watched
/// three gates pass on, with "Searched 0 mission(s) — the newest 16 on this
/// channel". Every coding session lives in its own channel and the gate only
/// ever read the repository's bound channel, so the count was truthful and the
/// search was pointless. The rule cannot fix a lookup it does not perform;
/// what it can do is say which one ran, so a reader can tell *"your mission had
/// nothing to say"* from *"nobody looked at your mission"*.
///
/// This is a fact about the caller's I/O, so `buzz-core` never resolves it —
/// the relay, the CLI prediction and the desktop adapter each say which lookup
/// they performed, and the sentence a person reads is rendered here from that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictAdmissionCandidateSource {
    /// The missions whose accepted authority chain seats the pusher.
    SeatOfMission {
        /// The pusher's key as the refusal names it — 8 hex, which is what a
        /// person can hold in their head and enough to grep a channel for.
        seat: String,
        /// How many of the pusher's seats were resolved — bounded by
        /// [`VERDICT_ADMISSION_MAX_PUSHER_SEATS`].
        seats: usize,
    },
    /// The pusher's seats, **narrowed to the missions this repository's own
    /// channels hold** (finding 91).
    ///
    /// [`Self::SeatOfMission`] is the same lookup before that narrowing: it
    /// returned every mission the key is seated on anywhere in the community,
    /// so a seat on one repository's mission offered its green rows to a push
    /// of another repository the same founder owns. A caller that narrows
    /// says so with this variant, because the count it reports is then a
    /// count of *some* of the key's seats and a reader must not read it as
    /// all of them.
    SeatOfMissionInScope {
        /// The pusher's key as the refusal names it — 8 hex.
        seat: String,
        /// How many of the pusher's seats survived the narrowing.
        seats: usize,
        /// How many seats the lookup held before it narrowed.
        held: usize,
        /// What the seats were narrowed to, as a person reads it — e.g.
        /// `the 3 session channel(s) of 30621:<owner>:beekeeper`.
        within: String,
    },
    /// Every session channel of the project the announcement back-references.
    ProjectSessions {
        /// The project coordinate the repository's `project` tag names.
        project: String,
        /// How many session channels of it were searched — bounded by
        /// [`VERDICT_ADMISSION_MAX_PROJECT_CHANNELS`].
        channels: usize,
    },
    /// The one channel the repository's `buzz-channel` tag names — the lookup
    /// that was the *only* lookup until finding 56, kept as the last fallback
    /// for a repository in no project whose pusher holds no seat.
    BoundChannel,
    /// One named mission, supplied by a caller already looking at it: the
    /// desktop's Land control asks about the mission on screen and nothing
    /// else, and must not imply it swept anything wider.
    ThisMission {
        /// The umbrella it asked about.
        session_ref: String,
    },
}

/// The bound-channel lookup as a borrowable `'static` value.
///
/// [`VerdictAdmissionQuery`](super::VerdictAdmissionQuery) borrows its source
/// so it can stay `Copy`, and this variant carries nothing — a caller that
/// searched the repository's bound channel has no value of its own to keep
/// alive. Const promotion cannot supply the borrow (the enum owns `String`s in
/// its other variants), so the one shared value lives here.
pub static VERDICT_ADMISSION_BOUND_CHANNEL: VerdictAdmissionCandidateSource =
    VerdictAdmissionCandidateSource::BoundChannel;

impl VerdictAdmissionCandidateSource {
    /// The clause of a refusal that names this lookup and its bound.
    ///
    /// `missions` is how many candidates the caller actually assembled, which
    /// is not the bound: a lookup that found one mission and a lookup that
    /// could have found sixteen both say so.
    pub fn searched_clause(&self, missions: usize) -> String {
        match self {
            Self::SeatOfMission { seat, seats } => format!(
                "Searched {missions} mission(s) — the {seats} newest mission(s) that seat \
                 {seat}, the key this push authenticated as, of the newest \
                 {VERDICT_ADMISSION_MAX_PUSHER_SEATS} seats it holds — over one shared page of \
                 the newest {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on their \
                 channels."
            ),
            Self::SeatOfMissionInScope {
                seat,
                seats,
                held,
                within,
            } if *seats == 0 => format!(
                "{seat}, the key this push authenticated as, holds {held} seat(s) in the newest \
                 {VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS} authority transitions this relay \
                 could read, and none of them is on a mission of this repository's own channels \
                 — a seat on another repository's mission proves nothing about this one \
                 (finding 91). The search therefore read {within}. Searched {missions} \
                 mission(s) — the newest {VERDICT_ADMISSION_MAX_SESSIONS} on those channels \
                 whose founder is a founder of this repository, over one shared page of the \
                 newest {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on them."
            ),
            Self::SeatOfMissionInScope {
                seat,
                seats,
                held,
                within,
            } => format!(
                "Searched {missions} mission(s) — the {seats} of the {held} seat(s) held by \
                 {seat}, the key this push authenticated as, that lie in {within}. A seat on a \
                 mission of some other repository is not searched here and proves nothing about \
                 this one (finding 91). Of the newest \
                 {VERDICT_ADMISSION_MAX_PUSHER_SEATS} seats this key holds, over one shared \
                 page of the newest {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on \
                 their channels."
            ),
            Self::ProjectSessions { project, channels } => format!(
                "This key holds no seat in the newest \
                 {VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS} authority transitions this relay \
                 could read, so the search fell back to the project this repository names. \
                 Searched {missions} mission(s) — the newest \
                 {VERDICT_ADMISSION_MAX_SESSIONS} on the {channels} session channel(s) of \
                 {project} whose founder is a founder of this repository, of the newest \
                 {VERDICT_ADMISSION_MAX_PROJECT_CHANNELS} such channels — over one shared page \
                 of the newest {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on them."
            ),
            Self::BoundChannel => format!(
                "This key holds no seat in the newest \
                 {VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS} authority transitions this relay \
                 could read, and this repository names no project, so the search fell back to \
                 the channel it is bound to. Searched {missions} mission(s) — the newest \
                 {VERDICT_ADMISSION_MAX_SESSIONS} on that channel whose founder is a founder of \
                 this repository — over one shared page of the newest \
                 {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on it."
            ),
            Self::ThisMission { session_ref } => {
                format!("Searched only mission {session_ref}, the one this screen is showing.")
            }
        }
    }
}

/// Project active role seats from a session's stored kind 44228 transitions.
///
/// **Prediction-grade, and only for a caller that has no better source.** The
/// relay enforces with its own accepted projection
/// (`buzz_db::coding_session_acl::session_authority_for_hire`), which is
/// authoritative because the relay refuses to store a transition it did not
/// accept. A client reading events off the wire has no such guarantee, so
/// `bee git check --ref` uses this and says "prediction" rather than
/// "promise". Transitions that fail signature verification, name another
/// genesis, or arrive out of sequence are skipped rather than trusted.
pub fn active_seats_from_authority_transitions(
    events: &[Event],
    genesis_ref: &str,
) -> Vec<CodingSessionTeamActiveSeat> {
    let mut links: Vec<(
        u32,
        CodingSessionAuthorityTransitionType,
        String,
        Option<String>,
    )> = Vec::new();
    for event in events {
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_AUTHORITY_TRANSITION {
            continue;
        }
        if crate::verify_event(event).is_err() {
            continue;
        }
        let Ok(payload) = decode_coding_session_authority_transition(&event.content) else {
            continue;
        };
        if payload.genesis_ref != genesis_ref {
            continue;
        }
        links.push((
            payload.seq,
            payload.transition_type,
            payload.grantee_pubkey,
            payload.role,
        ));
    }
    links.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));

    let mut seats: Vec<CodingSessionTeamActiveSeat> = Vec::new();
    for (_, transition_type, grantee, role) in links {
        match transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat => {
                let Some(role) = role else { continue };
                seats.retain(|seat| seat.actor_pubkey != grantee);
                seats.push(CodingSessionTeamActiveSeat {
                    actor_pubkey: grantee,
                    role,
                });
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                seats.retain(|seat| {
                    seat.actor_pubkey != grantee || Some(&seat.role) != role.as_ref()
                });
            }
            _ => {}
        }
    }
    seats
}
