//! The environment a project execution starts with, resolved by source.
//!
//! A bounded child does **not** inherit the host's environment. It receives
//! exactly the variables this module resolves, each tagged with where it came
//! from ([`EnvSource`]), in one documented precedence order:
//!
//! 1. **Baseline** — the platform, locale, toolchain, network/TLS, Git
//!    transport and the selected runtime's model-login variables, read by
//!    *name* from the host ([`BASELINE`], [`ModelAuth`]). Nothing else in the
//!    host environment survives: a value some shell exported for another
//!    project (`DATABASE_URL`, `PROJECT_A_ONLY`, a build flag) is not part of
//!    any baseline category and never reaches the child. Its *name* is
//!    recorded as excluded, never its value.
//! 2. **Runtime** — values the runtime descriptor or persona supplies (the CLI
//!    a runtime runs).
//! 3. **Project** — values this project's own authorized configuration
//!    declares for this execution (an action step's `env`, and the host
//!    variables it names in `env_from_host`). A declaration naming a
//!    scope-owned variable ([`SCOPE_OWNED`]) is refused, not dropped.
//! 4. **Scope** — values the execution scope owns: private temp and runtime
//!    state, staged Git configuration, scoped caches. Applied after the
//!    project, and a conflict with a project declaration is a refusal.
//! 5. **Identity** — the execution's own seat identity. Last, so no project
//!    setting can replace who the execution is. Its `PATH` is prepended to
//!    the effective `PATH`, never substituted for it.
//!
//! The host's credential [`crate::acp::EnvFence`] is applied to layers 1–3:
//! nothing in the host's own namespace is ever resolved from the baseline or
//! a project declaration. Only the identity layer may set fenced names,
//! because those are the execution's own credentials.
//!
//! Path-valued baseline variables are kept only when the path is usable from
//! inside the execution's boundary; `PATH` is filtered per component. A value
//! that points at something the child cannot read would only redirect it into
//! a failure — or, unbounded, into another project's files.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use crate::acp::EnvFence;

/// Where a resolved variable came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnvSource {
    /// Operating-system identity of the user and session (`HOME`, `USER`…).
    Platform,
    /// Locale and text encoding.
    Locale,
    /// `PATH`, filtered to directories usable inside the boundary.
    SearchPath,
    /// Toolchain homes (`CARGO_HOME`, `RUSTUP_HOME`).
    Toolchain,
    /// Proxies and certificate bundles.
    Network,
    /// SSH agent for Git transport.
    GitTransport,
    /// The selected runtime's existing model login or routing.
    ModelAuth,
    /// Supplied by the runtime descriptor or persona for this runtime.
    Runtime,
    /// Owned by the execution scope: private temp, runtime state, staged
    /// configuration, scoped caches.
    Scope,
    /// Declared by this project's authorized configuration.
    Project,
    /// A host variable this project's configuration names explicitly.
    ProjectFromHost,
    /// The execution's own identity.
    Identity,
}

impl EnvSource {
    /// Stable lowercase word for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Locale => "locale",
            Self::SearchPath => "search-path",
            Self::Toolchain => "toolchain",
            Self::Network => "network",
            Self::GitTransport => "git-transport",
            Self::ModelAuth => "model-auth",
            Self::Runtime => "runtime",
            Self::Scope => "scope",
            Self::Project => "project",
            Self::ProjectFromHost => "project-from-host",
            Self::Identity => "identity",
        }
    }
}

/// Whether a baseline variable is a path that must be usable in the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Value,
    Path,
}

/// The baseline, by name: every host variable a bounded child may inherit.
const BASELINE: &[(&str, EnvSource, Kind)] = &[
    ("HOME", EnvSource::Platform, Kind::Value),
    ("USER", EnvSource::Platform, Kind::Value),
    ("LOGNAME", EnvSource::Platform, Kind::Value),
    ("SHELL", EnvSource::Platform, Kind::Value),
    ("TERM", EnvSource::Platform, Kind::Value),
    ("TZ", EnvSource::Platform, Kind::Value),
    ("__CF_USER_TEXT_ENCODING", EnvSource::Platform, Kind::Value),
    ("LANG", EnvSource::Locale, Kind::Value),
    ("LANGUAGE", EnvSource::Locale, Kind::Value),
    ("LC_ALL", EnvSource::Locale, Kind::Value),
    ("LC_CTYPE", EnvSource::Locale, Kind::Value),
    ("LC_COLLATE", EnvSource::Locale, Kind::Value),
    ("LC_MESSAGES", EnvSource::Locale, Kind::Value),
    ("LC_MONETARY", EnvSource::Locale, Kind::Value),
    ("LC_NUMERIC", EnvSource::Locale, Kind::Value),
    ("LC_TIME", EnvSource::Locale, Kind::Value),
    ("CARGO_HOME", EnvSource::Toolchain, Kind::Path),
    ("RUSTUP_HOME", EnvSource::Toolchain, Kind::Path),
    ("HTTP_PROXY", EnvSource::Network, Kind::Value),
    ("HTTPS_PROXY", EnvSource::Network, Kind::Value),
    ("NO_PROXY", EnvSource::Network, Kind::Value),
    ("ALL_PROXY", EnvSource::Network, Kind::Value),
    ("http_proxy", EnvSource::Network, Kind::Value),
    ("https_proxy", EnvSource::Network, Kind::Value),
    ("no_proxy", EnvSource::Network, Kind::Value),
    ("all_proxy", EnvSource::Network, Kind::Value),
    ("SSL_CERT_FILE", EnvSource::Network, Kind::Path),
    ("SSL_CERT_DIR", EnvSource::Network, Kind::Path),
    ("NODE_EXTRA_CA_CERTS", EnvSource::Network, Kind::Path),
    ("REQUESTS_CA_BUNDLE", EnvSource::Network, Kind::Path),
    ("CURL_CA_BUNDLE", EnvSource::Network, Kind::Path),
    ("GIT_SSL_CAINFO", EnvSource::Network, Kind::Path),
    ("SSH_AUTH_SOCK", EnvSource::GitTransport, Kind::Path),
];

/// The existing model login a runtime may use from the host environment.
///
/// Only the selected runtime's names are inherited: a Claude execution does
/// not receive an OpenAI key and the other way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAuth {
    /// Claude Code: subscription (keychain, no variable), API key, gateway,
    /// Bedrock or Vertex routing.
    Claude,
    /// Codex: API key and gateway.
    Codex,
    /// No model-login variables.
    None,
}

impl ModelAuth {
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_CUSTOM_HEADERS",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_USE_VERTEX",
                "ANTHROPIC_VERTEX_PROJECT_ID",
                "CLOUD_ML_REGION",
                "AWS_REGION",
                "AWS_PROFILE",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN",
                "AWS_BEARER_TOKEN_BEDROCK",
            ],
            Self::Codex => &["OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"],
            Self::None => &[],
        }
    }
}

/// Names a project declaration may never set: the execution scope owns them.
///
/// Each redirects where the runtime keeps its home, history, memory,
/// configuration or Git administration, so letting a project choose them would
/// let it point the execution at another project's state.
pub const SCOPE_OWNED: &[&str] = &[
    "HOME",
    "TMPDIR",
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "CLAUDE_CODE_TMPDIR",
    "CLAUDE_CODE_PROJECT_DIR_NAME",
    "CODEX_HOME",
    "CODEX_SQLITE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_TEMPLATE_DIR",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "npm_config_cache",
    "NPM_CONFIG_CACHE",
];

fn is_scope_owned(name: &str) -> bool {
    SCOPE_OWNED.contains(&name)
        || name.starts_with("GIT_CONFIG_KEY_")
        || name.starts_with("GIT_CONFIG_VALUE_")
}

/// Why a declared variable was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvRefusal {
    /// The host's own credential namespace.
    Fenced(String),
    /// A name the execution scope owns.
    ScopeOwned(String),
    /// A scope value would override a project declaration of the same name.
    Conflict(String),
    /// A declared `PATH` names a directory the execution cannot use.
    PathOutsideScope(String),
}

impl std::fmt::Display for EnvRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fenced(name) => write!(f, "{name} is the host's own credential namespace"),
            Self::ScopeOwned(name) => write!(
                f,
                "{name} is owned by the execution scope and cannot be declared by a project"
            ),
            Self::Conflict(name) => {
                write!(
                    f,
                    "{name} was declared by the project and is also owned by the scope"
                )
            }
            Self::PathOutsideScope(component) => write!(
                f,
                "PATH names {component}, which is outside this execution's boundary"
            ),
        }
    }
}

impl std::error::Error for EnvRefusal {}

/// A resolved child environment with the source of every variable.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ResolvedEnv {
    vars: BTreeMap<String, (String, EnvSource)>,
    excluded: Vec<String>,
    path_components_dropped: Vec<String>,
}

// Values are never printed: only names and sources.
impl std::fmt::Debug for ResolvedEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedEnv")
            .field("provenance", &self.provenance())
            .field("excluded", &self.excluded)
            .field("path_components_dropped", &self.path_components_dropped)
            .finish()
    }
}

impl ResolvedEnv {
    /// Resolve the baseline from `ambient` (the host's own environment).
    ///
    /// `usable` answers whether a path is usable inside the execution's
    /// boundary; path-valued variables and `PATH` components failing it are
    /// dropped. `fence` removes the host's own namespace before anything is
    /// considered.
    pub fn baseline<I>(
        ambient: I,
        auth: ModelAuth,
        fence: &EnvFence,
        usable: &dyn Fn(&Path) -> bool,
    ) -> Self
    where
        I: IntoIterator<Item = (OsString, OsString)>,
    {
        let mut resolved = Self::default();
        for (name, value) in ambient {
            let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
                continue;
            };
            if fence.covers(name) {
                continue;
            }
            if name == "PATH" {
                let (kept, dropped) = filter_search_path(value, usable);
                resolved.path_components_dropped = dropped;
                if !kept.is_empty() {
                    resolved.insert(name, kept, EnvSource::SearchPath);
                }
                continue;
            }
            let entry = BASELINE
                .iter()
                .find(|(known, _, _)| *known == name)
                .map(|(_, source, kind)| (*source, *kind))
                .or_else(|| {
                    auth.names()
                        .contains(&name)
                        .then_some((EnvSource::ModelAuth, Kind::Value))
                });
            match entry {
                Some((source, Kind::Value)) => resolved.insert(name, value.to_owned(), source),
                Some((source, Kind::Path)) if usable(Path::new(value)) => {
                    resolved.insert(name, value.to_owned(), source);
                }
                _ => resolved.excluded.push(name.to_owned()),
            }
        }
        resolved.excluded.sort();
        resolved
    }

    fn insert(&mut self, name: &str, value: String, source: EnvSource) {
        self.vars.insert(name.to_owned(), (value, source));
    }

    /// A value the runtime descriptor or persona supplies for this runtime
    /// (e.g. `CLAUDE_CODE_EXECUTABLE`). Replaces the baseline; may itself be
    /// replaced by a project declaration unless the name is scope-owned.
    /// Fenced names are ignored.
    pub fn runtime(&mut self, name: &str, value: &str, fence: &EnvFence) {
        if !fence.covers(name) {
            self.insert(name, value.to_owned(), EnvSource::Runtime);
        }
    }

    /// A value this project's authorized configuration declares.
    ///
    /// A declared `PATH` is accepted when every component is an absolute
    /// directory usable inside the boundary (the project's own tools, a
    /// granted toolchain, the system); it replaces the baseline `PATH`, and
    /// the seat identity later puts its `bee` first.
    ///
    /// # Errors
    /// Refused — never silently dropped — when the name is the host's own
    /// namespace or one the execution scope owns ([`SCOPE_OWNED`]): a project
    /// may not redirect the runtime's home, history, temp, Git administration
    /// or configuration location. A `PATH` naming a directory outside the
    /// boundary is refused with that component.
    pub fn project(
        &mut self,
        name: &str,
        value: &str,
        source: EnvSource,
        fence: &EnvFence,
        usable: &dyn Fn(&Path) -> bool,
    ) -> Result<(), EnvRefusal> {
        if fence.covers(name) {
            return Err(EnvRefusal::Fenced(name.to_owned()));
        }
        if is_scope_owned(name) {
            return Err(EnvRefusal::ScopeOwned(name.to_owned()));
        }
        if name == "PATH" {
            let inside = |component: &str| {
                Path::new(component).is_absolute() && usable(Path::new(component))
            };
            if let Some(outside) = value
                .split(':')
                .find(|component| !component.is_empty() && !inside(component))
            {
                return Err(EnvRefusal::PathOutsideScope(outside.to_owned()));
            }
        }
        self.insert(name, value.to_owned(), source);
        Ok(())
    }

    /// A value the execution scope itself owns (private temp, runtime state,
    /// Git configuration location, dependency cache homes). Applied after
    /// every project declaration.
    ///
    /// # Errors
    /// [`EnvRefusal::Conflict`] when a project declaration already named it;
    /// the caller refuses the launch rather than pick a winner silently.
    pub fn scope(&mut self, name: &str, value: &str) -> Result<(), EnvRefusal> {
        if let Some((_, source)) = self.vars.get(name) {
            if matches!(source, EnvSource::Project | EnvSource::ProjectFromHost) {
                return Err(EnvRefusal::Conflict(name.to_owned()));
            }
        }
        self.insert(name, value.to_owned(), EnvSource::Scope);
        Ok(())
    }

    /// The execution's own identity, last. A `PATH` value is prepended
    /// ([`Self::prepend_path`]), never substituted.
    pub fn identity(&mut self, name: &str, value: &str, usable: &dyn Fn(&Path) -> bool) {
        if name == "PATH" {
            self.prepend_path(value, EnvSource::Identity, usable);
            return;
        }
        self.insert(name, value.to_owned(), EnvSource::Identity);
    }

    /// Put directories the host chose (a tool it resolved, the seat's own
    /// `bee`) first on `PATH`. Directories usable in the boundary and not
    /// already on the effective `PATH` go first; the effective `PATH` —
    /// baseline and project tool directories alike — follows unchanged.
    pub fn prepend_path(&mut self, value: &str, source: EnvSource, usable: &dyn Fn(&Path) -> bool) {
        let current = self.get("PATH").unwrap_or_default().to_owned();
        let existing: Vec<&str> = current.split(':').filter(|c| !c.is_empty()).collect();
        let mut composed: Vec<&str> = value
            .split(':')
            .filter(|c| {
                !c.is_empty()
                    && !existing.contains(c)
                    && Path::new(c).is_absolute()
                    && usable(Path::new(c))
            })
            .collect();
        composed.dedup();
        composed.extend(existing.iter().copied());
        let joined = composed.join(":");
        self.insert("PATH", joined, source);
    }

    /// Remove a variable a later layer decided the child must not hold.
    pub fn remove(&mut self, name: &str) {
        self.vars.remove(name);
    }

    /// The value the child will see, if any.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|(value, _)| value.as_str())
    }

    /// Names and sources only, in name order. Never values.
    #[must_use]
    pub fn provenance(&self) -> Vec<(String, EnvSource)> {
        self.vars
            .iter()
            .map(|(name, (_, source))| (name.clone(), *source))
            .collect()
    }

    /// Names of host variables deliberately not inherited.
    #[must_use]
    pub fn excluded(&self) -> &[String] {
        &self.excluded
    }

    /// Host `PATH` directories left out because nothing in them is usable
    /// inside the boundary — disclosed in diagnostics, never silently lost.
    #[must_use]
    pub fn path_components_dropped(&self) -> &[String] {
        &self.path_components_dropped
    }

    /// Every `(name, value)` the child receives.
    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars
            .iter()
            .map(|(name, (value, _))| (name.as_str(), value.as_str()))
    }

    /// Replace `cmd`'s environment with exactly this one.
    pub fn apply_to(&self, cmd: &mut tokio::process::Command) {
        cmd.env_clear();
        for (name, value) in self.vars() {
            cmd.env(name, value);
        }
    }

    /// [`Self::apply_to`] for a blocking [`std::process::Command`].
    pub fn apply_to_std(&self, cmd: &mut std::process::Command) {
        cmd.env_clear();
        for (name, value) in self.vars() {
            cmd.env(name, value);
        }
    }
}

/// Keep the `PATH` components usable in the boundary, in order, without
/// duplicates. Returns the kept value and the components dropped.
fn filter_search_path(value: &str, usable: &dyn Fn(&Path) -> bool) -> (String, Vec<String>) {
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = Vec::new();
    for component in value.split(':').filter(|c| !c.is_empty()) {
        let path = Path::new(component);
        if path.is_absolute() && usable(path) {
            if !kept.contains(&component) {
                kept.push(component);
            }
        } else {
            dropped.push(component.to_owned());
        }
    }
    (kept.join(":"), dropped)
}

#[cfg(test)]
#[path = "exec_env_tests.rs"]
mod tests;
