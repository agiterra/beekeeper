//! The one place a managed agent's identity is minted.
//!
//! Lifted out of `agents.rs` so the file stays inside the repo's size
//! ratchet; the function is unchanged.

/// Mint one agent identity: a fresh keypair, its bech32 secret, and the NIP-OA
/// auth tag binding it to this workspace's owner.
///
/// The single key-minting path for a managed agent. `create_managed_agent` and
/// the crew-role installer both call it, so an agent can never appear with a
/// key the owner never attested — the divergence that produces an actor the
/// provider cannot impersonate.
///
/// # Errors
///
/// Fails closed: a bad owner key, an un-encodable secret, or an auth tag that
/// cannot be computed aborts before anything is written.
pub(crate) fn mint_agent_identity(
    owner_keys: &nostr::Keys,
) -> Result<
    (
        nostr::Keys,
        crate::managed_agents::crew_roles::MintedCrewIdentity,
    ),
    String,
> {
    let minted = beekeeper_host_core::provision::mint_agent_identity(owner_keys)?;
    Ok((
        minted.keys,
        crate::managed_agents::crew_roles::MintedCrewIdentity {
            pubkey: minted.pubkey,
            private_key_nsec: minted.private_key_nsec,
            auth_tag: Some(minted.auth_tag),
        },
    ))
}
