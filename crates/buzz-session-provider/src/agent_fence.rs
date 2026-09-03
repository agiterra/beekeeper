//! Which of the sidecar's own environment variables never reach an adapter.
//!
//! The sidecar's environment is its identity. `BUZZ_PRIVATE_KEY` is the raw
//! nsec the provider signs with, and a consumer *fail-closed trusts* that
//! pubkey for the four provider-authored kinds — 44222 catalog, 44223
//! metadata, 44224 receipts, 44225 transcript. Anything holding that key can
//! mint transcript history the UI renders as genuine provider fact.
//! `BUZZ_AUTH_TAG` is the NIP-OA owner attestation for the same key, so a
//! forgery carrying both also carries the human owner's delegated standing.
//!
//! A coding-session execution is explicitly **not** a managed agent: the
//! metadata it produces always carries `agentRef: null`, and the field's
//! comment in `buzz-core` says exactly that. It is work the provider
//! supervises, not a Buzz participant acting as itself. So it must not hold
//! the provider's identity — and until this module existed it held all of it,
//! because an ACP spawn inherits the parent environment wholesale and the
//! provider passed only additive per-runtime variables on top.
//!
//! # Why a namespace fence and not an allowlist
//!
//! `buzz-terminal`'s `env_fence` clears the child environment and rebuilds it
//! from an eight-key allowlist, on the argument that a denylist is only as
//! current as the last person who remembered to extend it. That argument is
//! right, and it is why [`PREFIXES`] rather than a list of known-bad names
//! carries the weight here: everything spelled `BUZZ_*` is fenced, including
//! keys invented after this was written.
//!
//! The allowlist *shape* does not transfer. A terminal needs eight variables;
//! a coding agent runs the developer's real toolchain in a real checkout and
//! needs `PATH`, `HOME`, `SSH_AUTH_SOCK`, language-runtime configuration, and
//! the adapter's own credential cache. Clearing that wholesale does not
//! produce a safer agent, it produces one that cannot build. So: exhaustive
//! over the namespace Buzz owns, enumerated for the handful of third-party
//! secrets that reach the sidecar from a developer's `.env` without carrying
//! the prefix.
//!
//! # Consequence, intended
//!
//! The `bee` CLI no longer authenticates from inside a coding-session agent's
//! shell. It used to work by accident, by reading the provider's key out of
//! the inherited environment — which made every session's agent
//! indistinguishable from the provider itself on the wire. A coding-session
//! execution that should speak to Buzz gets its **own** identity through
//! `agent_ref`; it does not borrow the provider's.

use buzz_acp::acp::EnvFence;

/// The namespace Buzz owns end to end.
///
/// Nothing under it is useful to an ACP adapter: the adapter learns what to run
/// from argv and the additive per-runtime environment, and the `BUZZ_CSP_*`
/// surface configures the sidecar, not its children.
const PREFIXES: &[&str] = &["BUZZ_"];

/// Secrets that reach the sidecar without the `BUZZ_` prefix.
///
/// These arrive from a developer's `.env` by way of the launching shell rather
/// than from anything the desktop sets deliberately, and they authorize direct
/// writes to the media store and the search index — a path that bypasses relay
/// authorization entirely.
const KEYS: &[&str] = &[
    "NOSTR_PRIVATE_KEY",
    "TYPESENSE_API_KEY",
    "S3_ACCESS_KEY",
    "S3_SECRET_KEY",
];

/// Variables under a fenced prefix that are passed through anyway.
///
/// Deliberately empty. It exists so that admitting a non-secret `BUZZ_*` the
/// adapter genuinely needs is a one-line, reviewable exception rather than a
/// reason to weaken [`PREFIXES`]. Do not add a credential here.
const EXEMPT: &[&str] = &[];

/// The tools a seated execution must not use, named in its own briefing.
///
/// Seats run with the operator's `HOME`, so a harness that keeps its session
/// registry there — Claude Code's `~/.claude` is the live example — exposes
/// tools that reach *other* sessions on this machine without going near the
/// relay. `Task` and `Agent` spawn local subagents; `SendMessage` addresses
/// another session directly. All three produce coordination that the
/// provider's transcript never sees.
///
/// # Enforced on claude-agent-acp; a briefing on codex-acp
///
/// For a seated execution on `claude-agent-acp`, [`crate::session`] passes
/// this list to `AcpClient::set_disallowed_tools`, which writes it to
/// `_meta.claudeCode.options.disallowedTools` on `session/new` (and on
/// `session/resume` / `session/load`, which rebuild the session from the same
/// `_meta`). The adapter merges it into the SDK query's `disallowedTools`
/// (`dist/acp-agent.js:4913` in 0.70.0), so the tools are absent from the
/// model's toolset, not merely discouraged.
///
/// **On codex-acp it is still only a briefing.** `codex-acp` 1.6.2 exposes no
/// per-session tool denial: its argv is `--client-name/--client-title/
/// --client-version` on `login`, it reads `CODEX_PATH` and `CODEX_CONFIG`, and
/// there is no `_meta` option of this shape. There is nothing to enforce with,
/// so a codex seat is held to this list by [`actor_seat_briefing`] alone.
/// Unseated executions are not fenced this way at all, by design.
///
/// # Mechanisms measured and rejected
///
/// Measured against `claude` 2.1.248 and `@agentclientprotocol/claude-agent-acp`
/// 0.70.0 on 2026-08-28. Neither of these works; do not retry them:
///
/// - A per-seat `CLAUDE_CONFIG_DIR` holding a `permissions.deny` settings
///   file. Relocating it makes `claude` answer `Not logged in · Please run
///   /login`, because the credential it reads out of the macOS keychain is
///   keyed by the configuration home. Seeding the new directory with the
///   operator's `.claude.json`, and pointing `CLAUDE_SECURESTORAGE_CONFIG_DIR`
///   back at `~/.claude`, both still fail.
/// - `CLAUDE_CODE_MANAGED_SETTINGS_PATH`. The name exists in the binary but a
///   settings file supplied that way had no effect on the resolved
///   permissions; the same file passed as `--settings` removed the denied tool
///   from the toolset outright. The adapter offers no argv for it.
///
/// So the per-session `_meta` option is the only mechanism that denies these
/// tools without breaking the seat's login, and it is the one wired up.
/// [`actor_seat_briefing`] must keep naming the tools regardless: on codex it
/// is the whole fence, and on claude a named rule beats a tool that has simply
/// vanished without explanation.
pub(crate) const SEAT_OUT_OF_BOUNDS_TOOLS: &[&str] = &["Task", "Agent", "SendMessage"];

/// The fence every adapter this sidecar spawns is subject to.
pub(crate) const FENCE: EnvFence = EnvFence {
    keys: KEYS,
    prefixes: PREFIXES,
    exempt: EXEMPT,
};

/// What a fenced adapter is told about the consequence above, in its own words.
///
/// The fence is invisible from inside the adapter: it sees an environment with
/// no `BUZZ_*` in it and no explanation, so an operator who asks it about Buzz
/// coordination gets "unavailable — nothing is configured", which reads as a
/// broken install rather than a deliberate boundary. That is exactly what
/// happened on 2026-08-21: a session asked to read its Project Pulse reported
/// the empty key files as a misconfiguration.
///
/// So the provider says it. Every rule here is a fact about this process, not
/// an aspiration:
///
/// - It cannot authenticate `bee` — [`FENCE`] removes the whole namespace
///   before the adapter is spawned (`session.rs`, `spawn_with_env_fence`).
/// - Its session state *is* published: the provider observes branch, `HEAD`
///   commit and dirty state itself ([`crate::git_probe`]) and publishes them
///   as kind:44223 session metadata, which is what the Project Pulse digest
///   folds into its session groups (`buzz-acp/src/pulse_fetch.rs`).
/// - It receives no `[Project Pulse]` injection — that is composed in the ACP
///   harness's pool for managed agents (`buzz-acp/src/pool.rs`), a path a
///   coding session never takes.
///
/// Consequently this text must never instruct the adapter to run a relay
/// command, and `buzz-acp`'s base prompt — written for the *unfenced* managed
/// agents, which do inherit the harness's credentials — must keep instructing
/// exactly that. `the_fenced_briefing_never_tells_a_session_to_write_the_pulse`
/// pins both halves.
pub(crate) const FENCED_SESSION_BRIEFING: &str = "Buzz coding-session briefing: you are running inside a Buzz coding session, launched and supervised by the Buzz session provider.\n\nThe provider's Buzz identity is not yours. Every BUZZ_* variable is deliberately removed from this process's environment before you start, so the `bee` CLI cannot authenticate from your shell and this session holds no relay credentials. Do not run `bee` commands that talk to the relay, do not go looking for a key in .env, ~/.config/buzz/, or the environment, and do not report the missing key as a misconfiguration — the absence is the design, not a broken setup.\n\nYou do not need those credentials to be seen. The provider itself observes and publishes this session's state — branch, HEAD commit, dirty worktree, and verified liveness — so when this session's channel belongs to a project, that published state is what the project's Pulse and Beekeeper Desktop show for you. Routine progress needs no post from you.\n\nYou will not receive a Project Pulse digest in this session and you cannot read one from here. If you need to know what other sessions or people are working on before you touch shared code, say so and ask your operator in this conversation: they can see the Pulse and can post an entry on your behalf.\n\nRun long work in the foreground and wait for it. Do not detach a build, a test run, or any other command into the background and end your turn promising to report back when it finishes — nothing will wake you to do so, and your operator will be left watching a session that looks busy and has nothing left to say. If something takes a long time, run it in the foreground with an explicit timeout, or run it in pieces you can report on as you go.";

/// The same briefing, for an execution that **is** an agent seat.
///
/// The fence still runs — every `BUZZ_*` the sidecar holds is still removed —
/// and then exactly four variables are put back, all of them the *seat's* own
/// ([`crate::actor_seats::ActorSeat::post_fence_env`]). So the unseated
/// briefing above is now a lie for this process, in the specific way this
/// project treats as a bug: it would tell an agent that holds working relay
/// credentials that it holds none, and a competent agent would then either
/// refuse to use them or report a misconfiguration that is not one.
///
/// This text therefore says the true thing instead, and says it narrowly:
///
/// - The identity in this shell is **the seat's**, named by pubkey, and the
///   relay it authenticates against is named too — a seat that does not know
///   which community it is speaking into cannot tell a sibling from a
///   stranger.
/// - It is *not* the provider's identity. The distinction matters because the
///   provider signs the transcript and metadata a human reads as fact; a seat
///   that believed it could sign those would be forging its own record.
/// - The role it holds, so a crew's conventions have something to attach to.
/// - The same do-not-detach rule the unseated briefing carries: nothing
///   reads the adapter's output between turns whether or not the execution
///   is seated, so background work an agent promises to report back on is a
///   promise the provider cannot keep.
///
/// - The relay is the only channel out. A seat runs under the operator's
///   `HOME`, so the harness's own local subagent and cross-session tools
///   ([`SEAT_OUT_OF_BOUNDS_TOOLS`]) can reach other sessions on this machine
///   directly — and on 2026-08-27 a lead used one to dispatch a builder, and
///   the two seats then talked twice outside the relay. Nothing said that way
///   is in the transcript the provider publishes, so it cannot be read, cited
///   or replayed. Until the fence is enforced in the adapter's own options,
///   this sentence is the only thing standing there, and it says so plainly
///   rather than implying the tools are absent.
/// - After dispatching, end the turn. The report comes back as its own
///   addressed turn.
/// - Reports arrive as turns, so polling the inbox inside a turn reads the
///   same report the relay is about to deliver — which is exactly how the
///   2026-08-27 lead came to read every report twice and call it relay
///   redelivery.
///
/// It deliberately does **not** hand out a task list of `bee` commands. What a
/// seat may usefully do with its identity is a slice-4 question (`bee sessions
/// send`, the inbox, the roster); promising verbs that do not exist yet would
/// reproduce the 2026-08-21 failure in the opposite direction.
pub(crate) fn actor_seat_briefing(actor_pubkey: &str, role: &str, relay_url: &str) -> String {
    let out_of_bounds = SEAT_OUT_OF_BOUNDS_TOOLS.join(", ");
    format!(
        "Buzz coding-session briefing: you are running inside a Buzz coding session, launched and supervised by the Buzz session provider, and you are seated in it as a Buzz agent.\n\nYou hold your own Buzz identity in this shell: public key {actor_pubkey}, seated with the role \"{role}\", authenticated against the relay at {relay_url}. Run the `bee` CLI as `$BEE` - your host chose one binary, set $BEE to its absolute path, and put its directory first on your PATH, so `$BEE` and a bare `bee` are the same build. Never a path someone typed at you, and never a path from a transcript. It speaks as that identity. Those credentials are yours, not the provider's: the session provider signs this session's transcript, metadata, and receipts with a different key, and nothing you publish can claim to be provider-authored fact.\n\nEvery other Buzz variable is removed from this process's environment before you start, so anything under BUZZ_* that you cannot find is deliberately absent rather than misconfigured. Do not go looking for additional keys in .env, ~/.config/buzz/, or the environment, and never write your own key anywhere - not into a file in the working tree, not into a commit, and not into anything you post.\n\nThe provider itself observes and publishes this session's state - branch, HEAD commit, dirty worktree, and verified liveness - so routine progress needs no post from you. You will not receive a Project Pulse digest in this session.\n\nThe relay is the only channel to other seats and to the operator. Local subagent and cross-session tools - {out_of_bounds} - are out of bounds in this seat: they reach other sessions on this computer directly, and nothing said through them appears in the transcript this session publishes, so no one can read it, cite it, or replay it. Say it over the relay or it did not happen.\n\nAfter you dispatch work to another seat, end your turn. An addressed relay turn wakes you, so the reply arrives as a turn of its own; holding this turn open to wait for it only leaves the seat busy with nothing to say.\n\nReports arrive as turns. Do not poll `bee sessions inbox` inside a turn looking for one - you will read the same report the relay is about to hand you and count it twice.\n\nRun long work in the foreground and wait for it. Do not detach a build, a test run, or any other command into the background and end your turn promising to report back when it finishes - only an addressed relay turn wakes you, and a background job finishing is not one, so whoever is waiting on you is left watching a seat that looks busy and has nothing left to say. If something takes a long time, run it in the foreground with an explicit timeout, or run it in pieces you can report on as you go."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_seats::ActorSeat;

    /// The credentials named in the finding, one assertion each so a
    /// regression names the variable it re-exposed.
    #[test]
    fn the_provider_identity_and_infrastructure_secrets_are_fenced() {
        for key in [
            "BUZZ_PRIVATE_KEY",
            "BUZZ_ACP_PRIVATE_KEY",
            "BUZZ_AUTH_TAG",
            "BUZZ_API_TOKEN",
            "BUZZ_S3_ACCESS_KEY",
            "BUZZ_S3_SECRET_KEY",
            "BUZZ_DEV_KEYRING_SERVICE",
            "NOSTR_PRIVATE_KEY",
            "TYPESENSE_API_KEY",
            "S3_ACCESS_KEY",
            "S3_SECRET_KEY",
        ] {
            assert!(FENCE.covers(key), "{key} escaped the fence");
        }
    }

    /// The property an enumerated denylist cannot offer: a credential invented
    /// tomorrow is fenced for where it lives, not because someone listed it.
    #[test]
    fn an_unknown_buzz_variable_is_fenced_on_its_prefix_alone() {
        assert!(FENCE.covers("BUZZ_SOME_FUTURE_CREDENTIAL"));
    }

    /// The rule the 2026-08-21 finding cost us: never instruct an agent to do
    /// something its own environment forbids.
    ///
    /// Both halves are asserted together, in one test, because the defect was
    /// the *pair* drifting apart — a write instruction that is correct for the
    /// unfenced managed agents leaking into the fenced coding-session path.
    #[test]
    fn the_fenced_briefing_never_tells_a_session_to_write_the_pulse() {
        for forbidden in [
            "bee pulse update",
            "bee pulse digest",
            "bee pulse list",
            "bee pulse sessions",
            "BUZZ_PULSE_PROJECT",
        ] {
            assert!(
                !FENCED_SESSION_BRIEFING.contains(forbidden),
                "the fenced briefing tells a credential-less session to run `{forbidden}`"
            );
        }
        assert!(
            FENCED_SESSION_BRIEFING.contains("cannot authenticate"),
            "the fenced briefing must say why `bee` will not work here"
        );
        assert!(
            FENCED_SESSION_BRIEFING.contains("ask your operator"),
            "the fenced briefing must name the path that does work"
        );
        // Same rule, second instance: the provider cannot deliver anything the
        // agent produces after a turn ends — nothing reads the adapter's
        // output between turns — so an agent must never be left believing it
        // can detach work and report back later.
        assert!(
            FENCED_SESSION_BRIEFING.contains("foreground"),
            "the fenced briefing must tell a session not to detach long work it \
             cannot be woken to report on"
        );

        // The unfenced audience keeps the instruction: a managed ACP agent
        // inherits the harness's credentials and is the only writer Pulse has.
        assert!(
            buzz_acp::BASE_PROMPT.contains("post it yourself with `bee pulse update`"),
            "the managed-agent base prompt lost its Pulse write instruction"
        );
    }

    /// The seated half of the same rule: a briefing must describe the process
    /// it is actually installed in.
    ///
    /// The unseated text says `bee` cannot authenticate, which is true for a
    /// fenced execution and false for a seat that was handed its own key — so
    /// the two variants are asserted against each other here, not separately,
    /// because the defect this pins is the pair drifting into agreement.
    #[test]
    fn the_actor_seat_briefing_states_the_identity_the_seat_actually_holds() {
        let actor = "cd".repeat(32);
        let briefing = actor_seat_briefing(&actor, "lead", "wss://relay.example");

        assert!(briefing.contains(&actor), "the seat is not named");
        assert!(briefing.contains("wss://relay.example"), "no relay named");
        assert!(briefing.contains("\"lead\""), "no role named");
        assert!(
            briefing.contains("You hold your own Buzz identity"),
            "the seated briefing must say the shell is authenticated"
        );

        // The exact sentence the unseated briefing exists to deliver must not
        // survive into the seated one: it is false here.
        assert!(
            !briefing.contains("cannot authenticate"),
            "the seated briefing repeats the fenced briefing's claim"
        );
        assert!(
            FENCED_SESSION_BRIEFING.contains("cannot authenticate"),
            "the unseated briefing must still say why `bee` will not work there"
        );

        // Neither variant promises Pulse writes, and neither leaks a key.
        for text in [FENCED_SESSION_BRIEFING.to_owned(), briefing.clone()] {
            for forbidden in ["bee pulse update", "bee pulse digest", "nsec1"] {
                assert!(
                    !text.contains(forbidden),
                    "a session briefing contains `{forbidden}`"
                );
            }
        }

        // The seat is told the boundary that makes the provider's record
        // trustworthy: its key is not the provider's.
        assert!(
            briefing.contains("not the provider's"),
            "the seated briefing must separate the seat from the provider"
        );

        // The one rule that is identical for both variants, asserted on both
        // in the same test for the same reason as the pair above: nothing
        // reads the adapter's output between turns, seated or not, so an
        // agent must never be left believing it can detach work and be woken
        // to report on it. A seat is the *more* dangerous case — a sibling or
        // a lead may be blocked waiting on the report that never comes.
        for (label, text) in [
            ("fenced", FENCED_SESSION_BRIEFING.to_owned()),
            ("seated", briefing.clone()),
        ] {
            assert!(
                text.contains("foreground"),
                "the {label} briefing lost the do-not-detach rule"
            );
        }
    }

    /// Ledger 77 (*Fence*, a): a lead dispatched a builder through a local
    /// subagent and the two seats then talked twice outside the relay. The
    /// briefing must name the tools rather than leave the seat to infer them.
    #[test]
    fn the_seat_briefing_names_the_tools_that_bypass_the_relay() {
        let briefing = actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example");
        for tool in SEAT_OUT_OF_BOUNDS_TOOLS {
            assert!(
                briefing.contains(tool),
                "the seated briefing does not name `{tool}`"
            );
        }
        for tool in ["Task", "Agent", "SendMessage"] {
            assert!(
                SEAT_OUT_OF_BOUNDS_TOOLS.contains(&tool),
                "`{tool}` is no longer named as out of bounds for a seat"
            );
        }
        assert!(
            briefing.contains("The relay is the only channel to other seats"),
            "the seated briefing must say where coordination happens"
        );
        assert!(
            briefing.contains("out of bounds"),
            "the seated briefing must say the tools are forbidden, not absent"
        );
    }

    /// Ledger 77 (*Lead pack*, c): after dispatching, end the turn — the
    /// report arrives as its own addressed turn.
    #[test]
    fn the_seat_briefing_tells_a_dispatcher_to_end_its_turn() {
        let briefing = actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example");
        assert!(
            briefing.contains("end your turn"),
            "the seated briefing must tell a dispatching seat to end the turn"
        );
        assert!(
            briefing.contains("An addressed relay turn wakes you"),
            "the seated briefing must say what does wake a seat"
        );
    }

    /// Same finding, second half: polling the inbox inside a turn reads the
    /// report the relay is about to deliver, which is how the 2026-08-27 lead
    /// counted every report twice.
    #[test]
    fn the_seat_briefing_forbids_polling_the_inbox_inside_a_turn() {
        let briefing = actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example");
        assert!(
            briefing.contains("Reports arrive as turns"),
            "the seated briefing must say how a report arrives"
        );
        assert!(
            briefing.contains("Do not poll `bee sessions inbox` inside a turn"),
            "the seated briefing must forbid polling the inbox inside a turn"
        );
    }

    /// Ledger 77 (*Seat briefing*): "nothing wakes you between turns" is false
    /// for a seat, and the do-not-detach rule must survive without it.
    #[test]
    fn the_seated_do_not_detach_rule_no_longer_rests_on_a_falsehood() {
        let briefing = actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example");
        assert!(
            briefing.contains("foreground"),
            "the seated briefing lost the do-not-detach rule"
        );
        for falsehood in [
            "nothing will wake you",
            "nothing reads this process between turns, so nothing will wake you",
            "Nothing wakes you between turns",
        ] {
            assert!(
                !briefing.contains(falsehood),
                "the seated briefing still claims `{falsehood}`, which an \
                 addressed relay turn disproves"
            );
        }
        assert!(
            briefing.contains("a background job finishing is not one"),
            "the seated briefing must say why detaching still fails a seat"
        );
    }

    /// The fence itself is unchanged by seating: `EXEMPT` stays empty, so a
    /// seat's variables arrive by explicit post-fence injection and never by
    /// weakening the namespace rule.
    #[test]
    fn seating_an_agent_never_widens_the_fence() {
        assert!(
            EXEMPT.is_empty(),
            "an exemption was added instead of a post-fence injection"
        );
        for key in ["BUZZ_PRIVATE_KEY", "BUZZ_RELAY_URL", "BUZZ_AUTH_TAG"] {
            assert!(FENCE.covers(key), "{key} is no longer fenced");
        }
    }

    /// `BEE` and `PATH` are outside the `BUZZ_` namespace on purpose, and the
    /// fence must not start covering either.
    ///
    /// Fencing `BEE` would strip the host's choice back out; fencing `PATH`
    /// would leave the seat with no binaries at all. Both are supplied
    /// **post-fence** ([`crate::actor_seats::ActorSeat::post_fence_env_with_bee`]),
    /// which is also the only reason an operator's ambient `BEE` cannot
    /// survive: the fence would have left it alone.
    #[test]
    fn the_bee_the_host_chose_is_outside_the_fence_and_injected_over_it() {
        for key in [crate::seat_bee::BEE_ENV, "PATH"] {
            assert!(
                !FENCE.covers(key),
                "{key} was fenced, so the host's chosen bee could never reach the seat"
            );
        }
        let seat = ActorSeat {
            pubkey: "ab".repeat(32),
            nsec: "nsec1secret".to_owned(),
            auth_tag: None,
            relay_url: "wss://relay.example".to_owned(),
            display_name: None,
            pack_dir: None,
            persona_id: None,
            pack_ref: None,
        };
        let bee = crate::seat_bee::SeatBee {
            path: std::path::PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS/bee"),
            source: buzz_core::coding_session_payload::BeeStampSource::Bundled,
        };
        let inherited = std::env::join_paths([std::path::Path::new("/usr/bin")]).expect("a PATH");
        let env = seat.post_fence_env_with_bee(Some("builder"), None, Some(&bee), Some(&inherited));
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == crate::seat_bee::BEE_ENV)
                .map(|(_, value)| value.as_str()),
            Some("/Applications/Beekeeper.app/Contents/MacOS/bee"),
            "the post-fence list must carry the binary the host chose: {env:?}"
        );
        assert!(
            env.iter().any(|(key, value)| key == "PATH"
                && value.starts_with("/Applications/Beekeeper.app/Contents/MacOS:")),
            "the chosen directory must lead the seat's PATH: {env:?}"
        );
        // A host with no `bee` adds nothing: the seat keeps exactly the
        // environment it has today rather than an invented one.
        assert_eq!(
            seat.post_fence_env_with_bee(Some("builder"), None, None, Some(&inherited)),
            seat.post_fence_env_in_project(Some("builder"), None),
        );
    }

    /// The briefing cannot be skipped, so it is the half of the `$BEE` rule
    /// that ships regardless of whether a pack repeats it.
    #[test]
    fn the_seated_briefing_names_the_variable_rather_than_a_bare_command() {
        let briefing = actor_seat_briefing(&"ab".repeat(32), "builder", "wss://relay.example");
        assert!(
            briefing.contains("Run the `bee` CLI as `$BEE`"),
            "the briefing must name the variable the host actually sets"
        );
        assert!(
            briefing.contains("never a path from a transcript"),
            "the briefing must refuse the prose path that was honoured only sometimes"
        );
    }

    /// Over-fencing is the other failure mode: an agent that cannot find its
    /// toolchain is as broken as one that can sign as the provider.
    #[test]
    fn the_developer_toolchain_environment_survives() {
        for key in [
            "PATH",
            "HOME",
            "SHELL",
            "SSH_AUTH_SOCK",
            "TMPDIR",
            "CLAUDE_CODE_EXECUTABLE",
            "ANTHROPIC_API_KEY",
            "CODEX_CONFIG",
            "HERMES_ACP_SKIP_CONFIGURED_MCP",
        ] {
            assert!(
                !FENCE.covers(key),
                "{key} was fenced but the agent needs it"
            );
        }
    }
}
