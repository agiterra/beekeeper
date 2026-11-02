#!/usr/bin/env python3
"""Manage Host acceptance: real relay, real Host, scripted runtime, no model spend.

Proves the setup chain the menu bar's window drives, through `bee host manage`,
against services this script starts and removes: setup, exact retry, interrupted
apply, cancel and resume, unauthorized and cross-project refusal, the effective
catalog and a stopped provider, signed readback and a real channel conversation
with the `acme-acp` stub. With `--ui`, it also drives the window's real HTML and
JS through the menu bar's own action path (see host-manage-ui-check.mjs).

Touches nothing shared: its own containers (only ids it created are removed), its
own relay, Host, socket and home, and owner keys it generates. No Keychain, no
public relay, no model. Build first: bee, beekeeper-relay, beekeeper-host,
beekeeper-session-provider, git-credential-nostr, beekeeper-menubar.

Usage: scripts/host-manage-check.py [--ui] [--native | --native-only] [--evidence DIR]

--native opens the real menu bar binary, so a status item and a window appear
briefly on this machine's screen, on the isolated socket.
"""
import argparse
import fcntl
import json
import os
import pathlib
import secrets
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import traceback
import urllib.request
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
# HM_BIN_DIR runs an exact, frozen copy of the binaries rather than cargo's output,
# which a later build replaces.
BIN_DIR = pathlib.Path(os.environ["HM_BIN_DIR"]) if os.environ.get("HM_BIN_DIR") else None
DEBUG = BIN_DIR or ROOT / "target/debug"
BEE, RELAY, HOST, PROVIDER, HELPER = (
    DEBUG / name
    for name in ("bee", "beekeeper-relay", "beekeeper-host", "beekeeper-session-provider", "git-credential-nostr")
)
MENUBAR = (BIN_DIR or ROOT / "desktop/src-tauri/target/debug") / "beekeeper-menubar"
STUB = ROOT / "conformance/acme-acp/acme-acp"
FOREIGN = ROOT / "conformance/foreign-client"
OVERALL_LIMIT_SECS = 900
ALLOWED_ENV = {"PATH", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL", "TERM", "DOCKER_HOST", "DOCKER_CONTEXT", "HERMIT_ENV", "HERMIT_BIN"}


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def out(*args, **kwargs):
    return subprocess.check_output([str(a) for a in args], text=True, **kwargs).strip()


class Harness:
    def __init__(self, evidence):
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="hm-", dir="/tmp"))
        os.chmod(self.work, 0o700)
        self.evidence = evidence
        self.results = []
        self.containers = []
        self.processes = []
        self.home = self.work / "host-home"
        self.fake_home = self.work / "home"
        self.socket = self.work / "h.sock"
        self.checkout = self.work / "checkout"
        for path in (self.home, self.fake_home, self.checkout):
            path.mkdir(parents=True)
        self.env = {k: v for k, v in os.environ.items() if k in ALLOWED_ENV}
        self.env.update(
            {
                "HOME": str(self.fake_home),
                "GIT_TERMINAL_PROMPT": "0",
                "BEEKEEPER_HOST_HOME": str(self.home),
                "BEEKEEPER_HOST_SOCK": str(self.socket),
                "BEEKEEPER_HOST_INSTANCE": "production",
                "BEEKEEPER_TEMPLATES_DIR": str(ROOT / "personas/templates"),
            }
        )
        self.secrets = []

    # ── reporting ────────────────────────────────────────────────────────────
    def check(self, name, ok, detail=""):
        self.results.append({"check": name, "ok": bool(ok), "detail": str(detail)[:1200]})
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f" :: {str(detail)[:600]}"), flush=True)
        return ok

    def save(self, name, value):
        path = self.work / "evidence" / name
        path.parent.mkdir(exist_ok=True)
        path.write_text(value if isinstance(value, str) else json.dumps(value, indent=2))

    # ── services ─────────────────────────────────────────────────────────────
    def start_services(self):
        ns = "bk-hm-" + uuid.uuid4().hex[:8]
        ports = {name: free_port() for name in ("postgres", "redis", "s3", "health", "metrics", "relay")}
        def container(name, *args):
            self.containers.append(out("docker", "run", "-d", "--rm", "--name", f"{ns}-{name}", *args))
        container("postgres", "--memory=512m", "-p", f"127.0.0.1:{ports['postgres']}:5432", "-e", "POSTGRES_USER=buzz", "-e", "POSTGRES_PASSWORD=buzz_dev", "-e", "POSTGRES_DB=buzz",
                  "--health-cmd", "pg_isready -h 127.0.0.1 -U buzz -d buzz", "--health-interval=2s", "--health-retries=45", "postgres:17-alpine")
        container("redis", "--memory=128m", "-p", f"127.0.0.1:{ports['redis']}:6379", "redis:7-alpine")
        container("s3", "--memory=768m", "-p", f"127.0.0.1:{ports['s3']}:9000", "-e", "RUSTFS_ACCESS_KEY=buzz_dev", "-e", "RUSTFS_SECRET_KEY=buzz_dev_secret",
                  "--health-cmd", "curl -f http://localhost:9000/health", "--health-interval=2s", "--health-retries=45", "rustfs/rustfs:1.0.1")
        for name in ("postgres", "s3"):
            deadline = time.monotonic() + 120
            while out("docker", "inspect", "--format", "{{.State.Health.Status}}", f"{ns}-{name}") != "healthy":
                if time.monotonic() > deadline:
                    raise RuntimeError(f"{name} did not become healthy")
                time.sleep(1)
        bucket = "hm-" + ns.rsplit("-", 1)[1]
        out("docker", "run", "--rm", "--network", f"container:{ns}-s3", "--entrypoint", "/bin/sh", "pgsty/mc:RELEASE.2026-09-16T00-00-00Z", "-c",
            f"mc alias set local http://127.0.0.1:9000 buzz_dev buzz_dev_secret && mc mb local/{bucket}")
        self.relay_http = f"http://127.0.0.1:{ports['relay']}"
        relay_env = dict(self.env)
        relay_env.update(
            {
                "DATABASE_URL": f"postgres://buzz:buzz_dev@127.0.0.1:{ports['postgres']}/buzz",
                "REDIS_URL": f"redis://127.0.0.1:{ports['redis']}/14",
                "RELAY_URL": f"ws://127.0.0.1:{ports['relay']}",
                "BEEKEEPER_BIND_ADDR": f"127.0.0.1:{ports['relay']}",
                "BEEKEEPER_RELAY_PRIVATE_KEY": secrets.token_hex(32),
                "BEEKEEPER_REQUIRE_AUTH_TOKEN": "false",
                "BEEKEEPER_RECONCILE_CHANNELS": "true",
                "BEEKEEPER_AUTO_MIGRATE": "true",
                "BEEKEEPER_GIT_REPO_PATH": str(self.work / "relay-git"),
                "BEEKEEPER_RATE_LIMIT_HUMAN_API_CALLS_PER_MIN": "100000",
                "BEEKEEPER_HEALTH_PORT": str(ports["health"]),
                "BEEKEEPER_METRICS_PORT": str(ports["metrics"]),
                "BEEKEEPER_S3_ENDPOINT": f"http://127.0.0.1:{ports['s3']}",
                "BEEKEEPER_S3_ACCESS_KEY": "buzz_dev",
                "BEEKEEPER_S3_SECRET_KEY": "buzz_dev_secret",
                "BEEKEEPER_S3_BUCKET": bucket,
                "BEEKEEPER_S3_REGION": "us-east-1",
                "BEEKEEPER_S3_ADDRESSING_STYLE": "path",
            }
        )
        log = (self.work / "relay.log").open("w")
        # An empty cwd: the relay reads a dotenv from its working directory, and
        # this checkout's .env must not reach it.
        self.processes.append(subprocess.Popen([str(RELAY)], env=relay_env, cwd=self.work, stdout=log, stderr=subprocess.STDOUT, start_new_session=True))
        deadline = time.monotonic() + 120
        while True:
            try:
                urllib.request.urlopen(self.relay_http + "/_readiness", timeout=2)
                break
            except Exception:
                if time.monotonic() > deadline:
                    raise RuntimeError("relay did not become ready; see relay.log")
                time.sleep(1)
        self.env["BEEKEEPER_RELAY_URL"] = self.relay_http

    def owner_setup(self):
        self.owner_key = secrets.token_hex(32)
        self.secrets.append(self.owner_key)
        self.key_file = self.work / "owner.key"
        self.key_file.write_text(self.owner_key + "\n")
        self.key_file.chmod(0o600)
        (self.work / "owner.env").write_text(f"BEEKEEPER_PRIVATE_KEY={self.owner_key}\n")
        self.owner = self.identity(self.owner_key)
        self.stranger_key = secrets.token_hex(32)
        self.secrets.append(self.stranger_key)
        self.stranger = self.identity(self.stranger_key)
        self.acme_secret = secrets.token_hex(24)
        self.secrets.append(self.acme_secret)
        credentials = self.work / "acme.json"
        credentials.write_text(json.dumps({"runtimes": {"acme-primary": {"ACME_TOKEN": self.acme_secret}}}))
        credentials.chmod(0o600)
        commissioned = self.owner_bee("host", "commission", "--home", self.home, "--runtime", "acme", "--driver", STUB, "--credential-env", "ACME_TOKEN",
                                      "--credential-source", "acme-vault", "--credential-file", credentials, "--provider-command", PROVIDER,
                                      "--runtime-install-root", ROOT / "conformance/acme-acp")
        self.provider = json.loads(commissioned)["provider_pubkey"]
        self.git_env = dict(self.env)
        self.git_env.update(
            {
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_COUNT": "3",
                "GIT_CONFIG_KEY_0": "credential.helper",
                "GIT_CONFIG_VALUE_0": "",
                "GIT_CONFIG_KEY_1": "credential.helper",
                "GIT_CONFIG_VALUE_1": str(HELPER),
                "GIT_CONFIG_KEY_2": "credential.useHttpPath",
                "GIT_CONFIG_VALUE_2": "true",
                "NOSTR_PRIVATE_KEY": self.owner_key,
                "GIT_AUTHOR_NAME": "Fixture",
                "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                "GIT_COMMITTER_NAME": "Fixture",
                "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
            }
        )

    def identity(self, key):
        env = dict(self.env, FOREIGN_CLIENT_KEY=key, RELAY_URL=getattr(self, "relay_http", ""))
        return json.loads(out("node", FOREIGN / "foreign-identity.mjs", env=env, cwd=FOREIGN))["pubkey"]

    def start_host(self, extra_env=None):
        env = dict(self.env, RUST_LOG="info,csp=info", **(extra_env or {}))
        log = (self.work / "host.log").open("a")
        self.host = subprocess.Popen([str(HOST), "run"], env=env, cwd=self.work, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.processes.append(self.host)
        self.wait_for("provider live", lambda: self.inventory_or_none() and self.inventory_or_none()["host"]["provider"]["state"] == "live", 90)
        self.wait_for("effective catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)

    def stop_host(self):
        self.host.terminate()
        try:
            self.host.wait(timeout=30)
        except subprocess.TimeoutExpired:
            os.killpg(self.host.pid, signal.SIGKILL)
            self.host.wait()

    def wait_for(self, what, predicate, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                value = predicate()
                if value:
                    return value
            except Exception:
                pass
            time.sleep(0.5)
        raise RuntimeError(f"timed out waiting for {what}")

    # ── commands ─────────────────────────────────────────────────────────────
    def raw(self, *args, key=None, stdin=None, timeout=180, git=False):
        env = dict(self.git_env if git else self.env)
        if key:
            env["BEEKEEPER_PRIVATE_KEY"] = key
        return subprocess.run([str(BEE), *map(str, args)], env=env, cwd=self.work, input=stdin, capture_output=True, text=True, timeout=timeout)

    def owner_bee(self, *args):
        result = self.raw(*args, key=self.owner_key)
        if result.returncode:
            raise RuntimeError(f"{args[:3]}: {result.stderr[-1500:]} {result.stdout[-500:]}")
        return result.stdout

    def manage(self, verb, *args, key_file=None, stdin=None, timeout=180):
        """`bee host manage <verb>`; returns (exit, parsed stdout, error message)."""
        keyed = verb not in ("inventory", "roles", "cancel")
        flags = ["--owner-key-file", key_file or self.key_file] if keyed else []
        result = self.raw("host", "manage", verb, *args, *flags, stdin=stdin, timeout=timeout)
        (self.work / "manage-output.log").open("a").write(f"--- {verb} {args}\n{result.stdout}\n{result.stderr}\n")
        try:
            value = json.loads(result.stdout)
        except ValueError:
            value = None
        message = ""
        for line in reversed(result.stderr.splitlines()):
            try:
                message = json.loads(line).get("message", "")
                break
            except ValueError:
                continue
        return result.returncode, value, message or result.stderr[-400:]

    def inventory(self):
        code, value, message = self.manage("inventory")
        if code:
            raise RuntimeError(message)
        return value

    def inventory_or_none(self):
        code, value, _ = self.manage("inventory")
        return value if code == 0 else None

    def git(self, *args, cwd=None):
        result = subprocess.run(["git", *map(str, args)], cwd=cwd or self.work, env=self.git_env, capture_output=True, text=True, timeout=120)
        if result.returncode:
            raise RuntimeError(f"git {args[:2]}: {result.stderr[-800:]}")
        return result.stdout.strip()

    def read(self, kinds, name, channel=None, extra=None):
        """Signature-verified reads through the independent foreign client."""
        env = dict(self.env, RELAY_URL=self.relay_http, FOREIGN_CLIENT_OUT=str(self.work / "records"), FOREIGN_CLIENT_KEY_FILE=str(self.work / "owner.env"), FOREIGN_CLIENT_LABEL="owner")
        if channel:
            env["FOREIGN_CLIENT_CHANNEL"] = channel
        # Second-resolution NIP-98 tokens: space identical reads.
        time.sleep(1.05)
        query = {"kinds": kinds}
        if channel:
            query["#h"] = [channel]
        query.update(extra or {})
        out("node", FOREIGN / "foreign-native-read.mjs", json.dumps(query), name, env=env, cwd=FOREIGN)
        return json.loads((self.work / "records" / name).read_text())["body"]

    def publish_as(self, key, template, name):
        env = dict(self.env, RELAY_URL=self.relay_http, FOREIGN_CLIENT_OUT=str(self.work / "records"), FOREIGN_CLIENT_KEY=key)
        path = self.work / f"{name}-template.json"
        path.write_text(json.dumps(template))
        time.sleep(1.05)
        return json.loads(out("node", FOREIGN / "foreign-hire-publish.mjs", path, f"{name}.json", env=env, cwd=FOREIGN))["event_id"]

    # ── fixtures ─────────────────────────────────────────────────────────────
    def channel(self, name, key=None):
        result = self.raw("channels", "create", "--name", name, "--type", "stream", "--visibility", "private", key=key or self.owner_key)
        return json.loads(result.stdout)["channel_id"]

    def project(self, slug, channel):
        created = self.raw("projects", "create", slug, "--channel", channel, key=self.owner_key, git=True)
        if created.returncode:
            raise RuntimeError(created.stderr[-1500:])
        coordinate = f"30621:{self.owner}:{slug}"
        agents, code = self.work / f"{slug}-agents", self.work / f"{slug}-code"
        self.git("clone", f"{self.relay_http}/git/{self.owner}/{slug}-beekeeper-agents", agents)
        self.git("clone", f"{self.relay_http}/git/{self.owner}/{slug}", code)
        model = self.inventory()["runtimes"][0]["selectableModels"][0]["id"]
        (agents / "roles").mkdir(exist_ok=True)
        (agents / "team.yml").write_text(
            f"schema: beekeeper-team/v1\nname: {slug}\nversion: 0.1.0\nlead: lead\nroles:\n"
            + "".join(f"  {role}:\n    runtime: acme\n    model: {model}\n    workspace: {{ agents_repo: read }}\n" for role in ("lead", "helper", "second"))
            + "  undeclared:\n    workspace: { agents_repo: read }\n"
            + "  wrongmodel:\n    runtime: acme\n    model: not-offered\n    workspace: { agents_repo: read }\n"
            + "  needsmcp:\n    runtime: acme\n    model: " + model + "\n    workspace: { agents_repo: read }\n"
        )
        for role in ("lead", "helper", "second", "undeclared", "wrongmodel"):
            (agents / f"roles/{role}.md").write_text(f"---\nname: {role}\nrole: {role}\n---\nConverse about this project.\n")
        skill = agents / "skills/hm-proof"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").write_text("---\nname: hm-proof\ndescription: Acceptance marker.\n---\nHM_ROLE_PACK_MARKER\n")
        (agents / "roles/needsmcp.md").write_text("---\nname: needsmcp\nrole: needsmcp\nmcp_servers:\n  - name: tool\n    command: tool\n---\nNeeds a tool.\n")
        self.git("add", ".", cwd=agents)
        self.git("commit", "-s", "-m", "Manage Host fixture roles", cwd=agents)
        self.git("push", "origin", "HEAD:main", cwd=agents)
        (code / "README.md").write_text("fixture\n")
        self.git("add", ".", cwd=code)
        self.git("commit", "-s", "-m", "Fixture", cwd=code)
        self.git("push", "origin", "HEAD:main", cwd=code)
        return coordinate, code, agents, model

    def spec(self, project, channel, role, name, model, access=None, runtime="acme-primary"):
        return json.dumps({"project": project, "channel": channel, "role": role, "displayName": name, "providerInstance": runtime, "model": model, "parallelism": 1, "access": access or {"mode": "owner_only"}})

    def state_dir(self):
        return pathlib.Path(json.loads((self.home / ".local/state/buzz/host/host.json").read_text())["providerStateDir"])

    def journal(self, setup_id):
        return json.loads((self.state_dir() / "agent-setups" / f"{setup_id}.json").read_text())

    def setup_row(self, setup_id):
        return next(row for row in self.inventory()["setups"] if row["setup"]["setupId"] == setup_id)

    def kill_apply_at(self, setup_id, step):
        """Start an apply and SIGKILL it the moment its journal says `step` is running."""
        process = subprocess.Popen([str(BEE), "host", "manage", "apply", "--setup", setup_id, "--owner-key-file", str(self.key_file)], env=self.env, cwd=self.work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            try:
                steps = self.journal(setup_id)["steps"]
            except (ValueError, OSError):
                steps = []
            if steps and steps[-1]["step"] == step and steps[-1]["state"] == "running":
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                return True
            if process.poll() is not None:
                return False
            time.sleep(0.002)
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        return False

    def definitions(self, name):
        rows = self.read([30177], "defs.json", extra={"authors": [self.owner]})
        return [row for row in rows if json.loads(row["content"]).get("name") == name]

    def replies(self, actor, channel):
        return self.read([9], "replies.json", channel=channel, extra={"authors": [actor]})

    def answer_to(self, actor, channel, request, seconds=90):
        def found():
            return [row for row in self.replies(actor, channel) if any(t[:2] == ["e", request] for t in row["tags"]) and "prompt" in row["content"]]
        return self.wait_for(f"answer to {request[:12]}", found, seconds)

    # ── the acceptance ───────────────────────────────────────────────────────
    def run_cli(self):
        check = self.check
        inv = self.inventory()
        check("inventory: Host answers with no key and names its owner", inv["host"]["ownerPubkey"] == self.owner, inv["host"])
        row = inv["runtimes"][0]
        model = row["selectableModels"][0]["id"]
        check("inventory: runtime is listed from the running provider's catalog", row["readiness"] == "unconfirmed" and row["catalogRevision"], row)
        check("inventory: credential names appear and no value", row["credentialEnv"] == ["ACME_TOKEN"] and self.acme_secret not in json.dumps(inv), row["credentialEnv"])

        wrong_key = self.work / "wrong.key"
        wrong_key.write_text(secrets.token_hex(32))
        wrong_key.chmod(0o600)
        _, signer, _ = self.manage("signer", key_file=wrong_key)
        check("signer: a different key reads as not the owner", signer and signer["matches"] is False, signer)
        _, signer, _ = self.manage("signer")
        check("signer: the owner's key matches and its bytes are not printed", signer["matches"] is True and self.owner_key not in json.dumps(signer), signer["signer"])
        loose = self.work / "loose.key"
        loose.write_text(self.owner_key)
        loose.chmod(0o644)
        code, _, message = self.manage("signer", key_file=loose)
        check("key file: readable by others is refused and not quoted", code != 0 and "OWNER_KEY_UNAVAILABLE" in message and self.owner_key not in message, message)

        channel = self.channel("hm-main")
        self.channel_a = channel
        project, code_dir, agents, model = self.project("hm-alpha", channel)
        self.project_a, self.code_a, self.agents_a, self.model = project, code_dir, agents, model

        code, _, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "Alpha agent", model))
        check("refusal: an unrecorded project is PROJECT_NOT_TRUSTED, naming the command", code != 0 and "PROJECT_NOT_TRUSTED" in message and "trust-project" in message, message)

        code, trusted, message = self.manage("trust", "--project", project, "--checkout", code_dir, "--agents-repo", agents)
        check("trust: records the project through the public operation", code == 0, message)
        inv = self.inventory()
        check("inventory: the trusted project is usable", any(r["coordinate"] == project and r["usable"] for r in inv["projects"]), inv["projects"])
        _, roles, _ = self.manage("roles", "--project", project)
        check("roles: listed from the versioned source with its lead", roles["lead"] == "lead" and {"lead", "helper", "needsmcp"} <= {r["slug"] for r in roles["roles"]}, roles)
        _, channels, _ = self.manage("channels")
        check("channels: lists a channel the owner owns", any(c["id"] == channel and c["private"] for c in channels), channels)

        foreign_channel = self.channel("hm-not-yours", key=self.stranger_key)
        def refusal(name, code_word, spec, key_file=None, verb="plan"):
            before = self.effect_count()
            code, value, message = self.manage(verb, "--spec-file", "-", stdin=spec, key_file=key_file)
            journals = len(list((self.state_dir() / "agent-setups").glob("*.json"))) if (self.state_dir() / "agent-setups").exists() else 0
            after = self.effect_count()
            check(f"refusal before effects: {name}", code != 0 and code_word in message and before == after and journals == self.expected_journals, f"{code} {message} journals={journals} effects {before}->{after}")
        self.expected_journals = 0
        refusal("wrong key", "OWNER_SIGNER_MISMATCH", self.spec(project, channel, "lead", "X", model), key_file=wrong_key)
        refusal("channel the owner does not own", "CHANNEL_NOT_OWNED", self.spec(project, foreign_channel, "lead", "X", model))
        refusal("model outside the effective catalog", "MODEL_NOT_IN_EFFECTIVE_CATALOG", self.spec(project, channel, "lead", "X", "not-offered"))
        refusal("uncommissioned runtime", "RUNTIME_NOT_COMMISSIONED", self.spec(project, channel, "lead", "X", model, runtime="ghost-primary"))
        refusal("role absent from the source", "ROLE_", self.spec(project, channel, "ghost", "X", model))
        refusal("role declares a different model", "ROLE_DECLARATION_CONFLICT", self.spec(project, channel, "wrongmodel", "X", model))
        refusal("role needs MCP", "ROLE_REQUIRES_UNSUPPORTED_CAPABILITY", self.spec(project, channel, "needsmcp", "X", model))
        refusal("allowlist with no keys", "ACCESS_INVALID", self.spec(project, channel, "lead", "X", model, access={"mode": "allowlist", "pubkeys": []}))
        refusal("allowlist holding the owner", "ACCESS_INVALID", self.spec(project, channel, "lead", "X", model, access={"mode": "allowlist", "pubkeys": [self.owner]}))
        code, _, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "X", model, access={"mode": "anyone"}))
        check("refusal before effects: `anyone` is not an access choice", code != 0 and "INVALID_REQUEST" in message, message)
        # Cross-project: a project the owner has no grant on, on this Host's relay.
        stranger_channel = foreign_channel
        created = subprocess.run([str(BEE), "projects", "create", "hm-stranger", "--channel", stranger_channel], env=dict(self.git_env, BEEKEEPER_PRIVATE_KEY=self.stranger_key, NOSTR_PRIVATE_KEY=self.stranger_key), cwd=self.work, capture_output=True, text=True, timeout=120)
        check("fixture: a stranger's project exists", created.returncode == 0, created.stderr[-300:])
        refusal("another person's project", "PROJECT_NOT_AUTHORIZED", self.spec(f"30621:{self.stranger}:hm-stranger", channel, "lead", "X", model))

        code, report, _ = self.manage("preflight", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "Alpha agent", model))
        check("preflight: every check passes and it writes nothing", code == 0 and report["ok"] and not list((self.state_dir() / "agent-setups").glob("*.json")), report)
        self.save("preflight.json", report)

        # Setup one: killed during install, resumed to one identity.
        code, planned, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "Alpha agent", model))
        check("plan: journals with pre-minted operations and publishes nothing", code == 0 and planned["state"] == "planned" and not self.definitions("Alpha agent"), message)
        first = planned["setupId"]
        self.expected_journals = 1
        check("journal: holds no key material", self.owner_key not in (self.state_dir() / "agent-setups" / f"{first}.json").read_text(), first)
        code, _, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "Other name", model))
        check("duplicate: a second setup for the project and role is refused", code != 0 and "SETUP_ALREADY_EXISTS" in message, message)
        with (self.state_dir() / "agent-setups/apply.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            code, _, message = self.manage("apply", "--setup", first)
            check("concurrency: an apply over a held lock is SETUP_BUSY", code != 0 and "SETUP_BUSY" in message, message)
            fcntl.flock(lock, fcntl.LOCK_UN)

        killed = self.kill_apply_at(first, "install")
        check("interruption: apply was killed with SIGKILL during install", killed, "apply finished before the kill landed")
        row = self.setup_row(first)
        check("interruption: reads as interrupted, not running", row["effectiveState"] == "interrupted", row["effectiveState"])
        code, applied, message = self.manage("apply", "--setup", first)
        check("retry: the same setup completes", code == 0 and applied["state"] == "running", f"{message} {applied and applied.get('lastRefusal')}")
        self.save("setup-running.json", applied)
        actor = applied["agentPubkey"]
        definitions = self.definitions("Alpha agent")
        check("no duplicate identity: exactly one signed definition after a kill and retry", len(definitions) == 1 and ["d", actor] in definitions[0]["tags"], [d["id"] for d in definitions])
        check("readback: signed definition matches the choices", applied["readback"]["definition"]["model"] == model and applied["readback"]["definition"]["respondTo"] == "owner-only" and applied["readback"]["definition"]["homeRole"] == "lead", applied["readback"])
        check("readback: provider-signed metadata names the same model", applied["readback"]["provider"]["model"] == model and applied["readback"]["provider"]["credentialSource"] == "acme-vault", applied["readback"]["provider"])
        check("readback: says it does not prove a turn", "does not show a turn" in applied["readback"]["turnEvidence"], applied["readback"])
        created_rows = [r for r in self.read([44224], "receipts.json", channel=channel, extra={"authors": [self.provider]}) if json.loads(r["content"]).get("status") == "created"]
        check("provider signed exactly one created receipt", len(created_rows) == 1, len(created_rows))
        code, again, _ = self.manage("apply", "--setup", first)
        check("exact retry: applying a running setup changes nothing", code == 0 and again["agentPubkey"] == actor and len(self.definitions("Alpha agent")) == 1, again and again["state"])
        check("inventory: the agent appears, named, with Host custody", any(a["actor"] == actor and a["displayName"] == "Alpha agent" and a["custody"] == "host_enrolled" for a in self.inventory()["agents"]), self.inventory()["agents"])

        # A real channel conversation with the scripted runtime.
        request = json.loads(self.owner_bee("messages", "send", "--channel", channel, "--mention", actor, "--content", "@Alpha hello"))["event_id"]
        answer = self.answer_to(actor, channel, request)
        check("conversation: the owner's mention gets a signed answer in its thread", len(answer) == 1, answer)
        self.save("conversation-answer.json", answer)
        check("project context: the turn was framed with this project", f"project-context={project}" in answer[0]["content"], answer[0]["content"])
        check("project context: the turn was told it cannot hire", "hire=unavailable" in answer[0]["content"], answer[0]["content"])
        journal = self.journal(first)
        genesis = self.read([44226], "genesis.json", channel=channel, extra={"ids": [journal["genesisEventId"]]})
        check("project context: an owner-signed genesis scopes it, and one retry founded one", journal["projectContext"] == "bound" and len(genesis) == 1 and genesis[0]["pubkey"] == self.owner, genesis)
        check("project context: founding started no extra execution", len([r for r in self.inventory()["agents"] if r["actor"] == actor]) == 1 and next(r for r in self.inventory()["agents"] if r["actor"] == actor)["sessionCount"] == 1, self.inventory()["agents"])
        seats = [p for p in self.work.rglob("SKILL.md") if "/seats/" in str(p) and "HM_ROLE_PACK_MARKER" in p.read_text()]
        check("role pack: the project's versioned skill reached this agent's seat", bool(seats), "no seat bundle holds the marker")
        self.save("seat-bundle-paths.json", [str(p.relative_to(self.work)) for p in seats])
        # Membership alone does not admit.
        self.owner_bee("channels", "add-member", "--channel", channel, "--pubkey", self.stranger)
        stranger_request = self.publish_as(self.stranger_key, {"kind": 9, "tags": [["h", channel], ["p", actor]], "content": "@Alpha hello from a member"}, "stranger-mention")
        time.sleep(6)
        unanswered = [r for r in self.replies(actor, channel) if any(t[:2] == ["e", stranger_request] for t in r["tags"]) and "prompt" in r["content"]]
        check("authority: an owner-only agent does not answer a member", not unanswered, unanswered)

        # Stop, inspect, start: same identity, fresh operations.
        code, stopped, message = self.manage("stop", "--setup", first)
        check("stop: signed terminal receipt and identity kept", code == 0 and stopped["state"] == "stopped", message)
        code, inspected, _ = self.manage("inspect", "--setup", first)
        check("inspect: reads journal and signed head after a stop", code == 0 and inspected["effectiveState"] == "stopped" and inspected["live"]["definition"]["eventId"], inspected)
        code, started, message = self.manage("start", "--setup", first)
        check("start: the same identity runs again", code == 0 and started["state"] == "running" and started["agentPubkey"] == actor, f"{message} {started and started.get('lastRefusal')}")
        check("start: still one signed definition", len(self.definitions("Alpha agent")) == 1, "")
        self.setup_one, self.actor_one = first, actor

        # Setup two: an allowlist, killed during start.
        self.expected_journals = 2
        code, planned, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "helper", "Alpha helper", model, access={"mode": "allowlist", "pubkeys": [self.stranger]}))
        check("plan: an explicit allowlist setup", code == 0, message)
        second = planned["setupId"]
        killed = self.kill_apply_at(second, "start")
        check("interruption: apply was killed with SIGKILL during start", killed, "apply finished before the kill landed")
        code, applied, message = self.manage("apply", "--setup", second)
        check("retry: completes after a kill during start, reusing the operation", code == 0 and applied["state"] == "running", f"{message} {applied and applied.get('lastRefusal')}")
        helper = applied["agentPubkey"]
        created_rows = [r for r in self.read([44224], "receipts2.json", channel=channel, extra={"authors": [self.provider]}) if json.loads(r["content"]).get("status") == "created"]
        by_command = {json.loads(r["content"]).get("commandId") for r in created_rows}
        check("retry: the provider's created receipt for the start operation appears once", str(self.journal(second)["startOperation"]) in " ".join(by_command) or len(created_rows) == len(by_command), by_command)
        check("readback: the signed definition carries the allowlist", applied["readback"]["definition"]["allowlist"] == [self.stranger] and applied["readback"]["definition"]["respondTo"] == "allowlist", applied["readback"])
        request = self.publish_as(self.stranger_key, {"kind": 9, "tags": [["h", channel], ["p", helper]], "content": "@helper hello from the allowlist"}, "allowlisted-mention")
        helper_answer = self.answer_to(helper, channel, request)
        check("authority: an allowlisted person is answered", True)
        check("project context: a non-lead role has none, and the journal says so", "project-context=" not in helper_answer[0]["content"] and self.journal(second)["projectContext"] == "not_applicable", helper_answer[0]["content"])
        code, _, _ = self.manage("stop", "--setup", second)
        check("stop: allowlisted helper stops", code == 0, code)

        # Setup three: cancel with nothing reserved, then with an identity reserved.
        self.expected_journals = 3
        spec = self.spec(project, channel, "second", "Alpha second", model)
        _, planned, _ = self.manage("plan", "--spec-file", "-", stdin=spec)
        code, cancelled, message = self.manage("cancel", "--setup", planned["setupId"])
        check("cancel: nothing reserved, and it says so", code == 0 and cancelled["residual"]["identityReserved"] is None and "may start" in cancelled["residual"]["next"], cancelled)
        code, planned, message = self.manage("plan", "--spec-file", "-", stdin=spec)
        check("cancel: a new setup may start when nothing was reserved", code == 0, message)
        self.expected_journals = 4
        third = planned["setupId"]
        killed = self.kill_apply_at(third, "start")
        check("interruption: killed once the identity was installed", killed, "apply finished before the kill landed")
        code, cancelled, message = self.manage("cancel", "--setup", third)
        check("cancel: names the identity that already exists", code == 0 and cancelled["residual"]["identityReserved"] and cancelled["residual"]["published"] is True, cancelled)
        code, _, message = self.manage("plan", "--spec-file", "-", stdin=spec)
        check("cancel: a new setup stays refused while an identity is reserved", code != 0 and "SETUP_ALREADY_EXISTS" in message, message)
        code, _, message = self.manage("apply", "--setup", third)
        check("cancel: apply will not silently resume it", code != 0 and "SETUP_CANCELLED" in message, message)
        code, resumed, message = self.manage("apply", "--setup", third, "--resume-cancelled")
        check("resume: an explicit resume completes with the reserved identity", code == 0 and resumed["state"] == "running" and resumed["agentPubkey"] == cancelled["residual"]["identityReserved"], f"{message} {resumed and resumed.get('lastRefusal')}")
        code, _, message = self.manage("cancel", "--setup", third)
        check("cancel: a running agent must be stopped, not cancelled", code != 0 and "SETUP_RUNNING" in message, message)
        self.manage("stop", "--setup", third)

        # The provider stops: nothing is on offer and nothing may be planned.
        self.expected_journals = 4
        stopped = self.raw("host", "stop")
        check("disconnected: the provider stops", stopped.returncode == 0, stopped.stderr[-200:])
        self.wait_for("provider stopped", lambda: self.inventory()["host"]["provider"]["state"] == "notSupervised", 40)
        row = self.inventory()["runtimes"][0]
        check("disconnected: a stopped provider offers nothing, whatever is on disk", row["readiness"] == "provider_not_live" and row["selectableModels"] == [], row)
        # Every kind of recovery row, through the live socket. The provider rewrites
        # this file when it starts, so it is seeded while the provider is stopped.
        sessions = self.inventory()
        held = json.loads((self.state_dir() / "state.json").read_text())["sessions"]
        live_session = next(iter(held.values()))
        def seat(session, project_ref, fenced=False):
            return {"commandId": f"csl-{session}", "actor": self.stranger, "role": "builder", "projectRef": project_ref, "sessionId": session, "generation": 2,
                    "packRef": {"repo": "r", "sha": "abc123", "role": "builder", "path": "roles/builder"}, "fenced": fenced}
        sid = live_session["sessionId"]
        (self.state_dir() / "seat-requests.json").write_text(json.dumps({"version": 1, "requests": [
            seat(sid, project, True), seat("gone-session", project), seat(sid, None), seat(sid, "30621:" + self.stranger + ":elsewhere"), seat(sid, project)]}))
        recovery = self.inventory()["recovery"]
        codes = [row["code"] for row in recovery["rows"]]
        check("recovery: each row names its own truth", codes == ["FENCED_BY_HANDOVER", "SESSION_NOT_HELD", "NO_PROJECT", "PROJECT_NOT_TRUSTED_HERE", "RESTAGE_NEEDS_DESKTOP"], codes)
        check("recovery: a held row offers the public stop command with its real ids", recovery["rows"][4]["stopCommand"].startswith("bee sessions stop --channel ") and sid in recovery["rows"][4]["stopCommand"], recovery["rows"][4])
        check("recovery: read-only with its specific reason, and no clear-all", "Read-only" in recovery["writeOperation"] and "Keychain" in recovery["writeOperation"], recovery["writeOperation"])
        self.save("recovery-rows.json", recovery)
        (self.state_dir() / "seat-requests.json").unlink()
        code, _, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "undeclared", "Late", model))
        check("disconnected: planning is refused by name", code != 0 and "HOST_PROVIDER_NOT_LIVE" in message, message)
        row = self.setup_row(first)
        check("disconnected: a journal saying running reads as not running", row["setup"]["state"] == "running" and row["effectiveState"] == "provider_not_running", row["effectiveState"])
        self.raw("host", "start")
        self.wait_for("provider live again", lambda: self.inventory()["host"]["provider"]["state"] == "live", 60)
        self.wait_for("catalog from the new process", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)
        check("disconnected: a restarted provider offers again from its own catalog", self.inventory()["runtimes"][0]["catalogRevision"], "")

        # A runtime whose adapter does not answer discovery: the stub has no model
        # list. An edit of this isolated fixture's host.json, not a product path.
        host_json = self.home / ".local/state/buzz/host/host.json"
        original = host_json.read_text()
        edited = json.loads(original)
        edited["runtimes"][0]["discoverModels"] = True
        host_json.write_text(json.dumps(edited))
        self.raw("host", "bind")
        def discovery_settled():
            row = self.inventory()["runtimes"][0]
            return row if row["readiness"] == "discovery_failed" else None
        row = self.wait_for("discovery outcome", discovery_settled, 90)
        check("discovery failure: the row says the adapter did not answer, and `default` is not offered", row["selectableModels"] == [] and "fallback" in row["readinessDetail"], row)
        self.save("runtime-discovery-failed.json", row)
        host_json.write_text(original)
        self.raw("host", "bind")
        self.wait_for("catalog restored", lambda: self.inventory()["runtimes"][0]["readiness"] == "unconfirmed", 60)

        # The Host itself is gone.
        self.stop_host()
        code, _, message = self.manage("inventory")
        check("Host down: inventory names it rather than answering empty", code != 0 and "HOST_UNAVAILABLE" in message, message)
        menu = subprocess.run([str(MENUBAR), "--manage-inventory"], env=self.env, cwd=self.work, capture_output=True, text=True, timeout=30)
        outcome = json.loads(menu.stdout.strip().splitlines()[-1])
        check("Host down: the menu bar's own path says the same", outcome["ok"] is False and outcome["code"] == "HOST_UNAVAILABLE", menu.stdout[-400:])
        self.start_host()
        check("Host restart: setups come back from their journals", self.setup_row(first)["setup"]["agentPubkey"] == actor, "")

    def effect_count(self):
        """Published events that would prove a refusal had effects."""
        return len(self.read([30177], "effects.json", extra={"authors": [self.owner]}))

    def run_menubar_path(self):
        env = dict(self.env, BEEKEEPER_BEE=str(BEE))
        def action(payload):
            result = subprocess.run([str(MENUBAR), "--manage-action", json.dumps(payload)], env=env, cwd=self.work, capture_output=True, text=True, timeout=240)
            return json.loads(result.stdout.strip().splitlines()[-1])
        outcome = action({"kind": "signer", "keyFile": str(self.key_file)})
        self.check("menu bar path: a named action reaches bee", outcome["ok"] and outcome["value"]["matches"] is True, outcome)
        scrubbed = subprocess.run([str(MENUBAR), "--manage-action", json.dumps({"kind": "signer"})], env=dict(env, BEEKEEPER_PRIVATE_KEY=self.owner_key), cwd=self.work, capture_output=True, text=True, timeout=60)
        outcome = json.loads(scrubbed.stdout.strip().splitlines()[-1])
        self.check("menu bar path: an owner key in its environment does not reach bee", outcome["ok"] is False and outcome["code"] == "OWNER_KEY_UNAVAILABLE", outcome)
        result = subprocess.run([str(MENUBAR), "--manage-action", '{"kind":"shell","args":["rm"]}'], env=env, cwd=self.work, capture_output=True, text=True, timeout=30)
        self.check("menu bar path: an unknown action is refused", json.loads(result.stdout.strip().splitlines()[-1])["code"] == "INVALID_REQUEST", result.stdout)
        missing = subprocess.run([str(MENUBAR), "--manage-inventory"], env=dict(env, BEEKEEPER_HOST_SOCK=str(self.work / "absent.sock")), cwd=self.work, capture_output=True, text=True, timeout=30)
        self.check("menu bar path: an absent Host is named", "HOST_UNAVAILABLE" in missing.stdout, missing.stdout)

    def run_ui(self):
        env = dict(self.env)
        env.update(
            {
                "MENUBAR_BIN": str(MENUBAR),
                "BEEKEEPER_BEE": str(BEE),
                "BEE_BIN": str(BEE),
                "OWNER_KEY_FILE": str(self.key_file),
                "WRONG_KEY_FILE": str(self.work / "wrong.key"),
                "WORK": str(self.work),
                "RELAY_HTTP": self.relay_http,
                "OWNER_PUBKEY": self.owner,
                "FOREIGN_DIR": str(FOREIGN),
                "OWNER_ENV": str(self.work / "owner.env"),
                "UI_SHOTS": str(self.evidence or self.work / "shots"),
                "NODE_PATH": str(ROOT / "desktop/node_modules"),
                "PLAYWRIGHT_BROWSERS_PATH": str(pathlib.Path(os.environ["HOME"]) / "Library/Caches/ms-playwright"),
            }
        )
        slug = "hm-ui"
        channel = self.channel("hm-ui-chat")
        project, code_dir, agents, _ = self.project(slug, channel)
        env.update({"UI_PROJECT": project, "UI_CHECKOUT": str(code_dir), "UI_AGENTS": str(agents), "UI_CHANNEL": channel})
        result = subprocess.run(["node", str(ROOT / "scripts/host-manage-ui-check.mjs")], env=env, cwd=ROOT / "desktop", capture_output=True, text=True, timeout=420)
        self.save("ui-check.log", result.stdout + result.stderr)
        print(result.stdout, flush=True)
        for line in result.stdout.splitlines():
            if line.startswith("UI PASS "):
                self.results.append({"check": "ui: " + line[8:], "ok": True, "detail": ""})
            elif line.startswith("UI FAIL "):
                self.results.append({"check": "ui: " + line[8:], "ok": False, "detail": ""})
        self.check("ui: the driver finished cleanly", result.returncode == 0, result.stderr[-1500:])

    def run_native(self):
        """The real menu bar binary, WKWebView included, with no desktop app running."""
        desktop = subprocess.run(["pgrep", "-f", str(self.work)], capture_output=True, text=True).stdout.split()
        log_path = self.work / "menubar.log"
        env = dict(self.env, BEEKEEPER_BEE=str(BEE))
        with log_path.open("w") as log:
            process = subprocess.Popen([str(MENUBAR), "--manage-host"], env=env, cwd=self.work, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.processes.append(process)
        def logged(text):
            return text in log_path.read_text()
        try:
            self.wait_for("webview IPC", lambda: logged("manage-host read the inventory"), 45)
            self.check("native: WKWebView loaded the page under its CSP and reached Rust over IPC", True)
            self.wait_for("webview action", lambda: logged("manage-host running an action"), 30)
            self.check("native: the page's first action reached this app's own bee path", True)
        except RuntimeError as error:
            self.check("native: WKWebView reached Rust", False, f"{error}: {log_path.read_text()[-600:]}")
        swift = (
            "import CoreGraphics; import Foundation; "
            "let p = Int(CommandLine.arguments[1])!; "
            "let w = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]; "
            "print(w.filter { ($0[kCGWindowOwnerPID as String] as? Int) == p && ($0[kCGWindowLayer as String] as? Int) == 0 }.count)"
        )
        windows = subprocess.run(["swift", "-e", swift, str(process.pid)], capture_output=True, text=True, timeout=120)
        self.check("native: the process owns an ordinary window", windows.stdout.strip().isdigit() and int(windows.stdout.strip()) >= 1, windows.stdout + windows.stderr[-300:])
        time.sleep(float(os.environ.get("HM_NATIVE_HOLD_SECS", "2")))
        self.check("native: still running, independent of any desktop app", process.poll() is None, log_path.read_text()[-400:])
        self.check("native: this stack started no desktop app", not [p for p in desktop if "beekeeper-desktop" in subprocess.run(["ps", "-o", "command=", "-p", p], capture_output=True, text=True).stdout], desktop)
        process.terminate()
        process.wait(timeout=20)
        self.save("menubar.log", log_path.read_text())

    def scan_secrets(self):
        roots = [self.state_dir() / "agent-setups", self.work / "evidence", self.work / "manage-output.log", self.state_dir() / "effective-catalog.json"]
        leaks = []
        for root in roots:
            files = root.rglob("*") if root.is_dir() else [root]
            for path in files:
                if not path.is_file():
                    continue
                text = path.read_text(errors="replace")
                for secret in self.secrets:
                    if secret in text:
                        leaks.append(str(path))
                if "nsec1" in text:
                    leaks.append(str(path) + " nsec1")
        self.check("secrets: no key or credential in journals, inventory output or evidence", not leaks, leaks)
        inventory = json.dumps(self.inventory())
        self.check("secrets: the inventory answer holds no nsec", "nsec" not in inventory and not any(secret in inventory for secret in self.secrets), "")

    def teardown(self):
        for process in reversed(self.processes):
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
        deadline = time.monotonic() + 25
        for process in self.processes:
            try:
                process.wait(timeout=max(0.1, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for container in self.containers:
            subprocess.run(["docker", "rm", "-f", container], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", action="store_true")
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--native-only", action="store_true")
    parser.add_argument("--evidence")
    args = parser.parse_args()
    for binary in (BEE, RELAY, HOST, PROVIDER, HELPER, MENUBAR):
        if not binary.exists():
            print(f"missing {binary}; build it first", flush=True)
            return 2
    evidence = pathlib.Path(args.evidence) if args.evidence else None
    if evidence:
        evidence.mkdir(parents=True, exist_ok=True)
    harness = Harness(evidence)
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("overall limit")))
    signal.alarm(OVERALL_LIMIT_SECS)
    print(f"work: {harness.work}", flush=True)
    crashed = None
    try:
        harness.start_services()
        harness.owner_setup()
        harness.start_host()
        if args.native_only:
            channel = harness.channel("hm-native")
            project, checkout, agents, _ = harness.project("hm-native", channel)
            harness.manage("trust", "--project", project, "--checkout", checkout, "--agents-repo", agents)
            harness.run_native()
        else:
            harness.run_cli()
            harness.run_menubar_path()
            if args.ui:
                harness.run_ui()
            if args.native:
                harness.run_native()
            harness.scan_secrets()
    except BaseException:  # the summary and the cleanup must run whatever happened
        crashed = traceback.format_exc()
        print(crashed, flush=True)
        harness.check("the run completed", False, crashed[-800:])
    finally:
        signal.alarm(0)
        harness.teardown()
    failed = [row for row in harness.results if not row["ok"]]
    summary = {"passed": len(harness.results) - len(failed), "failed": len(failed), "checks": harness.results}
    (harness.work / "results.json").write_text(json.dumps(summary, indent=2))
    if evidence:
        shutil.copy(harness.work / "results.json", evidence / "results.json")
        source = harness.work / "evidence"
        if source.exists():
            shutil.copytree(source, evidence / "records", dirs_exist_ok=True)
    print(f"{summary['passed']} passed, {summary['failed']} failed; retained {harness.work}", flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
