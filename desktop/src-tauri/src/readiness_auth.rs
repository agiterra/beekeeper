//! Pure, no-secret validation of stored NIP-OA owner attestations.

use nostr::PublicKey;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReadinessAuthTag {
    pub present: bool,
    pub verified_owner: Option<String>,
    pub invalid: bool,
    pub owner_mismatch: bool,
}

/// Verify a Beekeeper-minted auth tag against its subject pubkey.
///
/// Readiness accepts only the empty conditions Beekeeper mints. The canonical
/// SDK verifier enforces the four-string encoding, distinct owner/subject,
/// condition grammar, signature encoding, and BIP-340 Schnorr signature.
pub(crate) fn inspect_auth_tag(
    raw: Option<&str>,
    subject_hex: &str,
    expected_owner: Option<&str>,
) -> ReadinessAuthTag {
    let Some(raw) = raw else {
        return ReadinessAuthTag::default();
    };
    let invalid = || ReadinessAuthTag {
        present: false,
        verified_owner: None,
        invalid: true,
        owner_mismatch: false,
    };
    let Ok(subject) = PublicKey::from_hex(subject_hex) else {
        return invalid();
    };
    let Ok(tag) = buzz_sdk_pkg::nip_oa::parse_auth_tag(raw) else {
        return invalid();
    };
    if tag.as_slice().get(2).map(String::as_str) != Some("") {
        return invalid();
    }
    match buzz_sdk_pkg::nip_oa::verify_auth_tag(raw, &subject) {
        Ok(owner) => {
            let owner = owner.to_hex();
            let owner_mismatch = expected_owner.is_some_and(|expected| expected != owner);
            ReadinessAuthTag {
                present: !owner_mismatch,
                verified_owner: Some(owner),
                invalid: false,
                owner_mismatch,
            }
        }
        Err(_) => invalid(),
    }
}
