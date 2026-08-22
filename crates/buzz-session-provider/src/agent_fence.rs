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
pub(crate) const FENCED_SESSION_BRIEFING: &str = "Buzz coding-session briefing: you are running inside a Buzz coding session, launched and supervised by the Buzz session provider.\n\nThe provider's Buzz identity is not yours. Every BUZZ_* variable is deliberately removed from this process's environment before you start, so the `bee` CLI cannot authenticate from your shell and this session holds no relay credentials. Do not run `bee` commands that talk to the relay, do not go looking for a key in .env, ~/.config/buzz/, or the environment, and do not report the missing key as a misconfiguration — the absence is the design, not a broken setup.\n\nYou do not need those credentials to be seen. The provider itself observes and publishes this session's state — branch, HEAD commit, dirty worktree, and verified liveness — so when this session's channel belongs to a project, that published state is what the project's Pulse and Bee Keeper Desktop show for you. Routine progress needs no post from you.\n\nYou will not receive a Project Pulse digest in this session and you cannot read one from here. If you need to know what other sessions or people are working on before you touch shared code, say so and ask your operator in this conversation: they can see the Pulse and can post an entry on your behalf.";

#[cfg(test)]
mod tests {
    use super::*;

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

        // The unfenced audience keeps the instruction: a managed ACP agent
        // inherits the harness's credentials and is the only writer Pulse has.
        assert!(
            buzz_acp::BASE_PROMPT.contains("post it yourself with `bee pulse update`"),
            "the managed-agent base prompt lost its Pulse write instruction"
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
