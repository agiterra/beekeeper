//! Loading components and running calls.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

use crate::drafts::{
    check_created_at, seal_tags, DraftKind, DraftReferences, MAX_DRAFT_CONTENT_BYTES,
};
use crate::registry::{PluginEntry, Registry};
use crate::{
    sha256_hex, Access, Draft, IfRefused, Invocation, InvokeError, InvokeOutcome, LoadError,
    LogLevel, Manifest, OpEffect, OpError, OperationDecl, RecordSource, Scope, SourceError,
    StoredRecord, TrapKind,
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/plugin",
        world: "operation-provider",
    });
}

use bindings::beekeeper::plugin::{log as wit_log, records as wit_records, types as wit};
use bindings::{OperationProvider, OperationProviderPre};

/// The interfaces a component may import. Anything else — every `wasi:*`
/// interface included — refuses the plugin at load.
const ALLOWED_IMPORTS: &[&str] = &[
    "beekeeper:plugin/types",
    "beekeeper:plugin/records",
    "beekeeper:plugin/log",
];

/// The largest page one `records.query` may ask for.
pub const MAX_QUERY_PAGE: u32 = 1_000;
/// The longest cursor a plugin may hand back.
const MAX_CURSOR_BYTES: usize = 256;
/// How often the epoch ticks; deadlines are whole ticks.
const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Fuel for `describe()`, which should do almost nothing.
const MANIFEST_FUEL: u64 = 200_000_000;
const MAX_LOGS: usize = 32;
const MAX_LOG_BYTES: usize = 1_024;

/// The kinds the embedder lets any plugin draft, each with its data-tag
/// allowlist and validator (`crate::drafts`).
#[derive(Clone, Default)]
pub struct DraftValidators(BTreeMap<u32, DraftKind>);

impl DraftValidators {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, kind: u32, draft_kind: DraftKind) -> Self {
        self.0.insert(kind, draft_kind);
        self
    }

    fn get(&self, kind: u32) -> Option<&DraftKind> {
        self.0.get(&kind)
    }
}

/// A plugin that passed every load check.
pub struct LoadedPlugin {
    pub entry: PluginEntry,
    pub manifest: Manifest,
    pub sha256: String,
    pub component_bytes: usize,
    pre: OperationProviderPre<CallState>,
    running: AtomicUsize,
}

impl LoadedPlugin {
    pub fn operation(&self, name: &str) -> Option<&OperationDecl> {
        self.manifest.operations.iter().find(|op| op.name == name)
    }
}

/// The loaded registry.
pub struct PluginHost {
    engine: Engine,
    plugins: BTreeMap<String, Arc<LoadedPlugin>>,
    validators: DraftValidators,
    ticker_stop: Arc<AtomicBool>,
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.ticker_stop.store(true, Ordering::Relaxed);
    }
}

/// Limits one call's resources. `memory_bytes` is the total across every
/// linear memory the call's instances hold — a component can contain several
/// core instances, each with its own memory — not a per-memory cap.
struct CallLimiter {
    memory_bytes: usize,
    /// Bytes of linear memory granted so far in this call.
    granted: usize,
    denied: bool,
}

/// Core instances, memories and tables one call may create. A packed Rust
/// component needs a handful of each; these are ceilings, not targets.
const MAX_INSTANCES: usize = 32;
const MAX_MEMORIES: usize = 8;
const MAX_TABLES: usize = 32;

impl ResourceLimiter for CallLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let total = self.granted.saturating_sub(current).saturating_add(desired);
        if total > self.memory_bytes {
            self.denied = true;
            return Ok(false);
        }
        self.granted = total;
        Ok(true)
    }

    fn instances(&self) -> usize {
        MAX_INSTANCES
    }

    fn memories(&self) -> usize {
        MAX_MEMORIES
    }

    fn tables(&self) -> usize {
        MAX_TABLES
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 100_000)
    }
}

struct Budget {
    queries_left: u32,
    records_left: u32,
    bytes_left: u64,
}

/// Per-call store data. A fresh one per call: nothing survives a call.
struct CallState {
    limiter: CallLimiter,
    source: Option<Arc<dyn RecordSource>>,
    read_kinds: Vec<u32>,
    budget: Budget,
    logs: Vec<(LogLevel, String)>,
}

impl wit::Host for CallState {}

impl wit_records::Host for CallState {
    fn query(&mut self, query: wit::RecordQuery) -> Result<wit::RecordPage, wit::HostError> {
        let Some(source) = self.source.clone() else {
            return Err(wit::HostError::Denied(
                "records are not readable while the host reads the manifest".to_owned(),
            ));
        };
        if query.kinds.is_empty() {
            return Err(wit::HostError::Invalid(
                "a query must name at least one kind".to_owned(),
            ));
        }
        if query.limit == 0 || query.limit > MAX_QUERY_PAGE {
            return Err(wit::HostError::Invalid(format!(
                "limit must be 1..={MAX_QUERY_PAGE}"
            )));
        }
        if let Some(kind) = query.kinds.iter().find(|k| !self.read_kinds.contains(k)) {
            return Err(wit::HostError::Denied(format!(
                "kind {kind} is not granted to this plugin"
            )));
        }
        if self.budget.queries_left == 0 {
            return Err(wit::HostError::BudgetExhausted(
                "this call has used all of its queries".to_owned(),
            ));
        }
        if self.budget.records_left == 0 {
            return Err(wit::HostError::BudgetExhausted(
                "this call has read all the records it may".to_owned(),
            ));
        }
        if query
            .cursor
            .as_ref()
            .is_some_and(|c| c.len() > MAX_CURSOR_BYTES)
        {
            return Err(wit::HostError::Invalid(format!(
                "a cursor is at most {MAX_CURSOR_BYTES} bytes"
            )));
        }
        self.budget.queries_left -= 1;
        let limit = query.limit.min(self.budget.records_left);
        let (records, next) = source
            .query(&query.kinds, query.until, query.cursor.as_deref(), limit)
            .map_err(|error| match error {
                SourceError::Invalid(message) => wit::HostError::Invalid(message),
                other => wit::HostError::Unavailable(other.to_string()),
            })?;
        if records.len() > limit as usize || records.iter().any(|r| !query.kinds.contains(&r.kind))
        {
            // The embedder broke its own promise; say so rather than pass
            // the plugin something it did not ask for.
            return Err(wit::HostError::Unavailable(
                "the host's record source answered outside the query".to_owned(),
            ));
        }
        let cost: u64 = records.iter().map(StoredRecord::cost).sum();
        if cost > self.budget.bytes_left {
            self.budget.bytes_left = 0;
            return Err(wit::HostError::BudgetExhausted(
                "this call has read all the bytes it may".to_owned(),
            ));
        }
        self.budget.bytes_left -= cost;
        self.budget.records_left -= records.len() as u32;
        Ok(wit::RecordPage {
            next,
            records: records
                .into_iter()
                .map(|r| wit::StoredRecord {
                    id: r.id,
                    pubkey: r.pubkey,
                    created_at: r.created_at,
                    kind: r.kind,
                    tags: r.tags,
                    content: r.content,
                })
                .collect(),
        })
    }
}

impl wit_log::Host for CallState {
    fn log(&mut self, level: wit_log::Level, message: String) {
        if self.logs.len() >= MAX_LOGS {
            return;
        }
        let level = match level {
            wit_log::Level::Debug => LogLevel::Debug,
            wit_log::Level::Info => LogLevel::Info,
            wit_log::Level::Warn => LogLevel::Warn,
        };
        self.logs.push((level, truncate(&message, MAX_LOG_BYTES)));
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn engine() -> Result<Engine, LoadError> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    config.wasm_component_model(true);
    Engine::new(&config).map_err(|error| LoadError::Registry(format!("wasm engine: {error}")))
}

fn spawn_ticker(engine: &Engine) -> Result<Arc<AtomicBool>, LoadError> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let weak = engine.weak();
    // Without the ticker no deadline would ever fire, so failing to start it
    // fails the load rather than running plugins unbounded in time.
    std::thread::Builder::new()
        .name("plugin-epoch".to_owned())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(EPOCH_TICK);
                match weak.upgrade() {
                    Some(engine) => engine.increment_epoch(),
                    None => return,
                }
            }
        })
        .map_err(|error| LoadError::Registry(format!("plugin deadline clock: {error}")))?;
    Ok(stop)
}

fn deadline_ticks(deadline_ms: u64) -> u64 {
    (deadline_ms / EPOCH_TICK.as_millis() as u64).max(1)
}

fn new_store(
    engine: &Engine,
    entry: &PluginEntry,
    source: Option<Arc<dyn RecordSource>>,
    read_kinds: Vec<u32>,
    fuel: u64,
) -> Result<Store<CallState>, wasmtime::Error> {
    let limits = &entry.limits;
    let mut store = Store::new(
        engine,
        CallState {
            limiter: CallLimiter {
                memory_bytes: limits.memory_bytes as usize,
                granted: 0,
                denied: false,
            },
            source,
            read_kinds,
            budget: Budget {
                queries_left: limits.max_queries,
                records_left: limits.max_records,
                bytes_left: limits.max_record_bytes,
            },
            logs: Vec::new(),
        },
    );
    store.limiter(|state| &mut state.limiter);
    store.set_fuel(fuel)?;
    store.set_epoch_deadline(deadline_ticks(limits.deadline_ms));
    Ok(store)
}

fn classify(error: &wasmtime::Error, memory_denied: bool) -> InvokeError {
    // `Memory` means a growth was refused during the call and it then
    // trapped — almost always the guest aborting on allocation failure, but a
    // guest that survived the refusal and trapped for another reason is
    // reported the same way.
    let kind = match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => TrapKind::Fuel,
        Some(Trap::Interrupt) => TrapKind::Deadline,
        _ if memory_denied => TrapKind::Memory,
        _ => TrapKind::Trap,
    };
    // The top-level message, not the guest backtrace: the backtrace names
    // guest symbols and is for the operator's log, not the caller.
    let detail = match error.downcast_ref::<Trap>() {
        Some(trap) => trap.to_string(),
        None => truncate(&error.to_string(), MAX_LOG_BYTES),
    };
    InvokeError::Trapped { kind, detail }
}

fn check_imports(engine: &Engine, component: &Component) -> Result<(), String> {
    for (name, _) in component.component_type().imports(engine) {
        let (base, version) = name.split_once('@').unwrap_or((name, ""));
        let major_one = version.split('.').next() == Some("1");
        if !ALLOWED_IMPORTS.contains(&base) || !major_one {
            return Err(format!(
                "imports {name:?}; a plugin may import only beekeeper:plugin@1 interfaces (no WASI)"
            ));
        }
    }
    Ok(())
}

fn ident_ok(text: &str, max: usize, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || extra.contains(&b)
        })
}

fn manifest_from(raw: wit::Manifest) -> Manifest {
    Manifest {
        name: raw.name,
        version: raw.version,
        api: raw.api,
        operations: raw
            .operations
            .into_iter()
            .map(|op| OperationDecl {
                name: op.name,
                effect: match op.effect {
                    wit::OpEffect::Read => OpEffect::Read,
                    wit::OpEffect::Draft => OpEffect::Draft,
                },
                summary: op.summary,
            })
            .collect(),
        read_kinds: raw.read_kinds,
        draft_kinds: raw.draft_kinds,
    }
}

fn check_manifest(
    manifest: &Manifest,
    entry: &PluginEntry,
    validators: &DraftValidators,
) -> Result<(), String> {
    if manifest.name != entry.name {
        return Err(format!(
            "manifest names itself {:?}, but it is registered as {:?}",
            manifest.name, entry.name
        ));
    }
    if manifest.version.is_empty() || manifest.version.len() > 64 {
        return Err("manifest version must be 1-64 bytes".to_owned());
    }
    if manifest.api.is_empty() || !manifest.api.iter().all(|a| ident_ok(a, 64, b"./")) {
        return Err("manifest api must list 1+ ids of [a-z0-9-./]".to_owned());
    }
    if manifest.operations.is_empty() || manifest.operations.len() > 64 {
        return Err("manifest must declare 1-64 operations".to_owned());
    }
    let mut names = std::collections::BTreeSet::new();
    for op in &manifest.operations {
        if !ident_ok(&op.name, 63, b"") || !names.insert(op.name.as_str()) {
            return Err(format!(
                "operation name {:?} is invalid or repeated",
                op.name
            ));
        }
        if op.summary.len() > 256 {
            return Err(format!("operation {:?} summary is over 256 bytes", op.name));
        }
    }
    if let Some(kind) = manifest
        .read_kinds
        .iter()
        .find(|k| !entry.grants.read_kinds.contains(k))
    {
        return Err(format!(
            "asks to read kind {kind}, which the registry does not grant it"
        ));
    }
    for kind in &manifest.draft_kinds {
        if !entry.grants.draft_kinds.contains(kind) {
            return Err(format!(
                "asks to draft kind {kind}, which the registry does not grant it"
            ));
        }
        if validators.get(*kind).is_none() {
            return Err(format!(
                "asks to draft kind {kind}, which this host has no validator for"
            ));
        }
    }
    Ok(())
}

/// A host with no plugins and nothing to compile, for state built before a
/// registry is read. Infallible: it never configures an engine that could
/// refuse, and it has no plugin a deadline would need to reach, so it runs
/// no ticker.
impl Default for PluginHost {
    fn default() -> Self {
        Self {
            engine: Engine::default(),
            plugins: BTreeMap::new(),
            validators: DraftValidators::new(),
            ticker_stop: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl PluginHost {
    /// A host with no plugins.
    pub fn empty() -> Result<Self, LoadError> {
        Self::load(&Registry::default(), DraftValidators::new())
    }

    /// Load every registry entry or fail naming the first that cannot load.
    pub fn load(registry: &Registry, validators: DraftValidators) -> Result<Self, LoadError> {
        let engine = engine()?;
        // No plugins, no ticker thread: an embedder that registered nothing
        // pays for nothing but an idle engine.
        let ticker_stop = if registry.entries.is_empty() {
            Arc::new(AtomicBool::new(true))
        } else {
            spawn_ticker(&engine)?
        };
        let mut host = Self {
            engine,
            plugins: BTreeMap::new(),
            validators,
            ticker_stop,
        };
        for entry in &registry.entries {
            let loaded = host
                .load_entry(entry)
                .map_err(|message| LoadError::Plugin {
                    plugin: entry.name.clone(),
                    message,
                })?;
            host.plugins.insert(entry.name.clone(), Arc::new(loaded));
        }
        Ok(host)
    }

    fn load_entry(&self, entry: &PluginEntry) -> Result<LoadedPlugin, String> {
        let bytes = std::fs::read(&entry.path)
            .map_err(|error| format!("cannot read {}: {error}", entry.path.display()))?;
        let digest = sha256_hex(&bytes);
        if digest != entry.sha256 {
            return Err(format!(
                "{} has sha256 {digest}, but the registry pins {}",
                entry.path.display(),
                entry.sha256
            ));
        }
        let component = Component::new(&self.engine, &bytes)
            .map_err(|error| format!("not a loadable component: {error}"))?;
        check_imports(&self.engine, &component)?;
        let mut linker = Linker::<CallState>::new(&self.engine);
        OperationProvider::add_to_linker::<CallState, HasSelf<CallState>>(&mut linker, |s| s)
            .map_err(|error| format!("linker: {error}"))?;
        let pre = linker
            .instantiate_pre(&component)
            .map_err(|error| format!("cannot link: {error}"))?;
        let pre = OperationProviderPre::new(pre)
            .map_err(|error| format!("does not export beekeeper:plugin/operations@1: {error}"))?;
        let mut store = new_store(&self.engine, entry, None, Vec::new(), MANIFEST_FUEL)
            .map_err(|error| format!("store: {error}"))?;
        let instance = pre
            .instantiate(&mut store)
            .map_err(|error| format!("cannot instantiate: {error}"))?;
        let raw = instance
            .beekeeper_plugin_operations()
            .call_describe(&mut store)
            .map_err(|error| {
                let denied = store.data().limiter.denied;
                format!("describe() failed: {}", classify(&error, denied))
            })?;
        let manifest = manifest_from(raw);
        check_manifest(&manifest, entry, &self.validators)?;
        Ok(LoadedPlugin {
            entry: entry.clone(),
            manifest,
            sha256: digest,
            component_bytes: bytes.len(),
            pre,
            running: AtomicUsize::new(0),
        })
    }

    /// Every loaded plugin, by name.
    pub fn plugins(&self) -> impl Iterator<Item = &LoadedPlugin> {
        self.plugins.values().map(Arc::as_ref)
    }

    pub fn get(&self, name: &str) -> Option<&LoadedPlugin> {
        self.plugins.get(name).map(Arc::as_ref)
    }

    /// The declaration of `operation` on `plugin`, for an embedder deciding
    /// what access to require before it calls [`PluginHost::invoke`].
    pub fn operation(&self, plugin: &str, operation: &str) -> Result<&OperationDecl, InvokeError> {
        let loaded = self
            .get(plugin)
            .ok_or_else(|| InvokeError::NotRegistered(plugin.to_owned()))?;
        loaded
            .operation(operation)
            .ok_or_else(|| InvokeError::UnknownOperation {
                plugin: plugin.to_owned(),
                operation: operation.to_owned(),
            })
    }

    /// Run one call. Blocking: an async embedder runs it on a blocking
    /// thread. `source` must already be bound to the caller and scope in
    /// `invocation`.
    pub fn invoke(
        &self,
        plugin: &str,
        invocation: Invocation,
        source: Arc<dyn RecordSource>,
    ) -> Result<InvokeOutcome, InvokeError> {
        let loaded = self
            .plugins
            .get(plugin)
            .ok_or_else(|| InvokeError::NotRegistered(plugin.to_owned()))?;
        let decl = self.operation(plugin, &invocation.operation)?.clone();
        if decl.effect == OpEffect::Draft && invocation.access != Access::Write {
            return Err(InvokeError::WriteNotAllowed(decl.name));
        }
        let input = serde_json::to_string(&invocation.input)
            .map_err(|error| InvokeError::Host(format!("input: {error}")))?;
        let limits = &loaded.entry.limits;
        if input.len() > limits.max_input_bytes {
            return Err(InvokeError::InputTooLarge {
                size: input.len(),
                limit: limits.max_input_bytes,
            });
        }
        let _permit = Permit::acquire(&loaded.running, limits.max_concurrent)
            .ok_or_else(|| InvokeError::Busy(plugin.to_owned()))?;

        let mut store = new_store(
            &self.engine,
            &loaded.entry,
            Some(source),
            loaded.manifest.read_kinds.clone(),
            limits.fuel,
        )
        .map_err(|error| InvokeError::Host(format!("store: {error}")))?;
        let call = wit::Invocation {
            operation: invocation.operation.clone(),
            input_json: input,
            caller: invocation.caller.clone(),
            scope: match &invocation.scope {
                Scope::Project(coordinate) => wit::Scope::Project(coordinate.clone()),
            },
            access: match invocation.access {
                Access::Read => wit::Access::Read,
                Access::Write => wit::Access::Write,
            },
            now: invocation.now,
            entropy: invocation.entropy.to_vec(),
        };
        let result = loaded.pre.instantiate(&mut store).and_then(|instance| {
            instance
                .beekeeper_plugin_operations()
                .call_invoke(&mut store, &call)
        });
        let fuel_used = limits.fuel.saturating_sub(store.get_fuel().unwrap_or(0));
        let state = store.into_data();
        let output = match result {
            Err(error) => return Err(classify(&error, state.limiter.denied)),
            Ok(Err(error)) => return Err(InvokeError::Operation(op_error(error))),
            Ok(Ok(output)) => output,
        };
        let (result, drafts, people) = self.check_output(loaded, &decl, &invocation, output)?;
        Ok(InvokeOutcome {
            result,
            drafts,
            people,
            fuel_used,
            logs: state.logs,
        })
    }

    fn check_output(
        &self,
        loaded: &LoadedPlugin,
        decl: &OperationDecl,
        invocation: &Invocation,
        output: wit::OpOutput,
    ) -> Result<(serde_json::Value, Vec<Draft>, Vec<String>), InvokeError> {
        let violation = |message: String| InvokeError::ContractViolation(message);
        let limits = &loaded.entry.limits;
        if output.result_json.len() > limits.max_output_bytes {
            return Err(violation(format!(
                "result is {} bytes; the limit is {}",
                output.result_json.len(),
                limits.max_output_bytes
            )));
        }
        let result: serde_json::Value = serde_json::from_str(&output.result_json)
            .map_err(|error| violation(format!("result is not JSON: {error}")))?;
        if decl.effect == OpEffect::Read && !output.drafts.is_empty() {
            return Err(violation(format!(
                "read operation {:?} returned drafts",
                decl.name
            )));
        }
        if output.drafts.len() > limits.max_drafts {
            return Err(violation(format!(
                "{} drafts; the limit is {}",
                output.drafts.len(),
                limits.max_drafts
            )));
        }
        let mut drafts = Vec::with_capacity(output.drafts.len());
        let mut people = std::collections::BTreeSet::new();
        for (index, raw) in output.drafts.into_iter().enumerate() {
            let mut draft = Draft {
                kind: raw.kind,
                created_at: raw.created_at,
                tags: raw.tags,
                content: raw.content,
                group: raw.group,
                if_refused: match raw.if_refused {
                    wit::IfRefused::Abort => IfRefused::Abort,
                    wit::IfRefused::SkipGroup => IfRefused::SkipGroup,
                },
                partial_summary: raw.partial_summary,
            };
            let references = self
                .check_draft(loaded, invocation, &mut draft)
                .map_err(|message| violation(format!("draft {index}: {message}")))?;
            people.extend(references.people);
            drafts.push(draft);
        }
        Ok((result, drafts, people.into_iter().collect()))
    }

    /// Check one draft and replace its tags with the sealed set: the host's
    /// scope tag plus the plugin's allowed data tags (`crate::drafts`).
    fn check_draft(
        &self,
        loaded: &LoadedPlugin,
        invocation: &Invocation,
        draft: &mut Draft,
    ) -> Result<DraftReferences, String> {
        if !loaded.manifest.draft_kinds.contains(&draft.kind) {
            return Err(format!("kind {} is not one this plugin drafts", draft.kind));
        }
        let draft_kind = self
            .validators
            .get(draft.kind)
            .ok_or_else(|| format!("this host has no validator for kind {}", draft.kind))?;
        check_created_at(draft.created_at, invocation.now)?;
        if draft.content.len() > MAX_DRAFT_CONTENT_BYTES {
            return Err(format!("content is over {MAX_DRAFT_CONTENT_BYTES} bytes"));
        }
        draft.tags = seal_tags(
            draft_kind.data_tags(),
            invocation.scope.coordinate(),
            &draft.tags,
        )?;
        if draft.group.is_empty() || draft.group.len() > 64 {
            return Err("group must be 1-64 bytes".to_owned());
        }
        if draft
            .partial_summary
            .as_ref()
            .is_some_and(|summary| summary.len() > 512)
        {
            return Err("partial_summary is over 512 bytes".to_owned());
        }
        draft_kind.validate(&draft.tags, &draft.content)
    }
}

fn op_error(error: wit::OpError) -> OpError {
    let cap = |text: String| truncate(&text, MAX_LOG_BYTES);
    match error {
        wit::OpError::InvalidInput(m) => OpError::InvalidInput(cap(m)),
        wit::OpError::NotFound(m) => OpError::NotFound(cap(m)),
        wit::OpError::Conflict(m) => OpError::Conflict(cap(m)),
        wit::OpError::Unsupported(m) => OpError::Unsupported(cap(m)),
        wit::OpError::Internal(m) => OpError::Internal(cap(m)),
        wit::OpError::Host(host) => OpError::Host(cap(match host {
            wit::HostError::Denied(m) => format!("denied: {m}"),
            wit::HostError::BudgetExhausted(m) => format!("budget exhausted: {m}"),
            wit::HostError::Unavailable(m) => format!("unavailable: {m}"),
            wit::HostError::Invalid(m) => format!("invalid: {m}"),
        })),
    }
}

struct Permit<'a>(&'a AtomicUsize);

impl<'a> Permit<'a> {
    fn acquire(counter: &'a AtomicUsize, max: usize) -> Option<Self> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                (running < max).then_some(running + 1)
            })
            .ok()
            .map(|_| Self(counter))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_limit_is_a_total_across_every_memory_of_a_call() {
        let mut limiter = CallLimiter {
            memory_bytes: 1_000_000,
            granted: 0,
            denied: false,
        };
        // Two memories of 600 kB each: the second would take the call past
        // its limit even though neither is over it alone.
        assert!(limiter.memory_growing(0, 600_000, None).unwrap());
        assert!(!limiter.memory_growing(0, 600_000, None).unwrap());
        assert!(limiter.denied);
        // Growing an existing memory counts only the difference.
        assert!(limiter.memory_growing(600_000, 900_000, None).unwrap());
        assert!(!limiter.memory_growing(900_000, 1_100_000, None).unwrap());
        assert_eq!(limiter.memories(), MAX_MEMORIES);
    }
}
