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
//! The `buzz` CLI no longer authenticates from inside a coding-session agent's
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
