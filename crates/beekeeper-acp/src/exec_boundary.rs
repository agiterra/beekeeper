//! The operating-system boundary a project execution runs inside.
//!
//! A host that launches an adapter (or a project command) on behalf of one
//! project hands this module the exact filesystem rights that execution holds
//! — its own checkout, its role bundle, the runtime it needs — and gets back a
//! [`PreparedBoundary`] that wraps the child's argv. Everything not granted is
//! unreadable and unwritable to the child **and every process it starts**:
//! shells, interpreters, MCP servers, hooks, build scripts. The rights are
//! decided by the host before the child exists and live in a policy file the
//! child cannot reach, so nothing the child says, configures or is approved to
//! do can widen them.
//!
//! # Why a complete read policy
//!
//! The policy denies every file read and write under `/`, allows only
//! metadata (so `stat`, `getcwd` and path resolution keep working), and then
//! grants reads of the operating system's own read-only roots
//! ([`SYSTEM_READ_ROOTS`]) plus the host's explicit [`Grant`]s. A project that
//! lives outside the usual home layout is therefore not a hole: there is no
//! "region" to fall outside of. A newly created sibling directory, a symlink,
//! a relative `../` spelling, a hard link, a clone and a child process all
//! resolve to a path that was never granted. Measured 2026-09-26 on macOS 27.0
//! against Claude Code 2.1.283 through claude-agent-acp 0.70.0 and codex-acp
//! 1.6.2 — see the project-integrity evidence in the agents repository.
//!
//! # Why the policy file is content-addressed
//!
//! The policy is written once to `<dir>/<sha256 of its text>.sb` and never
//! rewritten. A prepared handle therefore names exactly the rules it was
//! verified against: another preparation — another project, another seat —
//! writes a different file, and cannot change what an earlier handle
//! launches under.
//!
//! # Why the host verifies it before model work
//!
//! [`prepare`] runs a probe under the exact policy file that must be able to
//! read a granted file and must be refused reading one host-owned canary and
//! appending to another in the policy's directory — after the same probe,
//! unconfined, has shown the operating system itself allows both, so the
//! refusal can only be the boundary's. A boundary that could not be installed or that did not
//! refuse the canary is an error, never a silent fallback: a caller that
//! promised enforcement refuses the launch instead of running unconfined.
//!
//! # Outbound network
//!
//! By default the boundary governs files only: outbound network is whatever
//! the operating system allows. A spec that names [`Egress::LoopbackProxy`]
//! also denies every `network-outbound` operation — TCP and UDP to any
//! address, DNS through the system resolver's socket, and every Unix-domain
//! socket connect — except TCP to that one loopback port, where an
//! allowlisting proxy the host supplies is expected to listen. The
//! self-test then also proves, against a host-owned listener on another
//! loopback port, that the denial is in force before any child starts.
//!
//! macOS is the only backend. Elsewhere [`prepare`] answers
//! [`BoundaryError::Unsupported`] and the caller must disclose that no
//! boundary is enforced; it must not describe the execution as protected.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The backend identifier recorded wherever an enforced boundary is disclosed.
pub const BACKEND_MACOS_SEATBELT: &str = "macos-seatbelt";

/// Version of the policy shape this module renders. Part of every policy
/// text, so a change to the rendering is a change to the digest.
pub const POLICY_VERSION: u32 = 4;

/// macOS's login-shell `PATH` composer, denied inside the boundary.
pub const PATH_HELPER: &str = "/usr/libexec/path_helper";

/// The launcher every bounded child is started through.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Operating-system roots every bounded process may read, and why.
///
/// These are the vendor's and the package manager's read-only installation
/// roots — the dynamic linker, frameworks, system certificates, developer
/// tools and Homebrew. None of them is a place Beekeeper or a person keeps
/// project data. A project whose working tree lies under one of them cannot be
/// bounded (its siblings would be readable through the same grant), and the
/// scope preparation refuses it rather than advertising a boundary that does
/// not hold there ([`overlaps_system_root`]).
pub const SYSTEM_READ_ROOTS: &[(&str, &str)] = &[
    ("/System", "operating system"),
    (
        "/usr",
        "system binaries, libraries, data and /usr/local toolchains",
    ),
    ("/bin", "system binaries"),
    ("/sbin", "system binaries"),
    (
        "/Library",
        "system frameworks, certificates and developer tools",
    ),
    (
        "/private/etc",
        "system configuration (resolver, certificates)",
    ),
    ("/private/var/db", "system databases (time zones, dyld)"),
    ("/private/var/select", "active developer-tools selection"),
    ("/opt/homebrew", "Homebrew-installed toolchains"),
    ("/Applications", "installed applications"),
];

/// Device nodes every process needs (`/dev/null`, terminals, `/dev/urandom`).
const DEVICE_ROOT: &str = "/dev";

/// What a grant lets the child do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Access {
    /// Read, list and execute; never create, modify or delete.
    ReadOnly,
    /// Everything read-only allows, plus create, modify and delete.
    ReadWrite,
    /// Refuse deleting or renaming the target, whatever an earlier grant
    /// allowed. For a link the runtime must write *through* but never
    /// replace.
    NoUnlink,
    /// Refuse every write to the target, whatever an earlier grant allowed.
    /// For one file inside an otherwise writable tree that must stay as the
    /// host left it.
    NoWrite,
}

impl Access {
    /// Stable lowercase word for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::ReadWrite => "read-write",
            Self::NoUnlink => "no-unlink",
            Self::NoWrite => "no-write",
        }
    }
}

/// Which paths a grant covers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GrantTarget {
    /// A directory and everything below it.
    Tree(PathBuf),
    /// Exactly one path (a file, a socket, or a directory entry itself).
    File(PathBuf),
    /// Files named `<name_prefix>…` directly in any directory under `root`,
    /// e.g. Git's `tmp_obj_XXXX` scratch files. Never a directory prefix.
    ScratchFiles {
        /// Directory the scratch files live under.
        root: PathBuf,
        /// The fixed start of every scratch file name.
        name_prefix: String,
    },
}

impl GrantTarget {
    /// The path the grant is anchored at.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Tree(path) | Self::File(path) => path,
            Self::ScratchFiles { root, .. } => root,
        }
    }

    fn covers(&self, candidate: &Path) -> bool {
        match self {
            Self::Tree(root) => candidate.starts_with(root),
            Self::File(path) => candidate == path,
            Self::ScratchFiles { root, name_prefix } => {
                candidate.starts_with(root)
                    && candidate
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(name_prefix))
            }
        }
    }
}

/// One right the host grants a bounded execution, with the reason it exists.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Grant {
    /// The paths covered.
    pub target: GrantTarget,
    /// What the child may do there.
    pub access: Access,
    /// Why this grant exists, in words a diagnostic can show.
    pub reason: String,
}

impl Grant {
    /// A directory tree.
    pub fn tree(path: impl Into<PathBuf>, access: Access, reason: impl Into<String>) -> Self {
        Self {
            target: GrantTarget::Tree(path.into()),
            access,
            reason: reason.into(),
        }
    }

    /// One exact path.
    pub fn file(path: impl Into<PathBuf>, access: Access, reason: impl Into<String>) -> Self {
        Self {
            target: GrantTarget::File(path.into()),
            access,
            reason: reason.into(),
        }
    }

    /// Scratch files named `name_prefix…` anywhere under `root`.
    pub fn scratch(
        root: impl Into<PathBuf>,
        name_prefix: impl Into<String>,
        access: Access,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            target: GrantTarget::ScratchFiles {
                root: root.into(),
                name_prefix: name_prefix.into(),
            },
            access,
            reason: reason.into(),
        }
    }
}

/// What outbound network a bounded execution may open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Egress {
    /// Outbound network is not governed by the boundary (the default, and
    /// the behaviour of every policy before version 4).
    #[default]
    Unrestricted,
    /// Every outbound network operation is denied except a TCP connect to
    /// this loopback address's port.
    ///
    /// Seatbelt can name a remote only as `localhost:<port>` (or `*`), so the
    /// rule admits that port on every loopback address — `127.0.0.1` and
    /// `::1` alike — not the one address given. Unix-domain socket connects
    /// are denied with no exception: nothing a bounded runtime is known to
    /// need reaches the network through one, and the system resolver's socket
    /// (`mDNSResponder`) is exactly the DNS path an allowlisting proxy exists
    /// to remove. A runtime that needs one fails loudly with `EPERM`; it is
    /// never silently granted.
    LoopbackProxy(SocketAddr),
}

impl Egress {
    /// A loopback proxy egress for `addr`.
    ///
    /// # Errors
    /// [`BoundaryError::InvalidSpec`] when `addr` is not a loopback address
    /// with a non-zero port: the policy could not confine egress to it.
    pub fn loopback_proxy(addr: SocketAddr) -> Result<Self, BoundaryError> {
        check_loopback(addr)?;
        Ok(Self::LoopbackProxy(addr))
    }

    /// The proxy address, when egress is confined to one.
    #[must_use]
    pub const fn proxy(self) -> Option<SocketAddr> {
        match self {
            Self::Unrestricted => None,
            Self::LoopbackProxy(addr) => Some(addr),
        }
    }
}

/// Everything [`prepare`] needs.
#[derive(Debug, Clone)]
pub struct BoundarySpec {
    /// The rights, in order.
    pub grants: Vec<Grant>,
    /// Host-owned directory the content-addressed policy is written into.
    /// Must lie outside every grant, so the child can neither read nor
    /// rewrite the rules it runs under.
    pub policy_dir: PathBuf,
    /// A granted, readable file the self-test must be able to read.
    pub probe_readable: PathBuf,
    /// What outbound network the execution may open.
    pub egress: Egress,
}

/// Why a boundary could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryError {
    /// This platform has no supported backend.
    Unsupported(String),
    /// A grant or the policy location is not acceptable.
    InvalidSpec(String),
    /// The policy could not be written.
    Io(String),
    /// The policy was written but the probe under it did not behave as the
    /// policy promises.
    SelfTestFailed(String),
}

impl BoundaryError {
    /// Stable machine-readable code for receipts and diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "boundary-unsupported",
            Self::InvalidSpec(_) => "boundary-invalid-scope",
            Self::Io(_) => "boundary-policy-unwritable",
            Self::SelfTestFailed(_) => "boundary-self-test-failed",
        }
    }
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(detail)
            | Self::InvalidSpec(detail)
            | Self::Io(detail)
            | Self::SelfTestFailed(detail) => write!(f, "{}: {detail}", self.code()),
        }
    }
}

impl std::error::Error for BoundaryError {}

/// A boundary whose policy is on disk and has passed its self-test.
///
/// Only [`prepare`] constructs one, so holding a `PreparedBoundary` is proof
/// the policy was verified on this host before the child it wraps exists.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedBoundary {
    policy_file: PathBuf,
    digest: String,
    grants: Vec<Grant>,
    egress: Egress,
}

// Grants name host-private paths; diagnostics get the digest and a count.
impl fmt::Debug for PreparedBoundary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedBoundary")
            .field("backend", &BACKEND_MACOS_SEATBELT)
            .field("digest", &self.digest)
            .field("grants", &self.grants.len())
            .field("egress", &self.egress)
            .finish()
    }
}

impl PreparedBoundary {
    /// The argv that starts `program args…` inside this boundary.
    #[must_use]
    pub fn wrap(&self, program: &str, args: &[String]) -> (String, Vec<String>) {
        let mut wrapped = Vec::with_capacity(args.len() + 3);
        wrapped.push("-f".to_owned());
        wrapped.push(self.policy_file.to_string_lossy().into_owned());
        wrapped.push(program.to_owned());
        wrapped.extend(args.iter().cloned());
        (SANDBOX_EXEC.to_owned(), wrapped)
    }

    /// SHA-256 (hex) of the rendered policy: what exactly is enforced.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The backend enforcing this boundary.
    #[must_use]
    pub const fn backend(&self) -> &'static str {
        BACKEND_MACOS_SEATBELT
    }

    /// The grants. Paths are host-local: never publish them.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// What outbound network this boundary permits.
    #[must_use]
    pub const fn egress(&self) -> Egress {
        self.egress
    }

    /// Whether `path` is readable under this boundary (granted or a system
    /// root). Used to decide which path-valued settings a child can use.
    #[must_use]
    pub fn permits_read(&self, path: &Path) -> bool {
        permits_read(&self.grants, path)
    }
}

/// Whether `path` is readable under `grants` plus the system roots.
#[must_use]
pub fn permits_read(grants: &[Grant], path: &Path) -> bool {
    is_under_system_root(path)
        || grants
            .iter()
            .filter(|grant| !matches!(grant.access, Access::NoUnlink | Access::NoWrite))
            .any(|grant| grant.target.covers(path))
}

/// Whether `path` lies under a [`SYSTEM_READ_ROOTS`] entry.
#[must_use]
pub fn is_under_system_root(path: &Path) -> bool {
    SYSTEM_READ_ROOTS
        .iter()
        .any(|(root, _)| path.starts_with(root))
}

/// Whether a project path overlaps a system read root: the root would expose
/// its siblings, so no boundary around it can hold.
#[must_use]
pub fn overlaps_system_root(path: &Path) -> Option<&'static str> {
    SYSTEM_READ_ROOTS
        .iter()
        .find(|(root, _)| path.starts_with(root) || Path::new(root).starts_with(path))
        .map(|(root, _)| *root)
}

/// Render the policy text for `grants`, with outbound network unrestricted.
///
/// # Errors
/// See [`render_policy_with`].
pub fn render_policy(grants: &[Grant]) -> Result<String, BoundaryError> {
    render_policy_with(grants, Egress::Unrestricted)
}

/// Render the policy text for `grants` and `egress`.
///
/// # Errors
/// [`BoundaryError::InvalidSpec`] when a grant path is relative, is `/`, or
/// contains a character the policy language cannot carry safely, or when the
/// egress proxy is not a loopback address with a port.
pub fn render_policy_with(grants: &[Grant], egress: Egress) -> Result<String, BoundaryError> {
    let mut out = String::new();
    out.push_str(&format!(
        ";; Beekeeper project execution boundary, policy v{POLICY_VERSION}\n"
    ));
    out.push_str("(version 1)\n(allow default)\n");
    out.push_str("(deny file-read* file-write* (subpath \"/\"))\n");
    out.push_str("(allow file-read-metadata (subpath \"/\"))\n");
    out.push_str("(allow file-read-data (literal \"/\"))\n");
    for (root, _) in SYSTEM_READ_ROOTS {
        out.push_str(&format!("(allow file-read* (subpath {}))\n", quote(root)?));
    }
    out.push_str(&format!(
        "(allow file-read* file-write* (subpath {}))\n",
        quote(DEVICE_ROOT)?
    ));
    // Denials last, so no earlier grant of the same operation outranks them.
    let (denials, allows): (Vec<&Grant>, Vec<&Grant>) = grants
        .iter()
        .partition(|grant| matches!(grant.access, Access::NoUnlink | Access::NoWrite));
    for grant in allows.into_iter().chain(denials) {
        let path = grant.target.path();
        if !path.is_absolute() {
            return Err(BoundaryError::InvalidSpec(format!(
                "grant for {} is not an absolute path",
                grant.reason
            )));
        }
        if path == Path::new("/") {
            return Err(BoundaryError::InvalidSpec(format!(
                "grant for {} names the filesystem root",
                grant.reason
            )));
        }
        let text = path.to_string_lossy();
        let filter = match &grant.target {
            GrantTarget::Tree(_) => format!("(subpath {})", quote(&text)?),
            GrantTarget::File(_) => format!("(literal {})", quote(&text)?),
            GrantTarget::ScratchFiles { name_prefix, .. } => {
                if name_prefix.is_empty() || name_prefix.contains('/') {
                    return Err(BoundaryError::InvalidSpec(format!(
                        "scratch grant for {} has no fixed file-name prefix",
                        grant.reason
                    )));
                }
                format!(
                    "(regex #\"^{}/(.*/)?{}[^/]*$\")",
                    regex_escape(&text)?,
                    regex_escape(name_prefix)?
                )
            }
        };
        let rule = match grant.access {
            Access::ReadOnly => "allow file-read*",
            Access::ReadWrite => "allow file-read* file-write*",
            Access::NoUnlink => "deny file-write-unlink",
            Access::NoWrite => "deny file-write*",
        };
        out.push_str(&format!("({rule} {filter})\n"));
    }
    // Delegation out of the boundary: LaunchServices starts an application
    // through launchd, outside this sandbox, and Apple Events script one that
    // already runs outside it. Either would read what this policy denies
    // (measured: a background-only app opened from the writable tree read a
    // denied file). Nothing a project build or test needs sends either.
    out.push_str("(deny lsopen)\n(deny appleevent-send)\n");
    // A login shell's `path_helper` reorders the prepared `PATH`, putting the
    // host's tool directory (and its `mktemp`) behind `/usr/bin`. The host
    // composes the system search path itself (see the session scope), so the
    // helper is not needed and its absence is silent to `/etc/zprofile`.
    out.push_str(&format!(
        "(deny file-read* process-exec (literal {}))\n",
        quote(PATH_HELPER)?
    ));
    if let Egress::LoopbackProxy(addr) = egress {
        check_loopback(addr)?;
        // Last, so nothing above outranks either rule. The deny covers TCP,
        // UDP (including DNS) and Unix-domain connects; the one allow is TCP
        // to the proxy's port on loopback (Seatbelt's `localhost`).
        out.push_str("(deny network-outbound)\n");
        out.push_str(&format!(
            "(allow network-outbound (remote tcp \"localhost:{}\"))\n",
            addr.port()
        ));
    }
    Ok(out)
}

fn check_loopback(addr: SocketAddr) -> Result<(), BoundaryError> {
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return Err(BoundaryError::InvalidSpec(format!(
            "egress proxy {addr} is not a loopback address with a port"
        )));
    }
    Ok(())
}

fn check_policy_text(text: &str) -> Result<(), BoundaryError> {
    if text
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        return Err(BoundaryError::InvalidSpec(format!(
            "path {text:?} contains a character the boundary policy cannot express"
        )));
    }
    Ok(())
}

fn quote(text: &str) -> Result<String, BoundaryError> {
    check_policy_text(text)?;
    Ok(format!("\"{text}\""))
}

fn regex_escape(text: &str) -> Result<String, BoundaryError> {
    check_policy_text(text)?;
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if ".^$*+?()[]{}|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    Ok(out)
}

/// Write (once), verify and return the boundary for `spec`.
///
/// # Errors
/// See [`BoundaryError`]. Every error means the child must not be started as
/// if it were bounded.
pub fn prepare(spec: BoundarySpec) -> Result<PreparedBoundary, BoundaryError> {
    if !cfg!(target_os = "macos") {
        return Err(BoundaryError::Unsupported(format!(
            "no project execution boundary backend exists for {}",
            std::env::consts::OS
        )));
    }
    if !Path::new(SANDBOX_EXEC).exists() {
        return Err(BoundaryError::Unsupported(format!(
            "{SANDBOX_EXEC} is not present on this Mac"
        )));
    }
    let policy_dir = &spec.policy_dir;
    if !policy_dir.is_absolute() {
        return Err(BoundaryError::InvalidSpec(
            "the policy directory is not absolute".to_owned(),
        ));
    }
    for grant in &spec.grants {
        // A tree or exact path at or above the policy directory would expose
        // it; a scratch-file pattern reaches only files with its own name
        // prefix, which the self-test's canary and policy names never carry
        // (and the self-test checks regardless).
        let reaches_policy = match &grant.target {
            GrantTarget::Tree(_) | GrantTarget::File(_) => {
                grant.target.covers(policy_dir) || policy_dir.starts_with(grant.target.path())
            }
            GrantTarget::ScratchFiles { .. } => grant.target.covers(policy_dir),
        };
        if !matches!(grant.access, Access::NoUnlink | Access::NoWrite) && reaches_policy {
            return Err(BoundaryError::InvalidSpec(format!(
                "the boundary policy would lie inside a granted path ({})",
                grant.reason
            )));
        }
    }
    let rendered = render_policy_with(&spec.grants, spec.egress)?;
    let digest = hex::encode(Sha256::digest(rendered.as_bytes()));
    let policy_file = policy_dir.join(format!("{digest}.sb"));
    install_once(policy_dir, &policy_file, &rendered)?;
    let canary = policy_dir.join("boundary-canary");
    install_canary(&canary, CANARY_TEXT)?;
    let write_canary = policy_dir.join("boundary-write-canary");
    install_canary(&write_canary, "")?;
    self_test(&policy_file, &spec.probe_readable, &canary, &write_canary)?;
    if let Egress::LoopbackProxy(proxy) = spec.egress {
        egress_self_test(&policy_file, proxy)?;
    }
    Ok(PreparedBoundary {
        policy_file,
        digest,
        grants: spec.grants,
        egress: spec.egress,
    })
}

const CANARY_TEXT: &str = "boundary canary: must never be readable\n";

/// Create `file` with `text` unless it already exists with exactly `text`.
/// The policy is left read-only for its owner (0400); what keeps the child
/// from rewriting it is the boundary, which the self-test's writable canary
/// in the same directory proves.
///
/// The final name is claimed with a hard link from a private temporary, which
/// fails if the name exists, so two preparations racing on one digest both end
/// with the same bytes and neither ever replaces a file another verified.
fn install_once(dir: &Path, file: &Path, text: &str) -> Result<(), BoundaryError> {
    let io = |what: &str, error: std::io::Error| {
        BoundaryError::Io(format!("could not {what} {}: {error}", file.display()))
    };
    std::fs::create_dir_all(dir).map_err(|error| io("create the directory for", error))?;
    if !file.exists() {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = dir.join(format!(
            ".install.{}.{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&temp, text).map_err(|error| io("write", error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o400))
                .map_err(|error| io("restrict", error))?;
        }
        let linked = std::fs::hard_link(&temp, file);
        let _ = std::fs::remove_file(&temp);
        match linked {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io("install", error)),
        }
    }
    let existing = std::fs::read_to_string(file).map_err(|error| io("read back", error))?;
    if existing != text {
        return Err(BoundaryError::InvalidSpec(format!(
            "{} exists with different content; a content-addressed policy is never rewritten",
            file.display()
        )));
    }
    Ok(())
}

/// A host-owned canary that the operating system itself would let this
/// process read and append to (mode 0600, owned by the host user), so a
/// refusal under the policy can only come from the boundary.
fn install_canary(file: &Path, text: &str) -> Result<(), BoundaryError> {
    let io = |what: &str, error: std::io::Error| {
        BoundaryError::Io(format!("could not {what} {}: {error}", file.display()))
    };
    std::fs::write(file, text).map_err(|error| io("write", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| io("restrict", error))?;
    }
    Ok(())
}

/// The probe: read a granted file, read the canary, append to the write
/// canary. Unconfined, all three must succeed — the positive control proving
/// the files are ordinarily accessible. Under the policy, the first must
/// succeed and the other two must be refused.
const PROBE: &str = r#"cat "$1" >/dev/null 2>&1 || exit 11
if cat "$2" >/dev/null 2>&1; then R=1; else R=0; fi
if printf '' >> "$3" 2>/dev/null; then W=1; else W=0; fi
case "$4" in
  control) [ "$R$W" = 11 ] || exit 14 ;;
  bounded) [ "$R" = 0 ] || exit 12; [ "$W" = 0 ] || exit 13 ;;
esac
exit 0"#;

fn run_probe(
    policy: Option<&Path>,
    readable: &Path,
    canary: &Path,
    write_canary: &Path,
) -> Result<std::process::Output, BoundaryError> {
    let mut command = match policy {
        Some(policy) => {
            let mut command = std::process::Command::new(SANDBOX_EXEC);
            command.arg("-f").arg(policy).arg("/bin/sh");
            command
        }
        None => std::process::Command::new("/bin/sh"),
    };
    command
        .arg("-c")
        .arg(PROBE)
        .arg("sh")
        .arg(readable)
        .arg(canary)
        .arg(write_canary)
        .arg(if policy.is_some() {
            "bounded"
        } else {
            "control"
        })
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            BoundaryError::SelfTestFailed(format!("could not start the boundary probe: {error}"))
        })
}

/// Verify the policy: the unconfined control first, then the bounded probe.
fn self_test(
    policy: &Path,
    readable: &Path,
    canary: &Path,
    write_canary: &Path,
) -> Result<(), BoundaryError> {
    let control = run_probe(None, readable, canary, write_canary)?;
    if !control.status.success() {
        return Err(BoundaryError::SelfTestFailed(
            "the boundary probe's files are not accessible even without the boundary, so a \
             refusal under it would prove nothing"
                .to_owned(),
        ));
    }
    let output = run_probe(Some(policy), readable, canary, write_canary)?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(11) => Err(BoundaryError::SelfTestFailed(
            "a granted file was not readable under the policy".to_owned(),
        )),
        Some(12) => Err(BoundaryError::SelfTestFailed(
            "a host-owned canary outside every grant was readable".to_owned(),
        )),
        Some(13) => Err(BoundaryError::SelfTestFailed(
            "a host-owned file outside every grant was writable from inside the boundary"
                .to_owned(),
        )),
        other => Err(BoundaryError::SelfTestFailed(format!(
            "the boundary could not be applied (exit {other:?}): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

/// One TCP connect from a shell, to `127.0.0.1:<port>`. Bash's `/dev/tcp`,
/// so no tool beyond the system shell is needed. Exit 0 when connected.
const CONNECT_PROBE: &str = r#"exec 3<>"/dev/tcp/127.0.0.1/$1""#;

/// Run [`CONNECT_PROBE`] against `port`, unconfined or under `policy`.
pub(crate) fn run_connect_probe(
    policy: Option<&Path>,
    port: u16,
) -> Result<std::process::Output, BoundaryError> {
    let mut command = match policy {
        Some(policy) => {
            let mut command = std::process::Command::new(SANDBOX_EXEC);
            command.arg("-f").arg(policy).arg("/bin/bash");
            command
        }
        None => std::process::Command::new("/bin/bash"),
    };
    command
        .arg("-c")
        .arg(CONNECT_PROBE)
        .arg("bash")
        .arg(port.to_string())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            BoundaryError::SelfTestFailed(format!("could not start the egress probe: {error}"))
        })
}

/// Verify the egress rule: a host-owned listener on another loopback port
/// accepts an unconfined connect (the positive control) and refuses — with
/// the sandbox's `EPERM`, not a refused or reset connection — the same
/// connect under the policy.
fn egress_self_test(policy: &Path, proxy: SocketAddr) -> Result<(), BoundaryError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        BoundaryError::SelfTestFailed(format!(
            "could not open the egress probe's loopback listener: {error}"
        ))
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            BoundaryError::SelfTestFailed(format!(
                "the egress probe's listener has no port: {error}"
            ))
        })?
        .port();
    if port == proxy.port() {
        return Err(BoundaryError::SelfTestFailed(
            "the egress probe's listener landed on the proxy's own port".to_owned(),
        ));
    }
    let control = run_connect_probe(None, port)?;
    if !control.status.success() {
        return Err(BoundaryError::SelfTestFailed(
            "a loopback connect failed even without the boundary, so a refusal under it would \
             prove nothing"
                .to_owned(),
        ));
    }
    let bounded = run_connect_probe(Some(policy), port)?;
    let stderr = String::from_utf8_lossy(&bounded.stderr);
    if bounded.status.success() {
        return Err(BoundaryError::SelfTestFailed(
            "a loopback port other than the egress proxy's was reachable from inside the boundary"
                .to_owned(),
        ));
    }
    if !stderr.contains("Operation not permitted") {
        return Err(BoundaryError::SelfTestFailed(format!(
            "the egress probe failed under the boundary, but not with the boundary's refusal: {}",
            stderr.trim()
        )));
    }
    drop(listener);
    Ok(())
}

#[cfg(test)]
#[path = "exec_boundary_tests.rs"]
mod tests;
