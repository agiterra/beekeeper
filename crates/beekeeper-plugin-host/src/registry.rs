//! The plugin registry: which components a relay runs, pinned by digest, and
//! what each may touch.
//!
//! Registration is deployment configuration — TOML the operator owns, the
//! same for every community the relay serves. A relay reads up to two files:
//!
//! - the **built-in** registry its package ships beside the first-party
//!   components (Planning), so they are on with nothing to configure;
//! - the **operator** registry, which *adds* plugins to the built-ins and may
//!   set `[host]`. It never has to repeat a built-in or copy its pin.
//!
//! Both are parsed the same way, each entry's relative `path` against its own
//! file's directory. A name registered in both is refused, never silently
//! shadowed: an operator who wants a different plugin under a built-in's name
//! disables the built-in first (`disable_builtins = ["planning"]`, top level of
//! the operator file), and naming a built-in that does not exist is refused
//! too. The relay-wide `[host]` comes from the operator file if it has one,
//! else the built-in file, else the defaults, and every limit that depends on
//! it is checked against the merged result. First-party and third-party
//! plugins are otherwise registered the same way.
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
//!
//! [[plugin]]
//! name   = "github"
//! world  = "webhook-plugin@1"
//! path   = "github.wasm"
//! sha256 = "<64 hex>"
//! [plugin.grants]
//! type_prefix = "com.github"            # required; the manifest must ask for exactly this
//! [plugin.limits]                       # every field optional
//! max_body_bytes = 2_097_152
//!
//! [host]                                # optional; relay-wide
//! max_concurrent_calls = 32
//! ```
//!
//! The `world` selects the shape of `grants` and `limits`: each world has its
//! own structs, each refusing unknown fields, so a field that means something
//! in one world is an error in another rather than silently ignored.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::LoadError;

/// The operation-provider world (Planning): a member invokes an operation and
/// signs the drafts it returns.
pub const OPERATION_PROVIDER_WORLD: &str = "operation-provider@1";
/// The inbound-webhook world: a vendor delivery is authenticated by the host
/// and projected by the plugin into one application event the relay signs.
pub const WEBHOOK_WORLD: &str = "webhook-plugin@1";

/// The worlds this build can load. Each has its own grants, limits, import
/// allowlist and loaded-plugin type; an entry naming any other world refuses
/// the registry.
pub const SUPPORTED_WORLDS: &[&str] = &[OPERATION_PROVIDER_WORLD, WEBHOOK_WORLD];

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
        let over = range_check(plugin);
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

/// Per-call limits for a `webhook-plugin@1` plugin. Every field optional;
/// none may pass [`WebhookLimits::HARD_CEILINGS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WebhookLimits {
    /// Linear memory one call may hold, summed over every memory.
    pub memory_bytes: u64,
    /// Instructions a call may execute (wasmtime fuel).
    pub fuel: u64,
    /// Wall-clock backstop per call, in milliseconds.
    pub deadline_ms: u64,
    /// Calls of this plugin, across every installation, that may run at once.
    pub max_concurrent: usize,
    /// Calls for one installation that may run at once — a supplement to
    /// `max_concurrent` and the relay-wide cap, so one noisy installation
    /// cannot take every slot of its plugin.
    pub max_concurrent_per_installation: usize,
    /// Bytes of raw request body. The host applies the smaller of this and
    /// the plugin manifest's own request.
    pub max_body_bytes: usize,
}

impl Default for WebhookLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 2_000_000_000,
            deadline_ms: 2_000,
            max_concurrent: 8,
            max_concurrent_per_installation: 2,
            max_body_bytes: 1024 * 1024,
        }
    }
}

impl WebhookLimits {
    /// What no registry entry may exceed.
    pub const HARD_CEILINGS: WebhookLimits = WebhookLimits {
        memory_bytes: 512 * 1024 * 1024,
        fuel: 50_000_000_000,
        deadline_ms: 15_000,
        max_concurrent: 64,
        max_concurrent_per_installation: 64,
        max_body_bytes: 25 * 1024 * 1024,
    };

    fn check(&self, plugin: &str) -> Result<(), LoadError> {
        let c = &Self::HARD_CEILINGS;
        let over = range_check(plugin);
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
            "max_concurrent_per_installation",
            self.max_concurrent_per_installation as u64,
            self.max_concurrent as u64,
            1,
        )?;
        over(
            "max_body_bytes",
            self.max_body_bytes as u64,
            c.max_body_bytes as u64,
            1,
        )?;
        Ok(())
    }
}

/// The range check every limit goes through: `floor..=ceiling` or a
/// registry error naming the field.
fn range_check(plugin: &str) -> impl Fn(&str, u64, u64, u64) -> Result<(), LoadError> + '_ {
    move |field: &str, value: u64, ceiling: u64, floor: u64| {
        if value < floor || value > ceiling {
            Err(LoadError::Registry(format!(
                "plugin {plugin:?}: limits.{field} = {value} is outside {floor}..={ceiling}"
            )))
        } else {
            Ok(())
        }
    }
}

/// What the operator lets a `webhook-plugin@1` plugin emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookGrants {
    /// Every application-event type the plugin emits starts with this plus
    /// `.`. Granted by the operator; the plugin's manifest must request
    /// exactly this or the plugin is refused at load. Two or more
    /// dot-separated labels of `[a-z0-9-]` (e.g. `com.github`).
    pub type_prefix: String,
}

/// `true` for a type prefix of 2+ labels of `[a-z0-9-]{1,63}`, not starting or
/// ending with `-`, at most 64 bytes, so `prefix.name` can be a valid
/// application-event type (3+ labels, at most 128 bytes).
pub fn valid_type_prefix(prefix: &str) -> bool {
    prefix.len() <= 64
        && prefix.split('.').count() >= 2
        && prefix.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
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

/// One registered `operation-provider@1` plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEntry {
    pub name: String,
    /// Always [`OPERATION_PROVIDER_WORLD`]; other worlds have their own entry
    /// type ([`WebhookEntry`]).
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

/// One registered `webhook-plugin@1` plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookEntry {
    pub name: String,
    /// Resolved against the registry file's directory when relative.
    pub path: PathBuf,
    /// Lowercase hex sha256 of the component file.
    pub sha256: String,
    pub grants: WebhookGrants,
    pub limits: WebhookLimits,
}

/// Relay-wide limits: across every plugin of every world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HostLimits {
    /// Plugin calls, of any plugin in any world, that may run at once. A
    /// call past it is refused as busy, whatever its plugin's own limit.
    pub max_concurrent_calls: usize,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_concurrent_calls: 32,
        }
    }
}

impl HostLimits {
    /// The most a registry may set.
    pub const MAX_CONCURRENT_CALLS_CEILING: usize = 256;
}

/// One `[[plugin]]` table before its world is known: the world decides which
/// structs its `grants` and `limits` must parse as.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    world: String,
    path: PathBuf,
    sha256: String,
    #[serde(default)]
    grants: Option<toml::Value>,
    #[serde(default)]
    limits: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    /// Built-ins this (operator) registry turns off. Must be at the top of the
    /// file, before any table.
    #[serde(default)]
    disable_builtins: Vec<String>,
    #[serde(default)]
    plugin: Vec<RawEntry>,
    #[serde(default)]
    host: Option<HostLimits>,
}

/// One file, parsed and checked entry by entry, before it is merged.
#[derive(Debug, Default)]
struct Layer {
    disable_builtins: Vec<String>,
    entries: Vec<PluginEntry>,
    webhooks: Vec<WebhookEntry>,
    host: Option<HostLimits>,
    source: Option<PathBuf>,
}

impl Layer {
    fn names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(|e| e.name.as_str())
            .chain(self.webhooks.iter().map(|w| w.name.as_str()))
    }

    fn label(&self, fallback: &str) -> String {
        self.source
            .as_ref()
            .map_or_else(|| fallback.to_owned(), |p| p.display().to_string())
    }
}

/// A parsed, checked registry: the built-ins (less any disabled) followed by
/// the operator's plugins.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    /// `operation-provider@1` plugins: built-ins first, each file in order.
    pub entries: Vec<PluginEntry>,
    /// `webhook-plugin@1` plugins: built-ins first, each file in order.
    pub webhooks: Vec<WebhookEntry>,
    /// Relay-wide limits (`[host]`) after merging.
    pub host: HostLimits,
    /// The files it came from, built-in first, for status reporting.
    pub sources: Vec<PathBuf>,
    /// The names that came from the built-in registry and are still enabled.
    pub builtins: Vec<String>,
}

/// Parse one world-specific table, naming the plugin, world and table when
/// it does not fit (an unknown field included).
fn world_table<T: serde::de::DeserializeOwned>(
    entry: &RawEntry,
    table: &str,
    value: Option<toml::Value>,
    default: Option<T>,
) -> Result<T, LoadError> {
    match value {
        Some(value) => value.try_into().map_err(|error| {
            LoadError::Registry(format!(
                "plugin {:?}: [plugin.{table}] is not valid for world {:?}: {error}",
                entry.name, entry.world
            ))
        }),
        None => default.ok_or_else(|| {
            LoadError::Registry(format!(
                "plugin {:?}: world {:?} requires [plugin.{table}]",
                entry.name, entry.world
            ))
        }),
    }
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

fn read_layer(path: &Path) -> Result<Layer, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        LoadError::Registry(format!(
            "cannot read plugin registry {}: {error}",
            path.display()
        ))
    })?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut layer = parse_layer(&text, base).map_err(|error| match error {
        LoadError::Registry(message) => {
            LoadError::Registry(format!("{}: {message}", path.display()))
        }
        other => other,
    })?;
    layer.source = Some(path.to_path_buf());
    Ok(layer)
}

/// Parse one registry file, resolving relative plugin paths against `base`.
/// Everything that depends on another file (duplicates across files,
/// `disable_builtins`, `[host]`) is checked by [`merge`].
fn parse_layer(text: &str, base: &Path) -> Result<Layer, LoadError> {
    let file: RegistryFile = toml::from_str(text)
        .map_err(|error| LoadError::Registry(format!("invalid plugin registry: {error}")))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::new();
    let mut webhooks = Vec::new();
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
        if entry.path.is_relative() {
            entry.path = base.join(&entry.path);
        }
        if entry.world == OPERATION_PROVIDER_WORLD {
            let grants = world_table(
                &entry,
                "grants",
                entry.grants.clone(),
                Some(Grants::default()),
            )?;
            let limits = world_table(
                &entry,
                "limits",
                entry.limits.clone(),
                Some(Limits::default()),
            )?;
            limits.check(&entry.name)?;
            entries.push(PluginEntry {
                name: entry.name,
                world: entry.world,
                path: entry.path,
                sha256: entry.sha256,
                grants,
                limits,
            });
        } else {
            // The only other supported world. The type prefix is the
            // operator's grant, so there is no default for it.
            let grants: WebhookGrants = world_table(&entry, "grants", entry.grants.clone(), None)?;
            if !valid_type_prefix(&grants.type_prefix) {
                return Err(LoadError::Registry(format!(
                    "plugin {:?}: grants.type_prefix {:?} must be 2+ dot-separated labels of [a-z0-9-], at most 64 bytes",
                    entry.name, grants.type_prefix
                )));
            }
            let limits: WebhookLimits = world_table(
                &entry,
                "limits",
                entry.limits.clone(),
                Some(WebhookLimits::default()),
            )?;
            limits.check(&entry.name)?;
            webhooks.push(WebhookEntry {
                name: entry.name,
                path: entry.path,
                sha256: entry.sha256,
                grants,
                limits,
            });
        }
    }
    Ok(Layer {
        disable_builtins: file.disable_builtins,
        entries,
        webhooks,
        host: file.host,
        source: None,
    })
}

/// Merge the built-in layer (less what the operator disables) with the
/// operator's, and check everything that depends on both.
fn merge(builtin: Option<Layer>, operator: Option<Layer>) -> Result<Registry, LoadError> {
    let mut builtin = builtin.unwrap_or_default();
    let operator = operator.unwrap_or_default();
    let builtin_label = builtin.label("the built-in registry");
    let operator_label = operator.label("the operator registry");
    if !builtin.disable_builtins.is_empty() {
        return Err(LoadError::Registry(format!(
            "{builtin_label}: disable_builtins belongs in the operator registry, not the built-in one"
        )));
    }
    for name in &operator.disable_builtins {
        if !builtin.names().any(|n| n == name) {
            let known: Vec<&str> = builtin.names().collect();
            return Err(LoadError::Registry(format!(
                "{operator_label}: disable_builtins names {name:?}, which is not a built-in plugin (built-ins: {})",
                if known.is_empty() {
                    "none".to_owned()
                } else {
                    known.join(", ")
                }
            )));
        }
    }
    let disabled = |name: &str| operator.disable_builtins.iter().any(|d| d == name);
    builtin.entries.retain(|e| !disabled(&e.name));
    builtin.webhooks.retain(|w| !disabled(&w.name));
    let builtins: Vec<String> = builtin.names().map(str::to_owned).collect();
    for name in operator.names() {
        if builtins.iter().any(|b| b == name) {
            return Err(LoadError::Registry(format!(
                "plugin {name:?} is a built-in ({builtin_label}) and is registered again in {operator_label}; \
                 remove it there to keep the built-in, or add `disable_builtins = [{name:?}]` at the top of that file to replace it"
            )));
        }
    }

    let host = operator
        .host
        .clone()
        .or_else(|| builtin.host.clone())
        .unwrap_or_default();
    if host.max_concurrent_calls == 0
        || host.max_concurrent_calls > HostLimits::MAX_CONCURRENT_CALLS_CEILING
    {
        return Err(LoadError::Registry(format!(
            "host.max_concurrent_calls = {} is outside 1..={}",
            host.max_concurrent_calls,
            HostLimits::MAX_CONCURRENT_CALLS_CEILING
        )));
    }
    let mut entries = builtin.entries;
    entries.extend(operator.entries);
    let mut webhooks = builtin.webhooks;
    webhooks.extend(operator.webhooks);
    // A per-installation limit the relay-wide cap could never grant would
    // be a limit that never applies — checked against the merged `[host]`,
    // so an operator lowering the cap cannot strand a built-in's limit.
    for webhook in &webhooks {
        if webhook.limits.max_concurrent_per_installation > host.max_concurrent_calls {
            return Err(LoadError::Registry(format!(
                "plugin {:?}: limits.max_concurrent_per_installation = {} is over host.max_concurrent_calls = {}",
                webhook.name,
                webhook.limits.max_concurrent_per_installation,
                host.max_concurrent_calls
            )));
        }
    }
    Ok(Registry {
        entries,
        webhooks,
        host,
        sources: builtin.source.into_iter().chain(operator.source).collect(),
        builtins,
    })
}

impl Registry {
    /// Read and check one registry file on its own, as an operator registry
    /// with no built-ins beside it.
    pub fn from_file(path: &Path) -> Result<Self, LoadError> {
        merge(None, Some(read_layer(path)?))
    }

    /// Parse one registry's TOML, resolving relative plugin paths against
    /// `base`, as an operator registry with no built-ins beside it.
    pub fn from_toml(text: &str, base: &Path) -> Result<Self, LoadError> {
        merge(None, Some(parse_layer(text, base)?))
    }

    /// The registry a relay runs: the built-in file, if any, plus the
    /// operator's, if any, merged (see the module docs). Both naming the
    /// same file is read once, as the built-in registry — a deployment that
    /// pointed its operator setting at the shipped registry keeps working.
    pub fn layered(builtin: Option<&Path>, operator: Option<&Path>) -> Result<Self, LoadError> {
        let same = match (builtin, operator) {
            (Some(b), Some(o)) => match (std::fs::canonicalize(b), std::fs::canonicalize(o)) {
                (Ok(b), Ok(o)) => b == o,
                _ => b == o,
            },
            _ => false,
        };
        let operator = if same { None } else { operator };
        merge(
            builtin.map(read_layer).transpose()?,
            operator.map(read_layer).transpose()?,
        )
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
        // Was `webhook-plugin@1` before that world existed; any name this
        // build does not know is still refused.
        let text = entry("").replace("operation-provider@1", "webhook-plugin@2");
        let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("not supported"), "{error}");
    }

    fn webhook(extra: &str) -> String {
        format!(
            "[[plugin]]\nname = \"github\"\nworld = \"webhook-plugin@1\"\npath = \"g.wasm\"\nsha256 = \"{PIN}\"\n{extra}"
        )
    }

    const PREFIX: &str = "[plugin.grants]\ntype_prefix = \"com.github\"\n";

    #[test]
    fn a_webhook_entry_parses_into_its_own_world() {
        let text = format!(
            "{}{}",
            entry(""),
            webhook(&format!(
                "{PREFIX}[plugin.limits]\nmax_body_bytes = 2097152\nmax_concurrent_per_installation = 3\n"
            ))
        );
        let registry = Registry::from_toml(&text, Path::new("/srv")).expect("parses");
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.webhooks.len(), 1);
        let w = &registry.webhooks[0];
        assert_eq!(w.path, PathBuf::from("/srv/g.wasm"));
        assert_eq!(w.grants.type_prefix, "com.github");
        assert_eq!(w.limits.max_body_bytes, 2_097_152);
        assert_eq!(w.limits.max_concurrent_per_installation, 3);
        assert_eq!(w.limits.fuel, WebhookLimits::default().fuel);
        assert_eq!(registry.host, HostLimits::default());
    }

    #[test]
    fn a_webhook_entry_needs_a_valid_type_prefix_grant() {
        let error = Registry::from_toml(&webhook(""), Path::new(".")).unwrap_err();
        assert!(
            error.to_string().contains("requires [plugin.grants]"),
            "{error}"
        );
        for bad in ["github", "com..github", "Com.github", "com.-x", ""] {
            let text = webhook(&format!("[plugin.grants]\ntype_prefix = \"{bad}\"\n"));
            let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
            assert!(error.to_string().contains("type_prefix"), "{bad}: {error}");
        }
    }

    #[test]
    fn each_worlds_tables_refuse_the_other_worlds_fields() {
        // An operation-provider field in a webhook entry, and back.
        for text in [
            webhook(&format!("{PREFIX}read_kinds = [44248]\n")),
            webhook(&format!("{PREFIX}[plugin.limits]\nmax_queries = 3\n")),
            entry("[plugin.grants]\ntype_prefix = \"com.github\"\n"),
            entry("[plugin.limits]\nmax_body_bytes = 10\n"),
        ] {
            let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
            assert!(
                error.to_string().contains("unknown field"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn webhook_limits_hold_their_ceilings() {
        let error = Registry::from_toml(
            &webhook(&format!(
                "{PREFIX}[plugin.limits]\nmax_body_bytes = 26214401\n"
            )),
            Path::new("."),
        )
        .unwrap_err();
        assert!(error.to_string().contains("max_body_bytes"), "{error}");
        // A per-installation limit above its plugin's cap would be a limit
        // that never applies.
        let error = Registry::from_toml(
            &webhook(&format!(
                "{PREFIX}[plugin.limits]\nmax_concurrent = 2\nmax_concurrent_per_installation = 3\n"
            )),
            Path::new("."),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("max_concurrent_per_installation"),
            "{error}"
        );
    }

    #[test]
    fn a_per_installation_limit_is_within_the_relay_wide_cap() {
        let limits = "[plugin.limits]\nmax_concurrent = 8\nmax_concurrent_per_installation = 4\n";
        let text = format!(
            "[host]\nmax_concurrent_calls = 3\n{}",
            webhook(&format!("{PREFIX}{limits}"))
        );
        let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("is over host.max_concurrent_calls = 3"),
            "{error}"
        );
        let text = format!(
            "[host]\nmax_concurrent_calls = 4\n{}",
            webhook(&format!("{PREFIX}{limits}"))
        );
        let registry = Registry::from_toml(&text, Path::new(".")).expect("equal is allowed");
        assert_eq!(
            registry.webhooks[0].limits.max_concurrent_per_installation,
            4
        );
    }

    #[test]
    fn a_name_is_unique_across_worlds() {
        let text = format!(
            "{}{}",
            entry(""),
            webhook(PREFIX).replace("\"github\"", "\"planning\"")
        );
        let error = Registry::from_toml(&text, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("registered twice"), "{error}");
    }

    #[test]
    fn the_relay_wide_cap_is_configurable_and_bounded() {
        let registry =
            Registry::from_toml("[host]\nmax_concurrent_calls = 4\n", Path::new(".")).unwrap();
        assert_eq!(registry.host.max_concurrent_calls, 4);
        for bad in ["0", "257"] {
            let text = format!("[host]\nmax_concurrent_calls = {bad}\n");
            assert!(Registry::from_toml(&text, Path::new(".")).is_err(), "{bad}");
        }
        assert!(Registry::from_toml("[host]\nsurprise = 1\n", Path::new(".")).is_err());
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

    /// Write `text` to `dir/name`, creating its directory, and return the path.
    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, text).expect("write");
        path
    }

    fn other(name: &str) -> String {
        entry("").replace("\"planning\"", &format!("{name:?}"))
    }

    #[test]
    fn an_operator_registry_adds_to_the_builtins_with_paths_from_its_own_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let builtin = write(dir.path(), "shipped/registry.toml", &entry(""));
        let operator = write(dir.path(), "operator/registry.toml", &other("acme"));
        let registry = Registry::layered(Some(&builtin), Some(&operator)).expect("merges");
        let names: Vec<_> = registry.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["planning", "acme"],
            "built-ins first, nothing removed"
        );
        assert_eq!(registry.entries[0].path, dir.path().join("shipped/p.wasm"));
        assert_eq!(registry.entries[1].path, dir.path().join("operator/p.wasm"));
        assert_eq!(registry.builtins, ["planning"]);
        assert_eq!(registry.sources, [builtin.clone(), operator]);
        // Either alone.
        let alone = Registry::layered(Some(&builtin), None).expect("built-in alone");
        assert_eq!(alone.entries.len(), 1);
        assert!(Registry::layered(None, None)
            .expect("none")
            .entries
            .is_empty());
    }

    #[test]
    fn a_name_in_both_registries_is_refused_unless_the_builtin_is_disabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let builtin = write(dir.path(), "b/registry.toml", &entry(""));
        let operator = write(dir.path(), "o/registry.toml", &entry(""));
        let error = Registry::layered(Some(&builtin), Some(&operator)).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("is a built-in"), "{message}");
        assert!(message.contains("disable_builtins"), "{message}");

        // Disabled: the operator's own `planning` replaces it.
        let operator = write(
            dir.path(),
            "o/registry.toml",
            &format!("disable_builtins = [\"planning\"]\n{}", entry("")),
        );
        let registry = Registry::layered(Some(&builtin), Some(&operator)).expect("replaced");
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.entries[0].path, dir.path().join("o/p.wasm"));
        assert!(registry.builtins.is_empty());

        // Disabled with nothing in its place: no Planning, on purpose.
        let operator = write(
            dir.path(),
            "o/registry.toml",
            "disable_builtins = [\"planning\"]\n",
        );
        let registry = Registry::layered(Some(&builtin), Some(&operator)).expect("disabled");
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn disabling_a_builtin_that_does_not_exist_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let builtin = write(dir.path(), "b/registry.toml", &entry(""));
        let operator = write(
            dir.path(),
            "o/registry.toml",
            "disable_builtins = [\"planing\"]\n",
        );
        let error = Registry::layered(Some(&builtin), Some(&operator)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not a built-in plugin (built-ins: planning)"),
            "{error}"
        );
        let error = Registry::layered(None, Some(&operator)).unwrap_err();
        assert!(error.to_string().contains("(built-ins: none)"), "{error}");
        // And it means nothing in the built-in file itself.
        let error = Registry::layered(Some(&operator), None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("belongs in the operator registry"),
            "{error}"
        );
    }

    #[test]
    fn the_operators_host_cap_applies_and_is_checked_against_the_merged_plugins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let limits = "[plugin.limits]\nmax_concurrent = 8\nmax_concurrent_per_installation = 4\n";
        let builtin = write(
            dir.path(),
            "b/registry.toml",
            &format!(
                "[host]\nmax_concurrent_calls = 16\n{}",
                webhook(&format!("{PREFIX}{limits}"))
            ),
        );
        let registry = Registry::layered(Some(&builtin), None).expect("built-in host");
        assert_eq!(registry.host.max_concurrent_calls, 16);
        // The operator changes only the cap — no pin copied, nothing removed.
        let operator = write(
            dir.path(),
            "o/registry.toml",
            "[host]\nmax_concurrent_calls = 6\n",
        );
        let registry = Registry::layered(Some(&builtin), Some(&operator)).expect("cap changed");
        assert_eq!(registry.host.max_concurrent_calls, 6);
        assert_eq!(registry.webhooks.len(), 1);
        // A cap below a built-in's per-installation limit is refused.
        let operator = write(
            dir.path(),
            "o/registry.toml",
            "[host]\nmax_concurrent_calls = 3\n",
        );
        let error = Registry::layered(Some(&builtin), Some(&operator)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("is over host.max_concurrent_calls = 3"),
            "{error}"
        );
        let operator = write(
            dir.path(),
            "o/registry.toml",
            "[host]\nmax_concurrent_calls = 0\n",
        );
        assert!(Registry::layered(Some(&builtin), Some(&operator)).is_err());
    }

    #[test]
    fn the_same_file_named_twice_is_read_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let builtin = write(dir.path(), "registry.toml", &entry(""));
        let roundabout = dir.path().join(".").join("registry.toml");
        let registry = Registry::layered(Some(&builtin), Some(&roundabout)).expect("one file");
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.sources, [builtin]);
    }

    #[test]
    fn a_file_error_names_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let operator = write(
            dir.path(),
            "o/registry.toml",
            &entry("").replace(PIN, "ABC"),
        );
        let error = Registry::layered(None, Some(&operator)).unwrap_err();
        assert!(error.to_string().contains("o/registry.toml"), "{error}");
        let error = Registry::layered(Some(&dir.path().join("missing.toml")), None).unwrap_err();
        assert!(
            error.to_string().contains("cannot read plugin registry"),
            "{error}"
        );
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
