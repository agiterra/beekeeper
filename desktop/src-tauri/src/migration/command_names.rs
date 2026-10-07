//! Built-in command names persisted before a binary rename.
//!
//! Two renames are covered, and an old record goes straight to the current
//! name: `sprout-*` → `buzz-*` (2025), and `buzz-*` → `beekeeper-*` (2026-10).
//! Only exact built-in names are rewritten; a custom command or an explicit
//! path is the user's own choice and is left alone. The `buzz-agent` runtime
//! *id* (a record's or persona's `runtime`) is not a command and is not
//! touched here.

use std::path::Path;

use super::patch_json_records;

/// The current harness (`acp_command`) for a retired built-in name.
fn current_acp_command(command: &str) -> Option<&'static str> {
    matches!(command, "sprout-acp" | "buzz-acp").then_some("beekeeper-acp")
}

/// The current agent binary (`agent_command`, `agent_command_override`) for a
/// retired built-in name.
fn current_agent_command(command: &str) -> Option<&'static str> {
    matches!(command, "sprout-agent" | "buzz-agent").then_some("beekeeper-agent")
}

/// The current `mcp_command` for a retired built-in name. The removed MCP
/// server has no successor except for the bundled agent, which uses the dev
/// MCP; anything else gets none.
fn current_mcp_command(command: &str, agent_command: &str) -> Option<&'static str> {
    match command {
        "sprout-dev-mcp" | "buzz-dev-mcp" => Some("beekeeper-dev-mcp"),
        "sprout-mcp" | "sprout-mcp-server" | "buzz-mcp-server" => {
            Some(if agent_command == "beekeeper-agent" {
                "beekeeper-dev-mcp"
            } else {
                ""
            })
        }
        _ => None,
    }
}

fn replace_command_field(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    replacement: &str,
) -> bool {
    let Some(current) = obj.get(field).and_then(|v| v.as_str()) else {
        return false;
    };
    if current == replacement {
        return false;
    }
    eprintln!(
        "beekeeper-desktop: command-rename-reconcile: {:?}: {field} {:?} → {:?}",
        obj.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
        current,
        replacement,
    );
    obj.insert(
        field.to_string(),
        serde_json::Value::String(replacement.to_string()),
    );
    true
}

fn field<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, name: &str) -> &'a str {
    obj.get(name).and_then(|v| v.as_str()).unwrap_or("")
}

/// Rewrite one managed-agent record's retired built-in command names.
pub(super) fn reconcile_record_command_names(
    obj: &mut serde_json::Map<String, serde_json::Value>,
) -> bool {
    let mut changed = false;

    if let Some(acp) = current_acp_command(field(obj, "acp_command")) {
        changed |= replace_command_field(obj, "acp_command", acp);
    }
    for name in ["agent_command", "agent_command_override"] {
        if let Some(agent) = current_agent_command(field(obj, name)) {
            changed |= replace_command_field(obj, name, agent);
        }
    }
    let agent_command = field(obj, "agent_command").to_string();
    if let Some(mcp) = current_mcp_command(field(obj, "mcp_command"), &agent_command) {
        changed |= replace_command_field(obj, "mcp_command", mcp);
    }

    changed
}

pub(super) fn reconcile_legacy_command_names_in_file(path: &Path) {
    patch_json_records(path, reconcile_record_command_names);
}
