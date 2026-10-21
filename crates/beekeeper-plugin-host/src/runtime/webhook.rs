//! The `webhook-plugin@1` world: loading, and the calls a delivery makes.
//!
//! HTTP is the embedder's; authentication is the host's. The embedder takes a
//! [`WebhookCall`] for an installation, asks [`WebhookCall::authenticate`]
//! what to verify (headers only), reads the body up to
//! [`WebhookCall::max_body_bytes`], hands the plan, the installation's secret,
//! the body and its receive time to [`WebhookCall::verify`] — the host
//! computes and compares the MAC — and only then calls
//! [`WebhookCall::project`] with the [`VerifiedDelivery`] that came back.
//!
//! A [`WebhookCall`] holds no slot. The relay-wide, per-plugin and
//! per-installation slots are taken around each plugin call (`authenticate`,
//! `project`) and freed when it returns — never across the body read or the
//! MAC check, so a slow unauthenticated sender cannot hold the relay-wide
//! slots operation providers share. Either call may be
//! [`InvokeError::Busy`].

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::Store;

use super::{
    check_imports, classify, new_store_with, truncate, Budget, CallState, Permit, PluginHost,
    MANIFEST_FUEL, MAX_LOGS, MAX_LOG_BYTES,
};
use crate::registry::{WebhookEntry, WEBHOOK_WORLD};
use crate::webhook::{
    check_auth_plan, check_headers, check_settings, seal_application_event, verify_delivery,
    AuthFailure, AuthPlan, Header, MacAlgorithm, ProposedEvent, VerifiedDelivery, VerifyError,
    WebhookManifest, WebhookOutcome, MAX_REASON_BYTES,
};
use crate::{sha256_hex, InvokeError, LogLevel};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/plugin",
        world: "webhook-plugin",
    });
}

use bindings::beekeeper::plugin::{log as wit_log, webhook_types as wit};
use bindings::{WebhookPlugin, WebhookPluginPre};

/// The interfaces a `webhook-plugin@1` component may import: its types and
/// `log`. Not `records` — a webhook plugin reads nothing but the delivery —
/// and not the operation-provider `types`.
const WEBHOOK_IMPORTS: &[&str] = &["beekeeper:plugin/webhook-types", "beekeeper:plugin/log"];

impl wit::Host for CallState {}

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

/// A webhook plugin that passed every load check.
pub struct LoadedWebhookPlugin {
    pub entry: WebhookEntry,
    pub manifest: WebhookManifest,
    pub sha256: String,
    pub component_bytes: usize,
    /// The body cap the host applies: the smaller of the registry's
    /// `max_body_bytes` and the manifest's request.
    pub max_body_bytes: usize,
    pre: WebhookPluginPre<CallState>,
    running: AtomicUsize,
    installations: Mutex<HashMap<String, usize>>,
}

/// One delivery to one installation of a webhook plugin. Every plugin call
/// for the delivery goes through it. It holds no slot itself: each plugin
/// call takes its slots and frees them when it returns.
pub struct WebhookCall<'a> {
    host: &'a PluginHost,
    plugin: &'a LoadedWebhookPlugin,
    installation: String,
}

/// The slots one plugin call holds while it runs: this installation, this
/// plugin, relay-wide.
struct Slots<'a> {
    _installation: InstallationPermit<'a>,
    _plugin: Permit<'a>,
    _relay: Permit<'a>,
}

struct InstallationPermit<'a> {
    map: &'a Mutex<HashMap<String, usize>>,
    id: String,
}

impl<'a> InstallationPermit<'a> {
    fn acquire(map: &'a Mutex<HashMap<String, usize>>, id: &str, max: usize) -> Option<Self> {
        let mut running = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = running.entry(id.to_owned()).or_insert(0);
        if *count >= max {
            return None;
        }
        *count += 1;
        Some(Self {
            map,
            id: id.to_owned(),
        })
    }
}

impl Drop for InstallationPermit<'_> {
    fn drop(&mut self) {
        let mut running = self
            .map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = running.get_mut(&self.id) {
            *count -= 1;
            if *count == 0 {
                running.remove(&self.id);
            }
        }
    }
}

fn manifest_from(raw: wit::WebhookManifest) -> WebhookManifest {
    WebhookManifest {
        name: raw.name,
        version: raw.version,
        type_prefix: raw.type_prefix,
        max_body_bytes: raw.max_body_bytes,
    }
}

fn check_manifest(manifest: &WebhookManifest, entry: &WebhookEntry) -> Result<(), String> {
    if manifest.name != entry.name {
        return Err(format!(
            "manifest names itself {:?}, but it is registered as {:?}",
            manifest.name, entry.name
        ));
    }
    if manifest.version.is_empty() || manifest.version.len() > 64 {
        return Err("manifest version must be 1-64 bytes".to_owned());
    }
    if manifest.type_prefix != entry.grants.type_prefix {
        return Err(format!(
            "requests event type prefix {:?}, but the registry grants {:?}",
            manifest.type_prefix, entry.grants.type_prefix
        ));
    }
    if manifest.max_body_bytes == 0 {
        return Err("manifest max-body-bytes must be at least 1".to_owned());
    }
    Ok(())
}

fn to_wit_headers(headers: &[Header]) -> Vec<wit::Header> {
    headers
        .iter()
        .map(|h| wit::Header {
            name: h.name.clone(),
            value: h.value.clone(),
        })
        .collect()
}

impl PluginHost {
    pub(super) fn load_webhook_entry(
        &self,
        entry: &WebhookEntry,
    ) -> Result<LoadedWebhookPlugin, String> {
        if self.application_event.is_none() {
            return Err(
                "is a webhook plugin, and this host has no application-event validator to check what it emits"
                    .to_owned(),
            );
        }
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
        check_imports(&self.engine, &component, WEBHOOK_WORLD, WEBHOOK_IMPORTS)?;
        let mut linker = Linker::<CallState>::new(&self.engine);
        WebhookPlugin::add_to_linker::<CallState, HasSelf<CallState>>(&mut linker, |s| s)
            .map_err(|error| format!("linker: {error}"))?;
        let pre = linker
            .instantiate_pre(&component)
            .map_err(|error| format!("cannot link: {error}"))?;
        let pre = WebhookPluginPre::new(pre)
            .map_err(|error| format!("does not export beekeeper:plugin/webhook@1: {error}"))?;
        let mut store = store_for(&self.engine, entry, MANIFEST_FUEL)
            .map_err(|error| format!("store: {error}"))?;
        let instance = pre
            .instantiate(&mut store)
            .map_err(|error| format!("cannot instantiate: {error}"))?;
        let raw = instance
            .beekeeper_plugin_webhook()
            .call_describe(&mut store)
            .map_err(|error| {
                let denied = store.data().limiter.denied;
                format!("describe() failed: {}", classify(&error, denied))
            })?;
        let manifest = manifest_from(raw);
        check_manifest(&manifest, entry)?;
        let max_body_bytes = entry
            .limits
            .max_body_bytes
            .min(manifest.max_body_bytes as usize);
        Ok(LoadedWebhookPlugin {
            entry: entry.clone(),
            manifest,
            sha256: digest,
            component_bytes: bytes.len(),
            max_body_bytes,
            pre,
            running: AtomicUsize::new(0),
            installations: Mutex::new(HashMap::new()),
        })
    }

    /// Every loaded webhook plugin, by name.
    pub fn webhook_plugins(&self) -> impl Iterator<Item = &LoadedWebhookPlugin> {
        self.webhooks.values().map(Arc::as_ref)
    }

    pub fn webhook_plugin(&self, name: &str) -> Option<&LoadedWebhookPlugin> {
        self.webhooks.get(name).map(Arc::as_ref)
    }

    /// A delivery to `installation` of `plugin`. Takes no slot: each plugin
    /// call does, for as long as it runs.
    pub fn webhook_call(
        &self,
        plugin: &str,
        installation: &str,
    ) -> Result<WebhookCall<'_>, InvokeError> {
        let loaded = self
            .webhooks
            .get(plugin)
            .map(Arc::as_ref)
            .ok_or_else(|| InvokeError::NotRegistered(plugin.to_owned()))?;
        Ok(WebhookCall {
            host: self,
            plugin: loaded,
            installation: installation.to_owned(),
        })
    }

    /// Ask `plugin` whether an installation's settings are acceptable.
    /// `Ok(Err(reason))` is the plugin's refusal.
    pub fn validate_webhook_settings(
        &self,
        plugin: &str,
        settings_json: &str,
    ) -> Result<Result<(), String>, InvokeError> {
        let loaded = self
            .webhooks
            .get(plugin)
            .map(Arc::as_ref)
            .ok_or_else(|| InvokeError::NotRegistered(plugin.to_owned()))?;
        if let Err(reason) = check_settings(settings_json) {
            return Ok(Err(reason));
        }
        let _plugin_permit = Permit::acquire(&loaded.running, loaded.entry.limits.max_concurrent)
            .ok_or_else(|| InvokeError::Busy(plugin.to_owned()))?;
        let _relay_permit = self.relay_permit(plugin)?;
        run(&self.engine, loaded, |instance, store| {
            instance
                .beekeeper_plugin_webhook()
                .call_validate_settings(store, settings_json)
        })
        .map(|result| result.map_err(|reason| truncate(&reason, MAX_REASON_BYTES)))
    }
}

fn store_for(
    engine: &wasmtime::Engine,
    entry: &WebhookEntry,
    fuel: u64,
) -> Result<Store<CallState>, wasmtime::Error> {
    new_store_with(
        engine,
        entry.limits.memory_bytes,
        entry.limits.deadline_ms,
        fuel,
        None,
        Vec::new(),
        Budget {
            queries_left: 0,
            records_left: 0,
            bytes_left: 0,
        },
    )
}

/// Run one export in a fresh instance under the plugin's limits.
fn run<T>(
    engine: &wasmtime::Engine,
    loaded: &LoadedWebhookPlugin,
    call: impl FnOnce(&WebhookPlugin, &mut Store<CallState>) -> wasmtime::Result<T>,
) -> Result<T, InvokeError> {
    let mut store = store_for(engine, &loaded.entry, loaded.entry.limits.fuel)
        .map_err(|error| InvokeError::Host(format!("store: {error}")))?;
    let result = loaded
        .pre
        .instantiate(&mut store)
        .and_then(|instance| call(&instance, &mut store));
    let state = store.into_data();
    for (level, message) in &state.logs {
        match level {
            LogLevel::Warn => tracing::warn!(plugin = %loaded.entry.name, "{message}"),
            LogLevel::Info => tracing::info!(plugin = %loaded.entry.name, "{message}"),
            LogLevel::Debug => tracing::debug!(plugin = %loaded.entry.name, "{message}"),
        }
    }
    result.map_err(|error| classify(&error, state.limiter.denied))
}

impl WebhookCall<'_> {
    /// The body cap for this plugin; the embedder reads at most this many
    /// bytes and refuses a longer body before calling the plugin.
    pub fn max_body_bytes(&self) -> usize {
        self.plugin.max_body_bytes
    }

    /// Take this call's slots — installation, plugin, then relay-wide, so a
    /// call refused at a narrower cap never holds a wider one — or
    /// [`InvokeError::Busy`]. The per-installation limit supplements, never
    /// replaces, the plugin's and the relay's.
    fn slots(&self) -> Result<Slots<'_>, InvokeError> {
        let name = &self.plugin.entry.name;
        let limits = &self.plugin.entry.limits;
        let installation = InstallationPermit::acquire(
            &self.plugin.installations,
            &self.installation,
            limits.max_concurrent_per_installation,
        )
        .ok_or_else(|| InvokeError::Busy(name.clone()))?;
        let plugin = Permit::acquire(&self.plugin.running, limits.max_concurrent)
            .ok_or_else(|| InvokeError::Busy(name.clone()))?;
        let relay = self.host.relay_permit(name)?;
        Ok(Slots {
            _installation: installation,
            _plugin: plugin,
            _relay: relay,
        })
    }

    /// Headers only: what the host must verify. `Ok(Err(..))` means the
    /// delivery is not authenticated — the plugin said so, or its plan failed
    /// the host's shape check. Holds this call's slots only while the plugin
    /// runs.
    pub fn authenticate(
        &self,
        headers: &[Header],
    ) -> Result<Result<AuthPlan, AuthFailure>, InvokeError> {
        if let Err(reason) = check_headers(headers) {
            return Ok(Err(AuthFailure::Malformed(reason)));
        }
        let wit_headers = to_wit_headers(headers);
        let raw = {
            let _slots = self.slots()?;
            run(&self.host.engine, self.plugin, |instance, store| {
                instance
                    .beekeeper_plugin_webhook()
                    .call_authenticate(store, &wit_headers)
            })?
        };
        let plan = match raw {
            Err(failure) => {
                let cap = |text: String| truncate(&text, MAX_LOG_BYTES);
                return Ok(Err(match failure {
                    wit::AuthFailure::MissingHeader(m) => AuthFailure::MissingHeader(cap(m)),
                    wit::AuthFailure::Malformed(m) => AuthFailure::Malformed(cap(m)),
                    wit::AuthFailure::Unsupported(m) => AuthFailure::Unsupported(cap(m)),
                }));
            }
            Ok(plan) => AuthPlan {
                algorithm: match plan.algorithm {
                    wit::MacAlgorithm::HmacSha256 => MacAlgorithm::HmacSha256,
                },
                signature: plan.signature,
                signed_prefix: plan.signed_prefix,
                timestamp: plan.timestamp,
                delivery_id: plan.delivery_id,
                vendor_event: plan.vendor_event,
            },
        };
        Ok(check_auth_plan(&plan)
            .map(|()| plan)
            .map_err(AuthFailure::Malformed))
    }

    /// The host's MAC check: `plan` (from [`WebhookCall::authenticate`]),
    /// the installation's webhook `secret` — the embedder's, never shown to
    /// the plugin — the raw `body` and the host's `received_at` (unix
    /// seconds, never 0). Checks the body cap, the plan's shape, the vendor
    /// timestamp's skew against `received_at`, and the MAC over
    /// `signed-prefix || body` in constant time. Runs no plugin code and
    /// takes no slot. The [`VerifiedDelivery`] it returns is the only way
    /// into [`WebhookCall::project`].
    pub fn verify(
        &self,
        plan: AuthPlan,
        secret: &[u8],
        body: Vec<u8>,
        received_at: u64,
    ) -> Result<VerifiedDelivery, VerifyError> {
        if body.len() > self.plugin.max_body_bytes {
            return Err(VerifyError::BodyTooLarge {
                size: body.len(),
                limit: self.plugin.max_body_bytes,
            });
        }
        verify_delivery(
            &self.plugin.entry.name,
            &self.installation,
            plan,
            secret,
            body,
            received_at,
        )
    }

    /// Project a delivery the host verified. The body, the vendor event
    /// handed to the plugin and the event's `created_at` all come from
    /// `verified`; `delivery` carries the installation's channel (the `h`
    /// scope), settings and the request headers. An accepted event is fully
    /// built and validated; anything the contract forbids is
    /// [`InvokeError::ContractViolation`] and nothing is returned to store.
    /// Holds this call's slots only while the plugin runs.
    pub fn project(
        &self,
        verified: VerifiedDelivery,
        delivery: Delivery<'_>,
    ) -> Result<WebhookOutcome, InvokeError> {
        if !verified.is_for(&self.plugin.entry.name, &self.installation) {
            return Err(InvokeError::Host(
                "the delivery was verified for another plugin or installation".to_owned(),
            ));
        }
        if verified.body().len() > self.plugin.max_body_bytes {
            return Err(InvokeError::InputTooLarge {
                size: verified.body().len(),
                limit: self.plugin.max_body_bytes,
            });
        }
        check_settings(delivery.settings_json)
            .and_then(|()| check_headers(delivery.headers))
            .map_err(|reason| InvokeError::Host(format!("delivery: {reason}")))?;
        let wit_headers = to_wit_headers(delivery.headers);
        let disposition = {
            let _slots = self.slots()?;
            run(&self.host.engine, self.plugin, |instance, store| {
                instance.beekeeper_plugin_webhook().call_project(
                    store,
                    delivery.settings_json,
                    verified.vendor_event(),
                    &wit_headers,
                    verified.body(),
                )
            })?
        };
        let kind = self.host.application_event.as_ref().ok_or_else(|| {
            InvokeError::Host("no application-event validator is configured".to_owned())
        })?;
        Ok(match disposition {
            wit::Disposition::Ignore(reason) => {
                WebhookOutcome::Ignore(truncate(&reason, MAX_REASON_BYTES))
            }
            wit::Disposition::Reject(reason) => {
                WebhookOutcome::Reject(truncate(&reason, MAX_REASON_BYTES))
            }
            wit::Disposition::Accept(event) => WebhookOutcome::Accept(
                seal_application_event(
                    kind,
                    &self.plugin.entry.grants.type_prefix,
                    delivery.channel,
                    verified.received_at(),
                    ProposedEvent {
                        event_type: event.event_type,
                        schema: event.schema,
                        data_json: event.data_json,
                        tags: event.tags,
                    },
                )
                .map_err(InvokeError::ContractViolation)?,
            ),
        })
    }
}

/// What the embedder knows about a delivery besides its verified body: the
/// installation's channel and settings, and the request headers. Nothing in
/// it is authenticated, so nothing in it names the event or its time.
#[derive(Debug, Clone, Copy)]
pub struct Delivery<'a> {
    /// The installation's channel id: the event's `h` scope.
    pub channel: &'a str,
    pub settings_json: &'a str,
    pub headers: &'a [Header],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installation_is_capped_and_its_slot_is_freed_on_drop() {
        let map = Mutex::new(HashMap::new());
        let first = InstallationPermit::acquire(&map, "inst-a", 2).expect("first");
        let second = InstallationPermit::acquire(&map, "inst-a", 2).expect("second");
        assert!(InstallationPermit::acquire(&map, "inst-a", 2).is_none());
        // Another installation is not affected.
        let other = InstallationPermit::acquire(&map, "inst-b", 2).expect("other");
        drop(first);
        let third = InstallationPermit::acquire(&map, "inst-a", 2).expect("freed");
        drop((second, third, other));
        assert!(map.lock().unwrap().is_empty(), "no stale counters");
    }
}
