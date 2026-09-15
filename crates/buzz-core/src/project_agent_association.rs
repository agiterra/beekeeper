//! A managed agent's durable project association, as it travels on the wire.
//!
//! An agent is a durable, named project participant with one primary role. The
//! owner's computer records that association locally and publishes it on the
//! agent's owner-signed kind:30177 so a lead's CLI and a second computer can
//! discover the project's agents without reading anybody's disk.
//!
//! The content carries [`project_agent_digest`] rather than the coordinate: a
//! compact, fixed-length equality key that a reader matches against the digest
//! of the coordinate it is asking about. It is **not** confidentiality. A
//! project coordinate is `30621:<owner>:<slug>`, and owners and slugs are
//! guessable, so anyone who can read the kind:30177 can hash candidate
//! coordinates, identify the project, and correlate the agents that share it.
//! Associations are therefore published only for projects whose head is
//! public. A private project's associations are never published; its agents
//! are known only on the computers that hold them.
//!
//! The association is a claim by the event's author. **Readers must check the
//! author's authority** — the project's creator or a roster owner or
//! collaborator — before treating it as project membership. The relay does not
//! check it, and a role pack or a matching role name never implies it.

use sha2::{Digest, Sha256};

use crate::kind::normalize_project_coordinate;

/// Domain separator for [`project_agent_digest`]. Versioned so a future
/// association shape can never collide with this one.
pub const PROJECT_AGENT_DIGEST_DOMAIN: &str = "buzz-project-agent/v1\n";

/// Content key on kind:30177 carrying [`project_agent_digest`].
pub const PROJECT_AGENT_DIGEST_CONTENT_KEY: &str = "project_digest";

/// Content key on kind:30177 carrying the agent's primary role slug.
pub const PROJECT_AGENT_ROLE_CONTENT_KEY: &str = "home_role";

/// The digest a kind:30177 carries for the project an agent belongs to:
/// lowercase hex SHA-256 of [`PROJECT_AGENT_DIGEST_DOMAIN`] followed by the
/// normalized `30621:<lowercase-owner-hex>:<dtag>` coordinate.
///
/// An equality key, not a secret: it is deterministic over a public domain
/// separator and a guessable coordinate, so anyone can compute it for a
/// candidate project. Publish it only for a public project.
///
/// Surrounding **ASCII** whitespace (space, `\t`, `\n`, form feed, `\r`) is
/// trimmed first, and nothing else: `str::trim` and JavaScript's
/// `String.prototype.trim` disagree on U+0085 and U+FEFF, and the desktop's
/// TypeScript twin must produce the same digest for the same input. A
/// trailing U+FEFF therefore stays in the `dtag`; a trailing U+0085 is a
/// control character, so that coordinate has no digest.
///
/// `None` when `coordinate` is not a well-formed project coordinate — never a
/// digest of a guess.
pub fn project_agent_digest(coordinate: &str) -> Option<String> {
    let normalized = normalize_project_coordinate(trim_ascii_whitespace(coordinate))?;
    let mut hasher = Sha256::new();
    hasher.update(PROJECT_AGENT_DIGEST_DOMAIN.as_bytes());
    hasher.update(normalized.as_bytes());
    Some(hex::encode(hasher.finalize()))
}

/// `value` without surrounding ASCII whitespace — the one trim every reader of
/// a project coordinate applies, so Rust and TypeScript agree byte for byte.
pub fn trim_ascii_whitespace(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Vector {
        coordinate: String,
        digest: Option<String>,
    }

    /// The vectors the desktop's TypeScript twin is pinned to as well
    /// (`desktop/src/shared/lib/projectAgentAssociation.test.mjs`).
    #[test]
    fn digest_matches_the_shared_vectors() {
        let vectors: Vec<Vector> = serde_json::from_str(include_str!(
            "../testdata/project_agent_association/vectors.json"
        ))
        .expect("vectors parse");
        assert!(vectors.len() >= 4);
        for vector in vectors {
            assert_eq!(
                project_agent_digest(&vector.coordinate),
                vector.digest,
                "coordinate {:?}",
                vector.coordinate
            );
        }
    }

    #[test]
    fn owner_case_does_not_change_the_digest() {
        let lower = format!("30621:{}:tank-loop", "ab".repeat(32));
        let upper = format!("30621:{}:tank-loop", "AB".repeat(32));
        assert_eq!(project_agent_digest(&lower), project_agent_digest(&upper));
        assert!(project_agent_digest(&lower).is_some());
    }

    #[test]
    fn only_ascii_whitespace_is_trimmed() {
        let plain = format!("30621:{}:tank-loop", "ab".repeat(32));
        assert_eq!(
            project_agent_digest(&format!("\t{plain}\r\n")),
            project_agent_digest(&plain)
        );
        let bom = project_agent_digest(&format!("{plain}\u{feff}"));
        assert!(bom.is_some());
        assert_ne!(bom, project_agent_digest(&plain), "U+FEFF is not trimmed");
        assert_eq!(
            project_agent_digest(&format!("{plain}\u{85}")),
            None,
            "U+0085 is not trimmed, and a control character has no digest"
        );
    }

    #[test]
    fn a_non_project_coordinate_has_no_digest() {
        assert_eq!(project_agent_digest("30178:ab:tank-loop"), None);
        assert_eq!(project_agent_digest(""), None);
    }
}
