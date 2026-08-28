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
    use nostr::ToBech32;

    let agent_keys = nostr::Keys::generate();
    let pubkey = agent_keys.public_key().to_hex();
    let private_key_nsec = agent_keys
        .secret_key()
        .to_bech32()
        .map_err(|error| format!("failed to encode private key: {error}"))?;
    // Bridge nostr 0.37 → 0.36 (buzz-sdk) via hex round-trip.
    let compat_owner = nostr::Keys::parse(&owner_keys.secret_key().to_secret_hex())
        .map_err(|e| format!("failed to bridge owner keys: {e}"))?;
    let compat_agent = nostr::PublicKey::from_hex(&pubkey)
        .map_err(|e| format!("failed to bridge agent pubkey: {e}"))?;
    let auth_tag = buzz_sdk_pkg::nip_oa::compute_auth_tag(&compat_owner, &compat_agent, "")
        .map_err(|e| format!("failed to compute NIP-OA auth tag: {e}"))?;
    Ok((
        agent_keys,
        crate::managed_agents::crew_roles::MintedCrewIdentity {
            pubkey,
            private_key_nsec,
            auth_tag: Some(auth_tag),
        },
    ))
}
