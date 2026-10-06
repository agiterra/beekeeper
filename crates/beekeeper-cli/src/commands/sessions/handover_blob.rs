//! The Blossom leg of a handover: a working-tree patch too large for a NIP-34
//! event, and the checks it must pass before anything is applied.
//!
//! # Why a blob is verified and a patch event is not
//!
//! A NIP-34 patch artifact names an **event id**, and a nostr event id is the
//! hash of its own content: fetching it by id and getting different bytes is
//! not possible without the relay also breaking its signature check. A blob
//! artifact names a **Blossom hash and a byte count**, and those are the
//! author's statement about a body the relay serves by path. Nothing in the
//! fetch proves the two agree — so this module proves it, before
//! `git apply` ever sees the bytes.
//!
//! Both checks run, and in this order:
//!
//! 1. **Length**, against the `bytes` the checkpoint recorded. A body longer
//!    than the record is refused outright rather than hashed: hashing an
//!    unbounded response to discover it is wrong means having already read it.
//! 2. **sha256**, against `hash`.
//! 3. **UTF-8**, because a patch is text and `git apply` will not take
//!    anything else.
//!
//! A failure at any step is a `missing` line naming the artifact and the
//! reason, never a silent skip and never a partial apply: the caller has not
//! yet touched the working tree when this returns.

use sha2::{Digest, Sha256};

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The MIME a captured patch travels under, on the way up and on the way down.
///
/// One constant so the upload in `handover_checkpoint` and any reader of the
/// stored blob cannot drift apart about what was stored.
pub const PATCH_BLOB_MIME: &str = "text/x-patch";

/// Why a fetched blob was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobRejection {
    /// The body was longer than the checkpoint said.
    TooLong {
        /// Bytes actually returned.
        got: usize,
        /// Bytes the checkpoint recorded.
        expected: u64,
    },
    /// The body was shorter than the checkpoint said.
    TooShort {
        /// Bytes actually returned.
        got: usize,
        /// Bytes the checkpoint recorded.
        expected: u64,
    },
    /// The body hashed to something else.
    HashMismatch {
        /// sha256 of what was returned.
        got: String,
        /// The hash the checkpoint recorded.
        expected: String,
    },
    /// The body is not text, so it is not a patch.
    NotUtf8(String),
}

impl BlobRejection {
    /// The sentence a `missing` line carries.
    ///
    /// Always names both numbers or both hashes. "the blob was wrong" sends a
    /// person to look; "1024 bytes, the checkpoint said 2048" tells them which
    /// half to distrust.
    pub fn reason(&self) -> String {
        match self {
            Self::TooLong { got, expected } => format!(
                "the relay returned {got} bytes for a blob the checkpoint recorded as {expected}: \
                 a longer body than the record is refused unread rather than applied"
            ),
            Self::TooShort { got, expected } => format!(
                "the relay returned {got} bytes for a blob the checkpoint recorded as {expected}: \
                 the body is incomplete, so nothing was applied"
            ),
            Self::HashMismatch { got, expected } => format!(
                "the blob the relay served hashes to {got}, not the {expected} the checkpoint \
                 recorded: these are not the bytes that were checkpointed, so nothing was applied"
            ),
            Self::NotUtf8(error) => format!(
                "the blob is not valid UTF-8 patch text ({error}), so `git apply` could not have \
                 read it and nothing was applied"
            ),
        }
    }
}

/// Check a fetched blob against what the checkpoint said it was.
///
/// Pure, so the three ways a blob can be wrong are testable without a relay.
///
/// # Errors
/// The first check that fails, as a [`BlobRejection`].
pub fn verify_blob(
    body: &[u8],
    expected_hash: &str,
    expected_bytes: Option<u64>,
) -> Result<String, BlobRejection> {
    if let Some(expected) = expected_bytes {
        let got = body.len();
        // Length before hash: a body longer than the record is refused for
        // being longer, not for hashing differently, and the caller learns
        // which of the two facts is wrong.
        match (got as u64).cmp(&expected) {
            std::cmp::Ordering::Greater => {
                return Err(BlobRejection::TooLong { got, expected });
            }
            std::cmp::Ordering::Less => {
                return Err(BlobRejection::TooShort { got, expected });
            }
            std::cmp::Ordering::Equal => {}
        }
    }
    let digest = hex::encode(Sha256::digest(body));
    if !digest.eq_ignore_ascii_case(expected_hash) {
        return Err(BlobRejection::HashMismatch {
            got: digest,
            expected: expected_hash.to_ascii_lowercase(),
        });
    }
    String::from_utf8(body.to_vec()).map_err(|error| BlobRejection::NotUtf8(error.to_string()))
}

/// Fetch one blob artifact and return its patch text, or say why not.
///
/// The whole of the blob leg of a reconstruction: fetch, then
/// [`verify_blob`], then hand back text. The working tree is untouched
/// either way — verification happens before the caller is given anything it
/// could apply.
///
/// # Errors
/// A sentence naming the artifact and what was wrong with it, suitable for a
/// continuation's `missing` list verbatim.
pub async fn fetch_verified_blob(
    client: &BeekeeperClient,
    hash: Option<&str>,
    expected_bytes: Option<u64>,
) -> Result<String, CliError> {
    let hash = hash.ok_or_else(|| CliError::Other("a blob artifact named no hash".to_owned()))?;
    let body = client.download_media(hash).await.map_err(|error| {
        CliError::NotFound(format!(
            "the blob could not be fetched from the relay ({error}), so the uncommitted bytes it \
             carried were not recovered"
        ))
    })?;
    verify_blob(&body, hash, expected_bytes)
        .map_err(|rejection| CliError::Other(rejection.reason()))
}

#[cfg(test)]
#[path = "handover_blob_tests.rs"]
mod tests;
