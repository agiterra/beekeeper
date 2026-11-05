#!/usr/bin/env python3
"""The Host's own local gate: `just host-check`.

A fixed, explicit list, not a dependency heuristic. It covers the Host product and the
crates it is built on, and it refuses rather than passes when a change reaches outside
what the list can vouch for. Every selected step runs; the first failure stops the gate
and is named. Nothing is skipped silently. See TESTING.md § The Host gate.

Usage: scripts/host-check.py --base REV | --self-test
"""
import argparse
import json
import os
import pathlib
import shlex
import signal
import subprocess
import sys
import time
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]

# One list, shared with the remote Host lane (scripts/ci-gate-scope.mjs), which runs
# where this interpreter does not: the CI image has no Python.
POLICY = json.loads((ROOT / "scripts/host-gate-policy.json").read_text())

# Tested here because the Host is built from them or ships them.
HOST_PACKAGES = POLICY["host_packages"]

MENUBAR = POLICY["menubar"]

# A change here is what this gate vouches for.
IN_SCOPE_PREFIXES = tuple(POLICY["in_scope_prefixes"])
IN_SCOPE_FILES = set(POLICY["in_scope_files"])

# Consumed well beyond the Host, so a change here is not this gate's to pass.
SHARED_FANOUT = POLICY["shared_fanout"]


def classify(path):
    """(in_scope, reason) for one changed path."""
    if path in IN_SCOPE_FILES or path.startswith(IN_SCOPE_PREFIXES):
        return True, ""
    for prefix, consumers in SHARED_FANOUT.items():
        if path.startswith(prefix):
            return False, f"shared crate, also used by {consumers}"
    return False, "outside the Host product"


def changed_files(base):
    """Everything that differs from `base`, committed or not, including new files."""
    revision = subprocess.run(
        ["git", "rev-parse", "--verify", "--end-of-options", base + "^{commit}"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout.strip()
    paths = set()
    # Include both index and worktree: an unstaged reversal must not hide staged work.
    for cached in ([], ["--cached"]):
        output = subprocess.run(
            ["git", "diff", *cached, "--no-renames", "--name-only", "-z", revision, "--"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
        paths.update(output.split("\0"))
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    paths.update(untracked.split("\0"))
    return sorted(paths - {""})


def steps():
    """The whole gate, in order. Each is (name, argv, working directory)."""
    packages = [flag for name in HOST_PACKAGES for flag in ("-p", name)]
    manifest = "desktop/src-tauri/Cargo.toml"
    frontend = MENUBAR + "/frontend"
    biome = ["pnpm", "exec", "biome"]
    return [
        ("fmt: workspace", ["cargo", "fmt", "--all", "--", "--check"], "."),
        ("fmt: menu bar", ["cargo", "fmt", "--manifest-path", manifest, "-p", "beekeeper-menubar", "--", "--check"], "."),
        ("clippy: Host packages, all targets", ["cargo", "clippy", *packages, "--all-targets", "--", "-D", "warnings"], "."),
        ("clippy: menu bar, all targets", ["cargo", "clippy", "--manifest-path", manifest, "-p", "beekeeper-menubar", "--all-targets", "--", "-D", "warnings"], "."),
        ("frontend: format and lint", [*biome, "check", MENUBAR.removeprefix("desktop/") + "/frontend", "../scripts/host-manage-ui-check.mjs", "../scripts/host-manage-page-step.mjs", "../scripts/host-manage-ui-fixture-check.mjs"], "desktop"),
        ("frontend: no undeclared variables", [*biome, "lint", "--only=correctness/noUndeclaredVariables", MENUBAR.removeprefix("desktop/") + "/frontend/manage-host.js", MENUBAR.removeprefix("desktop/") + "/frontend/manage-host-view.js"], "desktop"),
        ("frontend: script parses", ["node", "--check", frontend + "/manage-host.js"], "."),
        ("frontend: view script parses", ["node", "--check", frontend + "/manage-host-view.js"], "."),
        ("frontend: harness scripts parse", ["python3", "-c", "import ast,sys;[ast.parse(open(p).read()) for p in sys.argv[1:]]", "scripts/host-check.py", "scripts/host-manage-check.py", "scripts/host-manage-recovery-check.py"], "."),
        ("test: Host packages", ["cargo", "test", *packages], "."),
        ("test: menu bar", ["cargo", "test", "--manifest-path", manifest, "-p", "beekeeper-menubar"], "."),
        ("build: menu bar embeds its frontend", ["cargo", "build", "--manifest-path", manifest, "-p", "beekeeper-menubar"], "."),
        ("compat: desktop Rust still builds against the Host libraries", ["cargo", "check", "--manifest-path", manifest, "-p", "beekeeper-desktop", "--all-targets"], "."),
    ]


def unit_env():
    """The same pinned closed ports `just test-unit` uses, so no test reaches a service."""
    command = f". {shlex.quote(str(ROOT / 'scripts/lib/unit-env.sh'))}; env -0"
    output = subprocess.run(["bash", "-c", command], cwd=ROOT, check=True, capture_output=True).stdout
    env = {}
    for item in output.split(b"\0"):
        if b"=" in item:
            key, value = item.split(b"=", 1)
            env[key.decode()] = value.decode()
    return adopt_legacy(env)


def adopt_legacy(env):
    """Read legacy names as current names, then drop the legacy names for this gate.

    `just` loads `.env`, which may still use the old names. A binary under test warns
    about any it inherits, and several tests require silence on stderr.
    """
    result = {key: value for key, value in env.items() if not key.startswith("BUZZ_")}
    for key, value in env.items():
        if key.startswith("BUZZ_") and len(key) > len("BUZZ_"):
            result.setdefault("BEEKEEPER_" + key[len("BUZZ_") :], value)
    return result


def run_step(argv, cwd, env):
    """Run in its own process group; an interrupted gate leaves nothing behind."""
    process = subprocess.Popen(argv, cwd=ROOT / cwd, env=env, start_new_session=True)
    try:
        return process.wait()
    except BaseException:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait()
        raise


def git_scope_test():
    """Exercise scope discovery against real staged and committed renames."""
    global ROOT
    original = ROOT
    try:
        with tempfile.TemporaryDirectory(prefix="host-gate-scope-") as directory:
            ROOT = pathlib.Path(directory)
            def git(*argv):
                return subprocess.run(["git", *argv], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            git("config", "user.name", "Host gate scope test")
            git("config", "user.email", "scope@invalid")
            source = ROOT / "migrations/0012.sql"
            source.parent.mkdir()
            source.write_text("test\n")
            git("add", ".")
            git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            destination = ROOT / "crates/beekeeper-host/src/moved.sql"
            destination.parent.mkdir(parents=True)
            git("mv", str(source.relative_to(ROOT)), str(destination.relative_to(ROOT)))
            for state in ("staged", "committed"):
                paths = changed_files(base)
                assert "migrations/0012.sql" in paths, (state, paths)
                assert any(not classify(path)[0] for path in paths), (state, paths)
                if state == "staged":
                    git("commit", "-qm", "rename")
            git("reset", "--hard", base)
            source.write_text("modified\n")
            git("add", ".")
            source.write_text("test\n")
            assert "migrations/0012.sql" in changed_files(base)
            try:
                changed_files("migrations/0012.sql")
            except subprocess.CalledProcessError:
                pass
            else:
                raise AssertionError("a path was accepted as a base commit")
    finally:
        ROOT = original


def self_test():
    git_scope_test()
    cases = {
        "crates/beekeeper-host/src/inventory.rs": True,
        "crates/beekeeper-host-core/src/agent_setup.rs": True,
        "crates/beekeeper-cli/src/commands/host_manage.rs": True,
        "desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.js": True,
        "desktop/src-tauri/Cargo.lock": True,
        "scripts/host-manage-check.py": True,
        "conformance/acme-acp/acme-acp": True,
        "crates/beekeeper-core/src/kind.rs": False,
        "crates/beekeeper-sdk/src/lib.rs": False,
        "crates/beekeeper-relay/src/main.rs": False,
        "crates/beekeeper-db/src/event.rs": False,
        "desktop/src/App.tsx": False,
        "desktop/src-tauri/src/lib.rs": False,
        "desktop/src-tauri/crates/beekeeper-tray/src/lib.rs": False,
        "mobile/lib/main.dart": False,
        "web/src/main.ts": False,
        "migrations/001.sql": False,
        "scripts/run-tests.sh": False,
        "crates/beekeeper-hostile/src/lib.rs": False,
    }
    wrong = {path: want for path, want in cases.items() if classify(path)[0] != want}
    if "shared crate" not in classify("crates/beekeeper-core/src/kind.rs")[1]:
        wrong["reason for a shared crate"] = True
    adopted = adopt_legacy({"BUZZ_A": "old", "BUZZ_B": "old", "BEEKEEPER_B": "new", "OTHER": "x"})
    if adopted != {"BEEKEEPER_A": "old", "BEEKEEPER_B": "new", "OTHER": "x"}:
        wrong["legacy environment"] = adopted
    names = [name for name, _, _ in steps()]
    for required in ("clippy: Host packages, all targets", "test: Host packages", "test: menu bar", "compat: desktop Rust still builds against the Host libraries", "frontend: no undeclared variables"):
        if required not in names:
            wrong[f"missing step {required}"] = True
    text = " ".join(" ".join(argv) for _, argv, _ in steps())
    for forbidden in ("just ci", "test:e2e", "flutter", "web/", "--workspace"):
        if forbidden in text:
            wrong[f"gate reaches {forbidden}"] = True
    for name in ("beekeeper-host", "beekeeper-session-provider", "beekeeper-agent-management", "beekeeper-cli"):
        if name not in HOST_PACKAGES:
            wrong[f"package {name}"] = True
    if wrong:
        print("self-test FAILED:", wrong)
        return 1
    print(f"self-test passed: {len(cases)} paths classified, {len(names)} steps checked")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", help="required branch-point commit; covers committed, staged and worktree changes")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if not args.base:
        parser.error("--base is required; supply the branch-point commit")
    if self_test():
        return 1

    files = changed_files(args.base)
    outside = [(path, classify(path)[1]) for path in files if not classify(path)[0]]
    print(f"host-check: {len(files)} changed file(s) against {args.base}")
    if outside:
        print("HOST_GATE_OUT_OF_SCOPE: this gate cannot vouch for these changes, so it refuses rather than passes:")
        for path, reason in outside:
            print(f"  {path}: {reason}")
        return 3

    env = unit_env()
    started = time.monotonic()
    results = []
    for name, argv, cwd in steps():
        print(f"\n== {name}\n$ {shlex.join(argv)}", flush=True)
        begun = time.monotonic()
        code = run_step(argv, cwd, env)
        results.append((name, code, time.monotonic() - begun))
        if code:
            break
    print("\nhost-check summary")
    for name, code, seconds in results:
        print(f"  {'ok  ' if code == 0 else 'FAIL'} {seconds:6.0f}s  {name}")
    print(f"  total {time.monotonic() - started:.0f}s")
    print("  Not run by design: relay, db and auth integration, desktop JS and Tauri tests, web, mobile, `just ci`.")
    print("  Product behavior is `just host-acceptance`, run separately.")
    return results[-1][1] if results and results[-1][1] else 0


if __name__ == "__main__":
    sys.exit(main())
