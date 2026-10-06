//! The seeder's decisions and its filesystem work, against an in-memory
//! filesystem.
//!
//! `buzz-core` carries no dev-dependencies, so there is no `tempfile` here.
//! That is a feature rather than a constraint: a fake lets a test hold a
//! source checkout mid-build, or make a destination a link that leads out of
//! the tree, which is awkward to arrange on a real disk and is exactly where
//! the dangerous cases are.

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Component;

use crate::sandbox_manifest::{parse_sandbox_yml, reclaim_plan, SANDBOX_SCHEMA};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    File(String),
    Dir,
    Link(PathBuf),
}

/// An in-memory filesystem plus the git and locking answers a seeder asks for.
#[derive(Default)]
struct FakeFs {
    nodes: RefCell<BTreeMap<PathBuf, Node>>,
    tracked: BTreeSet<String>,
    /// Paths a trailing-slash ignore rule hides: a directory but not a link.
    ignored_dirs: BTreeSet<String>,
    /// Paths ignored whatever shape they are.
    ignored_always: BTreeSet<String>,
    locks: BTreeSet<PathBuf>,
    clone_supported: bool,
    bytes: u64,
}

impl FakeFs {
    fn new() -> Self {
        Self {
            clone_supported: true,
            bytes: 36_000_000_000,
            ..Self::default()
        }
    }

    fn dir(self, path: &str) -> Self {
        self.insert(path, Node::Dir)
    }

    fn file(self, path: &str, body: &str) -> Self {
        self.insert(path, Node::File(body.to_owned()))
    }

    fn symlink(self, at: &str, target: &str) -> Self {
        self.insert(at, Node::Link(PathBuf::from(target)))
    }

    fn insert(self, path: &str, node: Node) -> Self {
        let path = PathBuf::from(path);
        let mut nodes = self.nodes.borrow_mut();
        let mut ancestor = path.parent().map(Path::to_path_buf);
        while let Some(current) = ancestor {
            if current.as_os_str().is_empty() {
                break;
            }
            nodes.entry(current.clone()).or_insert(Node::Dir);
            ancestor = current.parent().map(Path::to_path_buf);
        }
        nodes.insert(path, node);
        drop(nodes);
        self
    }

    fn ignoring_dirs(mut self, paths: &[&str]) -> Self {
        self.ignored_dirs
            .extend(paths.iter().map(|p| (*p).to_owned()));
        self
    }

    fn ignoring_always(mut self, paths: &[&str]) -> Self {
        self.ignored_always
            .extend(paths.iter().map(|p| (*p).to_owned()));
        self
    }

    fn tracking(mut self, paths: &[&str]) -> Self {
        self.tracked.extend(paths.iter().map(|p| (*p).to_owned()));
        self
    }

    fn locked(mut self, paths: &[&str]) -> Self {
        self.locks.extend(paths.iter().map(PathBuf::from));
        self
    }

    fn without_clone_support(mut self) -> Self {
        self.clone_supported = false;
        self
    }

    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let mut current = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    current.push(name);
                    let found = self.nodes.borrow().get(&current).cloned();
                    match found {
                        Some(Node::Link(target)) => current = self.resolve(&target)?,
                        Some(_) => {}
                        None => return None,
                    }
                }
                _ => return None,
            }
        }
        Some(current)
    }

    fn descendants(&self, root: &Path) -> Vec<PathBuf> {
        self.nodes
            .borrow()
            .keys()
            .filter(|path| path.starts_with(root) && path.as_path() != root)
            .cloned()
            .collect()
    }

    fn kind_of(&self, path: &str) -> Option<NodeKind> {
        self.node_kind(Path::new(path))
    }

    fn text_of(&self, path: &str) -> Option<String> {
        match self.nodes.borrow().get(Path::new(path)) {
            Some(Node::File(body)) => Some(body.clone()),
            _ => None,
        }
    }
}

impl SeedOps for FakeFs {
    fn node_kind(&self, path: &Path) -> Option<NodeKind> {
        self.nodes.borrow().get(path).map(|node| match node {
            Node::File(_) => NodeKind::File,
            Node::Dir => NodeKind::Directory,
            Node::Link(_) => NodeKind::Symlink,
        })
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        match self.nodes.borrow().get(path) {
            Some(Node::Link(target)) => Some(target.clone()),
            _ => None,
        }
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        self.resolve(path)
    }

    fn children(&self, path: &Path) -> std::io::Result<Vec<String>> {
        if !matches!(self.node_kind(path), Some(NodeKind::Directory)) {
            return Err(std::io::Error::other("not a directory"));
        }
        Ok(self
            .nodes
            .borrow()
            .keys()
            .filter(|candidate| candidate.parent() == Some(path))
            .filter_map(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect())
    }

    fn tree_bytes(&self, path: &Path) -> Option<u64> {
        if self.node_kind(path).is_some() {
            Some(self.bytes)
        } else {
            None
        }
    }

    fn clone_supported(&self, _from: &Path, _to: &Path) -> bool {
        self.clone_supported
    }

    fn tracked(&self, _root: &Path, relative: &str) -> bool {
        self.tracked.contains(relative)
    }

    fn ignored(&self, _root: &Path, relative: &str, kind: NodeKind) -> bool {
        if self.ignored_always.contains(relative) {
            return true;
        }
        match kind {
            NodeKind::Symlink => false,
            _ => self.ignored_dirs.contains(relative),
        }
    }

    fn lock_held(&self, path: &Path) -> bool {
        self.locks.contains(path)
    }

    fn make_dir(&self, path: &Path) -> std::io::Result<()> {
        let mut nodes = self.nodes.borrow_mut();
        let mut ancestor = Some(path.to_path_buf());
        while let Some(current) = ancestor {
            if current.as_os_str().is_empty() || current == Path::new("/") {
                break;
            }
            nodes.entry(current.clone()).or_insert(Node::Dir);
            ancestor = current.parent().map(Path::to_path_buf);
        }
        Ok(())
    }

    fn clone_tree(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if !self.clone_supported {
            return Err(std::io::Error::other("clone not supported"));
        }
        self.copy_tree(from, to)
    }

    fn copy_tree(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if self.node_kind(from).is_none() {
            return Err(std::io::Error::other("source is absent"));
        }
        if self.node_kind(to).is_some() {
            return Err(std::io::Error::other("destination exists"));
        }
        let moved: Vec<(PathBuf, Node)> = {
            let nodes = self.nodes.borrow();
            let root = nodes.get(from).cloned().expect("source checked above");
            let mut collected = vec![(to.to_path_buf(), root)];
            for path in self.descendants(from) {
                let suffix = path.strip_prefix(from).expect("descendant");
                let node = nodes.get(&path).cloned().expect("descendant");
                collected.push((to.join(suffix), node));
            }
            collected
        };
        let mut nodes = self.nodes.borrow_mut();
        for (path, node) in moved {
            nodes.insert(path, node);
        }
        Ok(())
    }

    fn link(&self, target: &Path, at: &Path) -> std::io::Result<()> {
        self.nodes
            .borrow_mut()
            .insert(at.to_path_buf(), Node::Link(target.to_path_buf()));
        Ok(())
    }

    fn unlink(&self, path: &Path) -> std::io::Result<()> {
        match self.nodes.borrow_mut().remove(path) {
            Some(Node::Dir) => Err(std::io::Error::other("is a directory")),
            Some(_) => Ok(()),
            None => Err(std::io::Error::other("not there")),
        }
    }

    fn remove_tree(&self, path: &Path) -> std::io::Result<()> {
        let doomed = self.descendants(path);
        let mut nodes = self.nodes.borrow_mut();
        for victim in doomed {
            nodes.remove(&victim);
        }
        nodes.remove(path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let moved: Vec<(PathBuf, Node)> = {
            let nodes = self.nodes.borrow();
            let Some(root) = nodes.get(from).cloned() else {
                return Err(std::io::Error::other("not there"));
            };
            let mut collected = vec![(to.to_path_buf(), root)];
            for path in self.descendants(from) {
                let suffix = path.strip_prefix(from).expect("descendant");
                collected.push((to.join(suffix), nodes.get(&path).cloned().expect("node")));
            }
            collected
        };
        self.remove_tree(from)?;
        let mut nodes = self.nodes.borrow_mut();
        for (path, node) in moved {
            nodes.insert(path, node);
        }
        Ok(())
    }

    fn read_text(&self, path: &Path) -> std::io::Result<String> {
        match self.nodes.borrow().get(path) {
            Some(Node::File(body)) => Ok(body.clone()),
            _ => Err(std::io::Error::other("not a file")),
        }
    }

    fn write_text(&self, path: &Path, text: &str) -> std::io::Result<()> {
        self.nodes
            .borrow_mut()
            .insert(path.to_path_buf(), Node::File(text.to_owned()));
        Ok(())
    }
}

const TREE: &str = "/tree";
const SOURCE: &str = "/src";
const POOL: &str = "/pool";

fn plan_from(entries: &str) -> SandboxPlan {
    parse_sandbox_yml(&format!(
        "schema: {SANDBOX_SCHEMA}\npool: beekeeper-build\nentries:\n{entries}"
    ))
    .expect("the test manifest should parse")
}

fn inputs<'a>(with_pool: bool) -> SeedInputs<'a> {
    SeedInputs {
        source: Some(Path::new(SOURCE)),
        dest: Path::new(TREE),
        pool_root: if with_pool {
            Some(Path::new(POOL))
        } else {
            None
        },
    }
}

fn seed(plan: &SandboxPlan, fs: &FakeFs, with_pool: bool) -> SeedReceipt {
    let program = preflight(Some(plan), &inputs(with_pool), fs);
    apply(&program, fs)
}

fn only(receipt: &SeedReceipt) -> &SeedOutcome {
    assert_eq!(
        receipt.outcomes.len(),
        1,
        "expected exactly one outcome, got {:?}",
        receipt.outcomes
    );
    &receipt.outcomes[0]
}

fn code_of(receipt: &SeedReceipt) -> Option<&'static str> {
    only(receipt).disposition.code()
}

// ── the happy path, so the refusals below mean something ─────────────────

fn clone_target_fs() -> FakeFs {
    FakeFs::new()
        .dir(TREE)
        .dir("/src/target/debug")
        .ignoring_dirs(&["target"])
}

#[test]
fn a_declared_clone_lands_and_names_the_mechanism_it_used() {
    let fs = clone_target_fs();
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    let outcome = only(&receipt);
    assert_eq!(outcome.disposition, SeedDisposition::Seeded);
    assert_eq!(outcome.used, Some(SeedMechanism::Clonefile));
    assert!(receipt.complete);
    assert_eq!(fs.kind_of("/tree/target"), Some(NodeKind::Directory));
    assert_eq!(fs.kind_of("/tree/target/debug"), Some(NodeKind::Directory));
}

#[test]
fn a_clones_exclusive_size_is_unknown_because_its_blocks_are_shared() {
    let fs = clone_target_fs();
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    let bytes = only(&receipt).bytes;
    assert!(bytes.logical.is_some());
    assert_eq!(
        bytes.exclusive, None,
        "a clone shares its blocks with the source, so what freeing it would release is unknown"
    );
}

#[test]
fn nothing_is_staged_in_place_so_a_crash_leaves_no_half_tree() {
    let fs = clone_target_fs();
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    seed(&plan, &fs, true);
    let leftovers: Vec<_> = fs
        .nodes
        .borrow()
        .keys()
        .filter(|path| path.to_string_lossy().contains(".seeding-"))
        .cloned()
        .collect();
    assert!(
        leftovers.is_empty(),
        "the staging directory must be renamed into place, not left behind: {leftovers:?}"
    );
}

// ── S4: the source is the host's to name ─────────────────────────────────

#[test]
fn a_destination_that_escapes_the_new_tree_is_refused() {
    // The destination's parent is a link that leads out of the sandbox.
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/elsewhere")
        .symlink("/tree/out", "/elsewhere")
        .dir("/src/out/target")
        .ignoring_dirs(&["out/target"]);
    let plan = plan_from("  - { path: out/target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_DEST_OUTSIDE));
    assert!(
        fs.kind_of("/elsewhere/target").is_none(),
        "nothing may be written outside the sandbox"
    );
}

#[test]
fn a_source_symlink_that_leads_out_of_the_checkout_is_refused() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir(SOURCE)
        .dir("/elsewhere/target")
        .symlink("/src/target", "/elsewhere/target")
        .ignoring_dirs(&["target"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_SOURCE_OUTSIDE));
}

#[test]
fn with_no_source_checkout_recorded_nothing_is_seeded() {
    let fs = FakeFs::new().dir(TREE).ignoring_dirs(&["target"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let program = preflight(
        Some(&plan),
        &SeedInputs {
            source: None,
            dest: Path::new(TREE),
            pool_root: None,
        },
        &fs,
    );
    let receipt = apply(&program, &fs);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_SOURCE_NOT_RECORDED));
    assert!(!receipt.complete);
}

#[test]
fn seeding_over_a_tracked_path_is_refused() {
    let fs = clone_target_fs().tracking(&["target"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_PATH_TRACKED));
    assert!(fs.kind_of("/tree/target").is_none());
}

// ── S1: never a symlink at a root git would not ignore ───────────────────

#[test]
fn a_link_git_would_not_ignore_is_refused_and_names_the_shape_that_works() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/build")
        .ignoring_dirs(&["build"]);
    let plan = plan_from("  - { path: build, kind: symlink, link: self }\n");
    let receipt = seed(&plan, &fs, true);
    let outcome = only(&receipt);
    assert_eq!(
        outcome.disposition,
        SeedDisposition::Refused {
            code: SANDBOX_SEED_NOT_IGNORED
        }
    );
    assert!(
        outcome.detail.contains("link: entries"),
        "the refusal must name the expression that works: {}",
        outcome.detail
    );
    assert!(fs.kind_of("/tree/build").is_none());
}

#[test]
fn a_link_git_ignores_whatever_its_shape_is_accepted() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/build")
        .ignoring_always(&["build"]);
    let plan = plan_from("  - { path: build, kind: symlink, link: self }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::Seeded);
    assert_eq!(fs.kind_of("/tree/build"), Some(NodeKind::Symlink));
}

#[test]
fn an_entry_linked_directory_is_a_real_directory_of_links() {
    // The shape a trailing-slash ignore rule still hides, and the shape pnpm
    // survives.
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/deps/alpha")
        .dir("/src/deps/beta")
        .ignoring_dirs(&["deps"]);
    let plan = plan_from("  - { path: deps, kind: symlink, link: entries }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::Seeded);
    assert_eq!(
        fs.kind_of("/tree/deps"),
        Some(NodeKind::Directory),
        "the root must be a real directory, or a directory ignore pattern would not hide it"
    );
    assert_eq!(fs.kind_of("/tree/deps/alpha"), Some(NodeKind::Symlink));
    assert_eq!(fs.kind_of("/tree/deps/beta"), Some(NodeKind::Symlink));
}

// ── clone support, and the downgrade that must be disclosed ──────────────

#[test]
fn a_clone_on_an_unsupported_filesystem_refuses_by_default() {
    let fs = clone_target_fs().without_clone_support();
    let plan =
        plan_from("  - { path: target, kind: clone, on_unsupported: refuse, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_CLONE_UNSUPPORTED));
    assert!(
        fs.kind_of("/tree/target").is_none(),
        "a refused clone must never quietly become a real copy of a directory this size"
    );
}

#[test]
fn a_clone_that_falls_back_to_a_copy_discloses_the_mechanism_used() {
    let fs = clone_target_fs().without_clone_support();
    let plan =
        plan_from("  - { path: target, kind: clone, on_unsupported: copy, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    let outcome = only(&receipt);
    assert_eq!(
        outcome.disposition,
        SeedDisposition::Downgraded {
            code: SANDBOX_SEED_CLONE_UNSUPPORTED,
            to: SeedKind::Copy
        }
    );
    assert_eq!(outcome.declared, SeedKind::Clone);
    assert_eq!(outcome.used, Some(SeedMechanism::Copy));
    assert_eq!(
        outcome.bytes.exclusive, outcome.bytes.logical,
        "a real copy's bytes are its own, so freeing it releases all of them"
    );
}

// ── an absent source ─────────────────────────────────────────────────────

#[test]
fn an_absent_optional_source_path_is_skipped_and_disclosed() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir(SOURCE)
        .ignoring_always(&[".env"]);
    let plan = plan_from("  - { path: .env, kind: copy, required: false }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(
        only(&receipt).disposition,
        SeedDisposition::Skipped {
            code: SANDBOX_SEED_SOURCE_ABSENT
        }
    );
    assert!(
        receipt.complete,
        "a checkout that never had an env file is not a failure"
    );
}

#[test]
fn an_absent_required_source_path_is_refused() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir(SOURCE)
        .ignoring_dirs(&["target"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_SOURCE_ABSENT));
    assert!(!receipt.complete);
}

// ── S3: never read a directory a build is writing ────────────────────────

#[test]
fn a_build_in_progress_in_the_source_skips_the_entry_and_says_why() {
    let fs = clone_target_fs().locked(&["/src/target/debug/.cargo-lock"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    let outcome = only(&receipt);
    assert_eq!(
        outcome.disposition,
        SeedDisposition::Unavailable {
            code: SANDBOX_SEED_DONOR_BUSY
        }
    );
    assert!(
        outcome.detail.contains("torn copy"),
        "the skip must say what it is protecting: {}",
        outcome.detail
    );
    assert!(fs.kind_of("/tree/target").is_none());
    assert!(
        !receipt.complete,
        "an unseeded entry is not a complete seed"
    );
}

#[test]
fn a_cargo_home_is_checked_against_its_own_package_cache_lock() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/.hermit/rust")
        .ignoring_always(&[".hermit/rust"])
        .locked(&["/src/.hermit/rust/.package-cache"]);
    let plan = plan_from("  - { path: .hermit/rust, kind: share, id: cargo-home, lock: none }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_DONOR_BUSY));
}

// ── the rewrite that stops a package manager eating the shared store ─────

fn node_modules_fs(state: &str) -> FakeFs {
    FakeFs::new()
        .dir(TREE)
        .dir("/src/node_modules/.pnpm")
        .file("/src/node_modules/.pnpm-workspace-state-v1.json", state)
        .ignoring_dirs(&["node_modules"])
}

const NODE_MODULES_ENTRY: &str = "  - { path: node_modules, kind: clone, \
                                   rewrite: [\".pnpm-workspace-state-v1.json\"], \
                                   reclaim: delete }\n";

#[test]
fn a_seeded_state_file_is_re_rooted_onto_the_new_sandbox() {
    let fs = node_modules_fs("{\"projects\":{\"/src\":{},\"/src/desktop\":{}}}");
    let receipt = seed(&plan_from(NODE_MODULES_ENTRY), &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::Seeded);
    let state = fs
        .text_of("/tree/node_modules/.pnpm-workspace-state-v1.json")
        .expect("the state file should have been seeded");
    assert!(
        state.contains("/tree") && !state.contains("/src"),
        "every path naming the source checkout must be re-rooted, or the package manager reads \
         the install as stale and purges the store: {state}"
    );
}

#[test]
fn a_rewrite_that_matches_nothing_is_refused_never_silently_skipped() {
    let fs = node_modules_fs("{\"projects\":{}}");
    let receipt = seed(&plan_from(NODE_MODULES_ENTRY), &fs, true);
    let outcome = only(&receipt);
    assert_eq!(
        outcome.disposition,
        SeedDisposition::Failed {
            code: SANDBOX_SEED_REWRITE_NO_MATCH
        }
    );
    assert!(
        outcome.detail.contains("purge"),
        "the failure must say what a silent no-op would have cost: {}",
        outcome.detail
    );
    assert!(
        fs.kind_of("/tree/node_modules").is_none(),
        "a directory whose state file could not be re-rooted must be undone, not left behind"
    );
    assert!(!receipt.complete);
}

#[test]
fn a_rewrite_naming_a_file_the_source_does_not_have_is_refused() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/node_modules/.pnpm")
        .ignoring_dirs(&["node_modules"]);
    let receipt = seed(&plan_from(NODE_MODULES_ENTRY), &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_REWRITE_ABSENT));
    assert!(fs.kind_of("/tree/node_modules").is_none());
}

#[test]
fn the_declared_env_reaches_the_caller() {
    let fs = node_modules_fs("{\"projects\":{\"/src\":{}}}");
    let plan = plan_from(
        "  - { path: node_modules, kind: clone, rewrite: [\".pnpm-workspace-state-v1.json\"], \
         env: { PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN: \"false\" }, reclaim: delete }\n",
    );
    let receipt = seed(&plan, &fs, true);
    assert_eq!(
        receipt
            .env
            .get("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN")
            .map(String::as_str),
        Some("false")
    );
}

// ── share: one pool, filled once ─────────────────────────────────────────

fn cargo_home_fs() -> FakeFs {
    FakeFs::new()
        .dir(TREE)
        .dir("/src/.hermit/rust/registry")
        .ignoring_always(&[".hermit/rust"])
}

const CARGO_HOME_ENTRY: &str =
    "  - { path: .hermit/rust, kind: share, id: cargo-home, lock: none }\n";

#[test]
fn a_share_entry_fills_the_pool_once_and_links_into_it() {
    let fs = cargo_home_fs();
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    let outcome = only(&receipt);
    assert_eq!(outcome.disposition, SeedDisposition::Seeded);
    assert_eq!(outcome.used, Some(SeedMechanism::Share));
    assert_eq!(fs.kind_of("/tree/.hermit/rust"), Some(NodeKind::Symlink));
    assert_eq!(
        fs.link_target(Path::new("/tree/.hermit/rust")),
        Some(PathBuf::from("/pool/cargo-home"))
    );
    assert_eq!(
        fs.kind_of("/pool/cargo-home/registry"),
        Some(NodeKind::Directory),
        "the pool should have been filled from the source checkout"
    );
    assert_eq!(
        fs.kind_of("/tree/.hermit"),
        Some(NodeKind::Directory),
        "the parent must be a real directory so the ignore rule still hides what is under it"
    );
}

#[test]
fn a_populated_pool_is_not_refilled() {
    let fs = cargo_home_fs().dir("/pool/cargo-home/registry");
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert!(only(&receipt)
        .notes
        .iter()
        .any(|note| note.contains("already populated")));
}

#[test]
fn a_share_entry_declares_its_lock_as_declared_not_as_measured() {
    let fs = cargo_home_fs();
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert!(
        only(&receipt)
            .notes
            .iter()
            .any(|note| note.contains("not measured")),
        "a declared lock must never read as a checked one: {:?}",
        only(&receipt).notes
    );
}

#[test]
fn a_link_into_a_pool_frees_nothing_and_says_so() {
    let fs = cargo_home_fs();
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert_eq!(render_seed_bytes(only(&receipt).bytes), "frees nothing");
}

#[test]
fn a_share_entry_with_no_project_scope_downgrades_to_a_clone_and_discloses_it() {
    let fs = cargo_home_fs();
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, false);
    let outcome = only(&receipt);
    assert_eq!(
        outcome.disposition,
        SeedDisposition::Downgraded {
            code: SANDBOX_SEED_SHARE_NO_SCOPE,
            to: SeedKind::Clone
        }
    );
    assert_eq!(
        fs.kind_of("/tree/.hermit/rust"),
        Some(NodeKind::Directory),
        "with no pool to share, the entry must land as a real directory, not a fake 'shared' one"
    );
    assert!(outcome
        .notes
        .iter()
        .any(|note| note.contains("one tree's own directory")));
}

// ── S2: delete nothing this engine did not write ─────────────────────────

#[test]
fn re_seeding_a_warm_sandbox_leaves_it_alone() {
    let fs = clone_target_fs().dir("/tree/target/debug");
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::AlreadyPresent);
    assert!(receipt.complete);
    assert_eq!(fs.kind_of("/tree/target/debug"), Some(NodeKind::Directory));
}

#[test]
fn re_seeding_an_identical_link_is_already_present() {
    let fs = cargo_home_fs().symlink("/tree/.hermit/rust", "/pool/cargo-home");
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::AlreadyPresent);
}

#[test]
fn a_link_this_seeder_did_not_place_is_left_exactly_as_it_is() {
    let fs = cargo_home_fs().symlink("/tree/.hermit/rust", "/somebody/elses/cache");
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_DEST_OCCUPIED));
    assert_eq!(
        fs.link_target(Path::new("/tree/.hermit/rust")),
        Some(PathBuf::from("/somebody/elses/cache")),
        "someone else's link must survive a re-seed untouched"
    );
}

#[test]
fn a_link_this_seeder_placed_earlier_is_re_pointed() {
    let fs = cargo_home_fs().symlink("/tree/.hermit/rust", "/pool/stale-pool");
    let receipt = seed(&plan_from(CARGO_HOME_ENTRY), &fs, true);
    assert_eq!(only(&receipt).disposition, SeedDisposition::Seeded);
    assert_eq!(
        fs.link_target(Path::new("/tree/.hermit/rust")),
        Some(PathBuf::from("/pool/cargo-home"))
    );
}

#[test]
fn a_file_where_a_directory_belongs_is_refused() {
    let fs = clone_target_fs().file("/tree/target", "not a directory");
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_DEST_OCCUPIED));
    assert_eq!(
        fs.text_of("/tree/target").as_deref(),
        Some("not a directory")
    );
}

// ── run entries belong to the caller ─────────────────────────────────────

#[test]
fn no_run_entry_is_executed_by_the_core_seeder() {
    let fs = clone_target_fs();
    let plan = plan_from(
        "  - { kind: run, recipe: _ensure-sidecar-stubs, produces: [binaries], \
         reclaim: delete }\n",
    );
    let receipt = seed(&plan, &fs, true);
    assert!(
        receipt.outcomes.is_empty(),
        "a run has no outcome until whoever holds the boundary has run it"
    );
    assert_eq!(receipt.pending_runs.len(), 1);
    assert_eq!(
        receipt.pending_runs[0].recipe.as_deref(),
        Some("_ensure-sidecar-stubs")
    );
    assert!(fs.kind_of("/tree/binaries").is_none());
}

#[test]
fn a_receipt_with_runs_outstanding_is_not_complete() {
    let fs = clone_target_fs();
    let plan =
        plan_from("  - { kind: run, recipe: stubs, produces: [binaries], reclaim: delete }\n");
    let mut receipt = seed(&plan, &fs, true);
    assert!(
        !receipt.complete,
        "nobody has said whether the project's own setup recipe worked"
    );
    let run = receipt.pending_runs[0].clone();
    receipt.record_run(&run, SeedDisposition::Seeded, "ran, exit 0");
    assert!(receipt.complete);
    assert_eq!(receipt.outcomes.len(), 1);
    assert_eq!(receipt.outcomes[0].declared, SeedKind::Run);
}

#[test]
fn a_failed_run_leaves_the_receipt_incomplete_and_names_the_entry() {
    let fs = clone_target_fs();
    let plan =
        plan_from("  - { kind: run, recipe: stubs, produces: [binaries], reclaim: delete }\n");
    let mut receipt = seed(&plan, &fs, true);
    let run = receipt.pending_runs[0].clone();
    receipt.record_run(
        &run,
        SeedDisposition::Failed {
            code: SANDBOX_SEED_FAILED,
        },
        "exit 1",
    );
    assert!(!receipt.complete);
    assert_eq!(receipt.unsatisfied().len(), 1);
    assert_eq!(receipt.unsatisfied()[0].id, "stubs");
}

#[test]
fn a_boundary_is_never_claimed_unless_someone_said_so() {
    let fs = clone_target_fs();
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let mut receipt = seed(&plan, &fs, true);
    assert_eq!(
        receipt.boundary, None,
        "an empty value must not read as 'this was bounded'"
    );
    receipt.set_boundary(SeedBoundary::NotEnforced {
        reason: "no backend on this platform".to_owned(),
    });
    assert!(matches!(
        receipt.boundary,
        Some(SeedBoundary::NotEnforced { .. })
    ));
}

// ── the receipt as a whole ───────────────────────────────────────────────

#[test]
fn every_entry_appears_in_the_receipt_exactly_once() {
    let fs = clone_target_fs()
        .dir("/src/node_modules")
        .file("/src/node_modules/.pnpm-workspace-state-v1.json", "/src")
        .dir("/src/.hermit/rust")
        .ignoring_dirs(&["node_modules"])
        .ignoring_always(&[".hermit/rust"]);
    let plan = plan_from(&format!(
        "  - {{ path: target, kind: clone, reclaim: delete }}\n{NODE_MODULES_ENTRY}{CARGO_HOME_ENTRY}"
    ));
    let receipt = seed(&plan, &fs, true);
    assert_eq!(receipt.outcomes.len(), 3);
    let mut ids: Vec<&str> = receipt.outcomes.iter().map(|o| o.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, [".hermit/rust", "node_modules", "target"]);
}

#[test]
fn a_receipt_with_any_refused_entry_is_not_complete() {
    let fs = clone_target_fs().tracking(&["target"]);
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    assert!(!seed(&plan, &fs, true).complete);
}

#[test]
fn a_reclaim_only_entry_is_skipped_without_being_a_failure() {
    let fs = FakeFs::new()
        .dir(TREE)
        .dir("/src/build")
        .ignoring_dirs(&["build"]);
    let plan = plan_from("  - { path: build, kind: copy, seed: never, reclaim: delete }\n");
    let receipt = seed(&plan, &fs, true);
    assert_eq!(
        only(&receipt).disposition,
        SeedDisposition::Skipped { code: "seed-never" }
    );
    assert!(receipt.complete);
    assert!(fs.kind_of("/tree/build").is_none());
}

#[test]
fn a_dry_run_reports_the_same_lines_and_writes_nothing() {
    let fs = clone_target_fs();
    let plan = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let program = preflight(Some(&plan), &inputs(true), &fs);
    let preview = program.preview();
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].used, Some(SeedMechanism::Clonefile));
    assert!(preview[0].detail.starts_with("would clone"));
    assert!(
        fs.kind_of("/tree/target").is_none(),
        "a preview must not write"
    );
}

#[test]
fn a_project_with_no_manifest_seeds_nothing_and_says_which_list_reclaim_uses() {
    let fs = FakeFs::new().dir(TREE).dir(SOURCE);
    let program = preflight(None, &inputs(true), &fs);
    let receipt = apply(&program, &fs);
    assert_eq!(code_of(&receipt), Some(SANDBOX_SEED_MANIFEST_ABSENT));
    assert!(only(&receipt).detail.contains("fallback list"));
    assert_eq!(receipt.manifest_sha256, None);
}

// ── reclaim: the bug this unification exists to kill ─────────────────────

#[test]
fn reclaim_never_removes_the_tree_behind_a_link_however_it_is_declared() {
    // A declaration of `reclaim: delete` over a path that turns out to be a
    // link must take the link and nothing else. `remove_dir_all` happens to
    // behave this way on macOS today — measured, not assumed — but nothing in
    // this repository pins that, so the branch is explicit and this test is
    // what holds it.
    let fs = FakeFs::new()
        .dir("/src/target/debug")
        .symlink("/tree/target", "/src/target");
    let declared = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(
        receipt.outcomes[0].disposition,
        ReclaimDisposition::Unlinked
    );
    assert_eq!(
        fs.kind_of("/src/target/debug"),
        Some(NodeKind::Directory),
        "what the link pointed at must survive untouched"
    );
    assert!(fs.kind_of("/tree/target").is_none());
    assert_eq!(receipt.freed(), (0, false));
}

#[test]
fn a_declared_link_reclaims_as_an_unlink_that_frees_nothing() {
    let fs = FakeFs::new()
        .dir("/src/deps")
        .symlink("/tree/deps", "/src/deps");
    let declared = plan_from("  - { path: deps, kind: symlink }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(
        receipt.outcomes[0].disposition,
        ReclaimDisposition::Unlinked
    );
    assert!(receipt.outcomes[0].detail.contains("frees nothing"));
}

#[test]
fn a_declared_link_that_is_not_one_is_left_alone_rather_than_removed_on_a_guess() {
    let fs = FakeFs::new().dir("/tree/deps/alpha");
    let declared = plan_from("  - { path: deps, kind: symlink }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(
        receipt.outcomes[0].disposition,
        ReclaimDisposition::Refused {
            code: SANDBOX_RECLAIM_LINK_NOT_DIRECTORY
        }
    );
    assert_eq!(fs.kind_of("/tree/deps/alpha"), Some(NodeKind::Directory));
}

#[test]
fn a_shared_pool_is_never_reclaimed() {
    let fs = FakeFs::new()
        .dir("/pool/cargo-home")
        .symlink("/tree/.hermit/rust", "/pool/cargo-home");
    let declared =
        plan_from("  - { path: .hermit/rust, kind: share, id: cargo-home, lock: none }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(receipt.outcomes[0].disposition, ReclaimDisposition::Kept);
    assert_eq!(
        fs.kind_of("/tree/.hermit/rust"),
        Some(NodeKind::Symlink),
        "a pool every sandbox of the project resolves through is not one sandbox's to empty"
    );
}

#[test]
fn a_cloned_directory_is_removed_and_its_freed_bytes_are_not_overclaimed() {
    let fs = FakeFs::new().dir("/tree/target/debug");
    let declared = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(receipt.outcomes[0].disposition, ReclaimDisposition::Removed);
    assert!(fs.kind_of("/tree/target").is_none());
    assert_eq!(
        receipt.outcomes[0].bytes.exclusive, None,
        "a clone's blocks are shared, so the freed total must not count its logical size"
    );
    let (total, unknown) = receipt.freed();
    assert_eq!(total, 0);
    assert!(
        unknown,
        "the receipt must say that something it removed could not be measured"
    );
}

#[test]
fn a_copied_directorys_freed_bytes_are_its_own() {
    let fs = FakeFs::new().dir("/tree/build/intermediates");
    let declared = plan_from("  - { path: build, kind: copy, reclaim: delete }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(receipt.outcomes[0].disposition, ReclaimDisposition::Removed);
    let (total, unknown) = receipt.freed();
    assert_eq!(total, 36_000_000_000);
    assert!(!unknown);
}

#[test]
fn a_path_that_is_not_there_reads_absent_not_removed() {
    let fs = FakeFs::new().dir(TREE);
    let declared = plan_from("  - { path: target, kind: clone, reclaim: delete }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(receipt.outcomes[0].disposition, ReclaimDisposition::Absent);
}

#[test]
fn a_reclaim_never_entry_is_kept_even_when_it_is_there() {
    let fs = FakeFs::new().file("/tree/.env", "KEY=value");
    let declared = plan_from("  - { path: .env, kind: copy, required: false }\n");
    let receipt = reclaim(&reclaim_plan(Some(&declared)), Path::new(TREE), &fs);
    assert_eq!(receipt.outcomes[0].disposition, ReclaimDisposition::Kept);
    assert_eq!(fs.text_of("/tree/.env").as_deref(), Some("KEY=value"));
}

#[test]
fn a_project_with_no_manifest_reclaims_the_fallback_list_and_says_so() {
    let fs = FakeFs::new()
        .dir("/tree/target")
        .dir("/tree/desktop/node_modules");
    let receipt = reclaim(&reclaim_plan(None), Path::new(TREE), &fs);
    assert_eq!(
        receipt.source,
        crate::sandbox_manifest::ReclaimSource::ClosedListFallback
    );
    assert!(fs.kind_of("/tree/target").is_none());
    assert!(fs.kind_of("/tree/desktop/node_modules").is_none());
}

// ── how a size is rendered ───────────────────────────────────────────────

#[test]
fn an_unmeasurable_size_reads_unknown_never_zero() {
    assert_eq!(
        render_seed_bytes(SeedBytes {
            logical: None,
            exclusive: None
        }),
        "unknown"
    );
}

#[test]
fn a_shared_size_reads_as_an_upper_bound_not_as_a_number_to_act_on() {
    let rendered = render_seed_bytes(SeedBytes {
        logical: Some(36_000_000_000),
        exclusive: None,
    });
    assert!(
        rendered.starts_with("up to 36.0 GB") && rendered.contains("unknown"),
        "a clone's size must not read as reclaimable disk: {rendered}"
    );
}

#[test]
fn an_exclusive_size_reads_as_itself() {
    assert_eq!(
        render_seed_bytes(SeedBytes {
            logical: Some(45_000_000_000),
            exclusive: Some(45_000_000_000)
        }),
        "45.0 GB"
    );
}

#[test]
fn a_link_reads_as_freeing_nothing() {
    assert_eq!(
        render_seed_bytes(SeedBytes {
            logical: None,
            exclusive: Some(0)
        }),
        "frees nothing"
    );
}
