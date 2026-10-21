//! In-process WebAssembly component host for Beekeeper plugins.
//!
//! A plugin is a component built against `wit/plugin/world.wit`
//! (`beekeeper:plugin@1`). This crate loads it from a sha256-pinned
//! [`Registry`], refuses any import outside that package (so no WASI: no
//! clock, no filesystem, no network, no environment), checks the plugin's
//! manifest against the operator's grants, and runs each call in a fresh
//! `Store` under fuel, an epoch deadline and a memory limit. A trap, a panic,
//! running out of fuel or memory is an [`InvokeError`], never a crash of the
//! embedding process, and never leaks state into the next call.
//!
//! The crate knows nothing about relays, databases or Nostr signing. The
//! embedder supplies a [`RecordSource`] — the only way a plugin reads
//! anything — already bound to the caller and scope it authorized, and
//! [`DraftValidators`] for the kinds a plugin may draft (see [`drafts`]
//! for what a draft may say and what the host constructs for it). That keeps the trust
//! boundary in one place: everything a plugin returns is data the embedder
//! checks, and nothing a plugin returns is ever signed by the embedder.
//!
//! See `docs/plugins.md` for the contract and its limits.

pub mod drafts;
mod registry;
mod runtime;

pub use drafts::{DraftKind, DraftReferences, DraftValidator};
pub use registry::{valid_plugin_name, Grants, Limits, PluginEntry, Registry, SUPPORTED_WORLDS};
pub use runtime::{DraftValidators, LoadedPlugin, PluginHost};

use serde::{Deserialize, Serialize};

/// A stored record handed to a plugin: a signed event without its signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

impl StoredRecord {
    /// Bytes this record costs against an invocation's record budget.
    pub fn cost(&self) -> u64 {
        let tags: usize = self
            .tags
            .iter()
            .flat_map(|tag| tag.iter().map(String::len))
            .sum();
        (self.content.len() + tags + self.id.len() + self.pubkey.len()) as u64
    }
}

/// Why a [`RecordSource`] could not answer.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SourceError {
    #[error("{0}")]
    Unavailable(String),
    /// The plugin's request was malformed (a cursor this source never issued).
    #[error("{0}")]
    Invalid(String),
}

/// The embedder's scoped read: records of `kinds` in the scope the embedder
/// authorized, visible to the caller it authenticated, newest first, at most
/// `limit`, starting at `cursor` (a previous page's `next`) or else at
/// `until`. Returns the records and the next page's cursor, `None` only when
/// no older readable record can exist. The host has already checked `kinds`
/// against the plugin's grants and charged the call to the invocation's
/// budget before calling this.
pub trait RecordSource: Send + Sync {
    fn query(
        &self,
        kinds: &[u32],
        until: Option<u64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>), SourceError>;
}

/// What an operation acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "kebab-case")]
pub enum Scope {
    /// A canonical project coordinate.
    Project(String),
}

impl Scope {
    /// The `a` tag value a draft in this scope must carry.
    pub fn coordinate(&self) -> &str {
        match self {
            Scope::Project(coordinate) => coordinate,
        }
    }
}

/// What the embedder authorized the caller to do in the scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Read,
    Write,
}

/// One call.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub operation: String,
    pub input: serde_json::Value,
    /// 64 lowercase hex.
    pub caller: String,
    pub scope: Scope,
    pub access: Access,
    /// Unix seconds.
    pub now: u64,
    pub entropy: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpEffect {
    Read,
    Draft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationDecl {
    pub name: String,
    pub effect: OpEffect,
    pub summary: String,
}

/// What a plugin says about itself, checked at load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub api: Vec<String>,
    pub operations: Vec<OperationDecl>,
    pub read_kinds: Vec<u32>,
    pub draft_kinds: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IfRefused {
    Abort,
    SkipGroup,
}

/// An unsigned event for the caller to sign with their own key. Its tags
/// are the host's construction, not the plugin's proposal: the scope tag is
/// written from the invocation and the rest passed a per-kind allowlist
/// ([`drafts`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub group: String,
    pub if_refused: IfRefused,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_summary: Option<String>,
}

/// What a call returned, after the host checked it.
#[derive(Debug, Clone)]
pub struct InvokeOutcome {
    pub result: serde_json::Value,
    pub drafts: Vec<Draft>,
    /// Every person a returned draft names (an assignee), sorted. The
    /// embedder must decide each is a legitimate target in the invocation's
    /// scope before handing the drafts to anyone to sign.
    pub people: Vec<String>,
    /// Fuel the call consumed.
    pub fuel_used: u64,
    /// Messages the plugin logged, truncated and capped.
    pub logs: Vec<(LogLevel, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
}

/// How a plugin stopped without returning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrapKind {
    /// It used its whole fuel budget.
    Fuel,
    /// It ran past its wall-clock deadline.
    Deadline,
    /// It asked for more memory than its limit.
    Memory,
    /// Any other trap — a panic, `unreachable`, an out-of-bounds access.
    Trap,
}

/// An error the plugin itself returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "message", rename_all = "kebab-case")]
pub enum OpError {
    InvalidInput(String),
    NotFound(String),
    Conflict(String),
    Unsupported(String),
    /// A host capability refused the plugin and it passed that on.
    Host(String),
    Internal(String),
}

/// Why a call produced no checked output.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InvokeError {
    #[error("no plugin named {0:?} is registered on this relay")]
    NotRegistered(String),
    #[error("plugin {plugin:?} has no operation {operation:?}")]
    UnknownOperation { plugin: String, operation: String },
    #[error("operation {0:?} writes, and the caller may only read this scope")]
    WriteNotAllowed(String),
    #[error("operation input is {size} bytes; this plugin accepts at most {limit}")]
    InputTooLarge { size: usize, limit: usize },
    #[error("plugin {0:?} is at its concurrency limit; try again")]
    Busy(String),
    #[error("plugin stopped: {kind:?}: {detail}")]
    Trapped { kind: TrapKind, detail: String },
    #[error("plugin returned an error: {0:?}")]
    Operation(OpError),
    /// The plugin returned something the contract does not allow. The output
    /// is discarded whole.
    #[error("plugin broke its contract: {0}")]
    ContractViolation(String),
    #[error("plugin host failure: {0}")]
    Host(String),
}

/// Why a registry could not be loaded. Every one is fatal at startup: an
/// operator who registered a plugin gets it or is told why not.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LoadError {
    #[error("{0}")]
    Registry(String),
    #[error("plugin {plugin:?}: {message}")]
    Plugin { plugin: String, message: String },
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}
