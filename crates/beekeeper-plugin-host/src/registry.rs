//! The plugin registry: which components a relay runs, pinned by digest, and
//! what each may touch.
//!
//! Registration is deployment configuration — a TOML file the operator owns
//! (named by `BEEKEEPER_PLUGINS_FILE`), the same for every community the
//! relay serves. First-party and third-party plugins are registered the same
//! way; there is no built-in list beside this file.
//!
//! ```toml
//! [[plugin]]
//! name   = "planning"
//! world  = "operation-provider@1"
//! path   = "planning.wasm"              # relative to this file
//! sha256 = "<64 hex>"                   # required; a mismatch refuses to start
//! [plugin.grants]
//! read_kinds  = [44248]
//! draft_kinds = [44248]
//! [plugin.limits]                       # every field optional
//! fuel = 2_000_000_000
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::LoadError;

/// The worlds this build can load. `webhook-plugin@1` is planned
/// (agents repo `plans/GITHUB_PLUGIN_SLICE_PLAN.md`) and would be a second
/// arm here, sharing the loader, the pin and the limits.
pub const SUPPORTED_WORLDS: &[&str] = &["operation-provider@1"];

/// Per-call limits. Every default is a ceiling a plugin may lower but the
/// registry may not raise past [`Limits::HARD_CEILINGS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// Linear memory one call may hold, summed over every memory its core
    /// instances create.
    pub memory_bytes: u64,
    /// Instructions a call may execute (wasmtime fuel).
    pub fuel: u64,
    /// Wall-clock backstop for the whole call — the host reads it makes
    /// included — in milliseconds.
    pub deadline_ms: u64,
    /// Calls of this plugin that may run at once; more are refused (busy).
    pub max_concurrent: usize,
    /// `records.query` calls per invocation.
    pub max_queries: u32,
    /// Records returned across one invocation's queries.
    pub max_records: u32,
    /// Bytes of record content and tags returned across one invocation.
    pub max_record_bytes: u64,
    /// Bytes of operation input.
    pub max_input_bytes: usize,
    /// Bytes of `result-json`.
    pub max_output_bytes: usize,
    /// Drafts one invocation may return.
    pub max_drafts: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 4_000_000_000,
            deadline_ms: 5_000,
            max_concurrent: 8,
            max_queries: 32,
            max_records: 20_000,
            max_record_bytes: 32 * 1024 * 1024,
            max_input_bytes: 64 * 1024,
            max_output_bytes: 4 * 1024 * 1024,
            max_drafts: 16,
        }
    }
}

impl Limits {
    /// What no registry entry may exceed.
    pub const HARD_CEILINGS: Limits = Limits {
        memory_bytes: 512 * 1024 * 1024,
        fuel: 50_000_000_000,
        deadline_ms: 15_000,
        max_concurrent: 64,
        max_queries: 256,
        max_records: 100_000,
        max_record_bytes: 256 * 1024 * 1024,
        max_input_bytes: 1024 * 1024,
        max_output_bytes: 16 * 1024 * 1024,
        max_drafts: 64,
    };

    fn check(&self, plugin: &str) -> Result<(), LoadError> {
        let c = &Self::HARD_CEILINGS;
        let over = |field: &str, value: u64, ceiling: u64, floor: u64| {
            if value < floor || value > ceiling {
                Err(LoadError::Registry(format!(
                    "plugin {plugin:?}: limits.{field} = {value} is outside {floor}..={ceiling}"
                )))
            } else {
                Ok(())
            }
        };
        over(
            "memory_bytes",
            self.memory_bytes,
            c.memory_bytes,
            1024 * 1024,
        )?;
        over("fuel", self.fuel, c.fuel, 1)?;
        over("deadline_ms", self.deadline_ms, c.deadline_ms, 10)?;
        over(
            "max_concurrent",
            self.max_concurrent as u64,
            c.max_concurrent as u64,
            1,
        )?;
        over(
            "max_queries",
            u64::from(self.max_queries),
            u64::from(c.max_queries),
            0,
        )?;
        over(
            "max_records",
            u64::from(self.max_records),
            u64::from(c.max_records),
            0,
        )?;
        over(
            "max_record_bytes",
            self.max_record_bytes,
            c.max_record_bytes,
            0,
        )?;
        over(
            "max_input_bytes",
            self.max_input_bytes as u64,
            c.max_input_bytes as u64,
            2,
        )?;
        over(
            "max_output_bytes",
            self.max_output_bytes as u64,
            c.max_output_bytes as u64,
            2,
        )?;
        over("max_drafts", self.max_drafts as u64, c.max_drafts as u64, 0)?;
        Ok(())
    }
}

/// What the operator lets a plugin touch. A plugin's manifest *asks*; a
/// request outside these grants refuses the plugin at load, it is never
/// silently narrowed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    #[serde(default)]
    pub read_kinds: Vec<u32>,
    #[serde(default)]
    pub draft_kinds: Vec<u32>,
}

/// One registered plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEntry {
    pub name: String,
    pub world: String,
    /// Resolved against the registry file's directory when relative.
    pub path: PathBuf,
    /// Lowercase hex sha256 of the component file.
    pub sha256: String,
    #[serde(default)]
    pub grants: Grants,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    #[serde(default)]
    plugin: Vec<PluginEntry>,
}

/// A parsed, checked registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub entries: Vec<PluginEntry>,
    /// The file it came from, for status reporting.
    pub source: Option<PathBuf>,
}

/// `true` for a name a URL path segment can carry unescaped:
/// `[a-z0-9-]{1,63}`, not starting or ending with `-`.
pub fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Registry {
    /// Read and check the registry at `path`.
    pub fn from_file(path: &Path) -> Result<Self, LoadError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            LoadError::Registry(format!(
                "cannot read plugin registry {}: {error}",
                path.display()
            ))
        })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut registry = Self::from_toml(&text, base)?;
        registry.source = Some(path.to_path_buf());
        Ok(registry)
    }

    /// Parse registry TOML, resolving relative plugin paths against `base`.
    pub fn from_toml(text: &str, base: &Path) -> Result<Self, LoadError> {
        let file: RegistryFile = toml::from_str(text)
            .map_err(|error| LoadError::Registry(format!("invalid plugin registry: {error}")))?;
        let mut seen = std::collections::BTreeSet::new();
        let mut entries = Vec::with_capacity(file.plugin.len());
        for mut entry in file.plugin {
            if !valid_plugin_name(&entry.name) {
                return Err(LoadError::Registry(format!(
                    "plugin name {:?} must be 1-63 of [a-z0-9-], not starting or ending with '-'",
                    entry.name
                )));
            }
            if !seen.insert(entry.name.clone()) {
                return Err(LoadError::Registry(format!(
                    "plugin {:?} is registered twice",
                    entry.name
                )));
            }
            if !SUPPORTED_WORLDS.contains(&entry.world.as_str()) {
                return Err(LoadError::Registry(format!(
                    "plugin {:?}: world {:?} is not supported by this build (supported: {})",
                    entry.name,
                    entry.world,
                    SUPPORTED_WORLDS.join(", ")
                )));
            }
            if entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(LoadError::Registry(format!(
                    "plugin {:?}: sha256 must be 64 lowercase hex characters",
                    entry.name
                )));
            }
            entry.limits.check(&entry.name)?;
            if entry.path.is_relative() {
                entry.path = base.join(&entry.path);
            }
            entries.push(entry);
        }
        Ok(Self {
            entries,
            source: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIN: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn entry(extra: &str) -> String {
        format!(
            "[[plugin]]\nname = \"planning\"\nworld = \"operation-provider@1\"\npath = \"p.wasm\"\nsha256 = \"{PIN}\"\n{extra}"
        )
    }

    #[test]
    fn parses_an_entry_and_resolves_its_path() {
        let registry = Registry::from_toml(
            &entry("[plugin.grants]\nread_kinds = [44248]\n"),
            Path::new("/srv/plugins"),
        )
        .expect("parses");
        let e = &registry.entries[0];
        assert_eq!(e.path, PathBuf::from("/srv/plugins/p.wasm"));
        assert_eq!(e.grants.read_kinds, vec![44248]);
        assert_eq!(e.limits, Limits::default());
    }

    #[test]
    fn refuses_a_duplicate_name() {
        let text = format!("{}{}", entry(""), entry(""));
        let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("registered twice"), "{error}");
    }

    #[test]
    fn refuses_an_unknown_world() {
        let text = entry("").replace("operation-provider@1", "webhook-plugin@1");
        let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("not supported"), "{error}");
    }

    #[test]
    fn refuses_a_bad_pin_and_a_bad_name() {
        let text = entry("").replace(PIN, "ABC");
        assert!(Registry::from_toml(&text, Path::new(".")).is_err());
        let text = entry("").replace("\"planning\"", "\"Planning!\"");
        assert!(Registry::from_toml(&text, Path::new(".")).is_err());
    }

    #[test]
    fn refuses_a_limit_past_the_ceiling_and_an_unknown_field() {
        let error = Registry::from_toml(
            &entry("[plugin.limits]\nmemory_bytes = 99999999999\n"),
            Path::new("."),
        )
        .unwrap_err();
        assert!(error.to_string().contains("memory_bytes"), "{error}");
        assert!(Registry::from_toml(&entry("surprise = 1\n"), Path::new(".")).is_err());
    }

    #[test]
    fn plugin_names_are_url_safe() {
        assert!(valid_plugin_name("planning"));
        assert!(valid_plugin_name("test-planning-fixture"));
        assert!(!valid_plugin_name("-x"));
        assert!(!valid_plugin_name("a/b"));
        assert!(!valid_plugin_name(""));
    }
}
