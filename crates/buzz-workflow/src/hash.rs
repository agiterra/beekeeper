//! The definition hash: one function, run on the relay and on every host.
//!
//! The relay stores a workflow definition as canonical JSON and its SHA-256 in
//! `workflows.definition_hash`. A host that receives a kind:46013 request
//! recompiles the same entry from the project's own `beekeeper/actions.yml`
//! and compares hashes before it runs anything — so the two sides must hash
//! **exactly** the same bytes. Those bytes are `serde_json::to_string` of the
//! `serde_json::Value` parsed from the definition's canonical JSON (the relay's
//! `handle_workflow_def` chain), which is what [`hash_definition_value`]
//! reproduces. The relay injects a webhook secret into the value first for
//! webhook triggers; host steps never combine with webhook triggers in C1, so
//! a host recompiling an `actions.yml` entry hashes the plain value.

use sha2::{Digest, Sha256};

use crate::error::WorkflowError;
use crate::schema::WorkflowDef;

/// SHA-256 of the canonical JSON encoding of `value`.
pub fn hash_definition_value(value: &serde_json::Value) -> Result<Vec<u8>, WorkflowError> {
    let encoded = serde_json::to_string(value)
        .map_err(|error| WorkflowError::InvalidDefinition(format!("json serialize: {error}")))?;
    Ok(Sha256::digest(encoded.as_bytes()).to_vec())
}

/// The canonical JSON value of a definition, as the relay stores it.
pub fn definition_value(def: &WorkflowDef) -> Result<serde_json::Value, WorkflowError> {
    let json = serde_json::to_string(def)
        .map_err(|error| WorkflowError::InvalidDefinition(format!("json serialize: {error}")))?;
    serde_json::from_str(&json)
        .map_err(|error| WorkflowError::InvalidDefinition(format!("json parse: {error}")))
}

/// SHA-256 of a definition's canonical JSON, raw bytes.
pub fn definition_hash(def: &WorkflowDef) -> Result<Vec<u8>, WorkflowError> {
    hash_definition_value(&definition_value(def)?)
}

/// SHA-256 of a definition's canonical JSON, lowercase hex.
pub fn definition_hash_hex(def: &WorkflowDef) -> Result<String, WorkflowError> {
    definition_hash(def).map(hex::encode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_yaml;

    const YAML: &str = "name: nightly\nproject: '30621:1111111111111111111111111111111111111111111111111111111111111111:pulse'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"true\"]\n";

    #[test]
    fn hash_is_stable_across_yaml_and_stored_json() {
        let (def, canonical) = parse_yaml(YAML).expect("parse");
        let from_def = definition_hash_hex(&def).expect("hash");
        // The relay's chain: canonical JSON string → Value → string → sha256.
        let value: serde_json::Value = serde_json::from_str(&canonical).expect("value");
        let from_value = hex::encode(hash_definition_value(&value).expect("hash"));
        assert_eq!(from_def, from_value);
        // And a definition re-read from the stored JSON hashes identically.
        let reread: WorkflowDef = serde_json::from_value(value).expect("reread");
        assert_eq!(definition_hash_hex(&reread).expect("hash"), from_def);
        assert_eq!(from_def.len(), 64);
    }

    #[test]
    fn hash_changes_when_the_command_changes() {
        let (a, _) = parse_yaml(YAML).expect("parse");
        let (b, _) = parse_yaml(&YAML.replace("[\"true\"]", "[\"false\"]")).expect("parse");
        assert_ne!(
            definition_hash_hex(&a).expect("hash"),
            definition_hash_hex(&b).expect("hash")
        );
    }
}
