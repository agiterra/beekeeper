#!/usr/bin/env python3
"""Isolated signed-event vertical acceptance; invoked by headless-acme-check."""
import hashlib
import json
import os
import pathlib
import subprocess
import signal
import shutil
import secrets
import sys
import time
import uuid
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORK = pathlib.Path(sys.argv[1])
CHANNEL, OWNER, BEE = sys.argv[2:5]
BEE = str(pathlib.Path(BEE).resolve())
ENV = dict(os.environ)
ENV.update({"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_COUNT": "3", "GIT_CONFIG_KEY_2": "credential.useHttpPath", "GIT_CONFIG_VALUE_2": "true", "GIT_CONFIG_KEY_0": "credential.helper", "GIT_CONFIG_VALUE_0": "",
            "GIT_CONFIG_KEY_1": "credential.helper", "GIT_CONFIG_VALUE_1": str(ROOT / "target/debug/git-credential-nostr"),
            "NOSTR_PRIVATE_KEY": ENV["BEEKEEPER_PRIVATE_KEY"], "GIT_TERMINAL_PROMPT": "0",
            "GIT_AUTHOR_NAME": "Acme fixture", "GIT_AUTHOR_EMAIL": "acme@example.invalid",
            "GIT_COMMITTER_NAME": "Acme fixture", "GIT_COMMITTER_EMAIL": "acme@example.invalid"})
HOME = WORK / "host-home"
LISTENER = None


def save(name, value):
    (WORK / name).write_text(json.dumps(value, indent=2))
    return WORK / name


def run(*args, cwd=ROOT, timeout=120):
    result = subprocess.run([str(a) for a in args], cwd=cwd, env=ENV, capture_output=True, text=True, timeout=timeout)
    if result.returncode and not (result.returncode == 5 and "--no-wait" in args and json.loads(result.stdout).get("accepted")):
        raise RuntimeError(f"{args[:3]} failed ({result.returncode}): {result.stderr[-5000:]} {result.stdout[-1000:]}")
    return result.stdout.strip()


def bee(*args):
    return json.loads(run(BEE, *args))


def read(kinds, name):
    run("node", ROOT / "conformance/foreign-client/foreign-native-read.mjs",
        json.dumps({"kinds": kinds, "#h": [CHANNEL]}), name)
    return json.loads((WORK / "records" / name).read_text())["body"]


def until(fn, description, seconds=90):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(1)
    raise RuntimeError("timed out: " + description)


def mode(value):
    (WORK / "proxy-mode").write_text(value)


def stop_listener():
    global LISTENER
    if LISTENER is not None:
        LISTENER.terminate()
        try:
            LISTENER.wait(timeout=10)
        except subprocess.TimeoutExpired:
            LISTENER.kill()
            LISTENER.wait()
        LISTENER = None


def main():
    global LISTENER
    slug = "hire-" + uuid.uuid4().hex[:12]
    project = f"30621:{OWNER}:{slug}"
    relay = ENV["BEEKEEPER_RELAY_URL"]
    provider = (WORK / "provider.pubkey").read_text().strip()
    config = json.loads((HOME / ".local/state/buzz/host/host.json").read_text())
    runtime = config["runtimes"][0]
    model = runtime["allowedModels"][0]
    save("project-create.json", bee("projects", "create", slug, "--channel", CHANNEL))
    agents, code = WORK / "agents", WORK / "code"
    run("git", "clone", f"{relay}/git/{OWNER}/{slug}-beekeeper-agents", agents)
    run("git", "clone", f"{relay}/git/{OWNER}/{slug}", code)
    (agents / "team.yml").write_text(f"schema: beekeeper-team/v1\nname: acme-hire-proof\nversion: 0.1.0\nlead: builder\nroles:\n  builder:\n    runtime: acme\n    model: {model}\n    workspace: {{ agents_repo: read }}\n")
    (agents / "roles/builder.md").write_text("---\nname: builder\nrole: builder\n---\nACME_HIRE_PROOF_V1 canonical builder role. Read the assigned fixture and report its fact.\n")
    skill = agents / "skills/hire-proof"
    skill.mkdir(parents=True)
    (skill / "SKILL.md").write_text("---\nname: hire-proof\ndescription: Read the harmless fixture marker.\n---\nACME_HIRE_SKILL_V1: read hire-proof.json and report marker 42.\n")
    run("git", "add", ".", cwd=agents)
    run("git", "commit", "-s", "-m", "Isolated canonical acme role", cwd=agents)
    run("git", "push", "origin", "HEAD:main", cwd=agents)
    (code / "hire-proof.json").write_text(json.dumps({"marker": "ACME_HIRE_PROOF_V1:42", "owner_key_sha256": hashlib.sha256(ENV["BEEKEEPER_PRIVATE_KEY"].encode()).hexdigest()}))
    run("git", "add", ".", cwd=code)
    run("git", "commit", "-s", "-m", "Deterministic hire fixture", cwd=code)
    run("git", "push", "origin", "HEAD:main", cwd=code)
    save("trust.json", bee("host", "trust-project", "--home", HOME, "--project", project, "--checkout", code, "--agents-repo", agents))
    install_args = ("sessions", "hire-host", "install-role", "--operation-id", str(uuid.uuid4()), "--home", HOME,
                    "--project", project, "--channel", CHANNEL, "--role", "builder", "--name", "Acme builder",
                    "--provider-instance", "acme-primary", "--model", model, "--parallelism", "2", "--templates", ROOT / "personas/templates")
    installed = bee(*install_args)
    save("role-install.json", installed)
    actor = installed["agent_pubkey"]
    assert bee(*install_args)["agent_pubkey"] == actor
    save("role-start.json", bee("agents", "start", "--operation-id", str(uuid.uuid4()), "--agent-pubkey", actor,
        "--home", HOME, "--channel", CHANNEL, "--display-name", "Acme builder",
        "--system-prompt", "Follow the canonical role pack staged for this responsibility.",
        "--cwd", code, "--project", project, "--provider-instance", "acme-primary", "--model", model, "--wait-secs", "60"))
    session = str(uuid.uuid4())
    template = save("genesis-template.json", {"kind": 44226, "tags": [["h", CHANNEL], ["csg-v", "csg1-1"], ["csg-session", session]], "content": json.dumps({"sessionRef": session, "v": 1})})
    genesis = json.loads(run("node", ROOT / "conformance/foreign-client/foreign-hire-publish.mjs", template, "genesis.json"))["event_id"]
    state = HOME / ".local/state/buzz/host/service/session-provider" / provider
    save("umbrella-create.json", bee("sessions", "create", "--channel", CHANNEL, "--session-ref", session, "--genesis", genesis,
         "--provider-instance", "acme-primary", "--provider-authority", provider, "--model", model,
         "--project", project, "--cwd", code, "--projects-file", state / "projects.json", "--wait", "--timeout-secs", "60"))
    scope = ("--channel", CHANNEL, "--session-ref", session, "--genesis", genesis)
    policy = save("policy.json", {"enabled": True, "allowedRoles": ["builder"], "maxSeatsPerUmbrella": 2, "allowedProviderInstanceRefs": ["acme-primary"]})
    policy.chmod(0o600)
    listener_args = [BEE, "sessions", "hire-host", "run", "--home", str(HOME), "--project", project,
                     "--channel", CHANNEL, "--genesis", genesis, "--actor", actor, "--provider-instance", "acme-primary",
                     "--model", model, "--policy-file", str(policy), "--isolated-cutover", "--wait-secs", "2"]
    fault_bin = WORK / "fault-bin"
    fault_bin.mkdir()
    shutil.copyfile(ROOT / "conformance/acme-acp/hire_git_fault.py", fault_bin / "git")
    (fault_bin / "git").chmod(0o700)
    listener_env = dict(ENV, PATH=str(fault_bin) + os.pathsep + ENV["PATH"], ACME_HIRE_REAL_GIT=shutil.which("git", path=ENV["PATH"]), ACME_HIRE_FAULT_DIR=str(WORK))
    def start(label):
        global LISTENER
        output = open(WORK / f"listener-{label}.log", "w")
        LISTENER = subprocess.Popen(listener_args, env=listener_env, stdout=output, stderr=subprocess.STDOUT)
        output.close()
    disabled = save("disabled-policy.json", {"enabled": False, "allowedRoles": ["builder"], "maxSeatsPerUmbrella": 2, "allowedProviderInstanceRefs": ["acme-primary"]})
    disabled.chmod(0o600)
    # The ordinary requesting CLI must receive a real refusal, even when the
    # owner's publish ACK is lost. No earlier refusal can satisfy this request.
    listener_args[listener_args.index("--policy-file") + 1] = str(disabled)
    mode("lose-refusal-ack")
    start("disabled-refusal")
    refused_cli = subprocess.run([BEE,"sessions","hire",*scope,"--role","builder","--content","Disabled policy must permanently refuse"], env=ENV, capture_output=True,text=True,timeout=40)
    (WORK / "requester-refusal.log").write_text(refused_cli.stdout + refused_cli.stderr)
    refused_body = json.loads(refused_cli.stdout)
    assert refused_body["outcome"] == "refused", refused_body
    assert "HIRE_OFF" in refused_cli.stdout
    until(lambda: (WORK / "proxy-refusal-ack-lost").exists(), "refusal ACK dropped")
    stop_listener()
    refused_wire = json.loads((WORK / "proxy-refusal.json").read_text())
    assert refused_wire["pubkey"] == OWNER and ["buzz-host-answer","hire"] in refused_wire["tags"]
    listener_args[listener_args.index("--policy-file") + 1] = str(policy)
    mode("pass")
    enabled = subprocess.run(listener_args + ["--once"],env=listener_env,capture_output=True,text=True,timeout=60)
    (WORK / "enabled-refused-replay.log").write_text(enabled.stdout + enabled.stderr)
    assert 'policy_refused' in enabled.stdout and 'HIRE_OFF' in enabled.stdout
    assert not [e for e in read([44221],"disabled-replay-commands.json") if json.loads(e["content"])["action"].get("hireRef")]
    disclosures = read([44220],"refusal-events.json")
    assert [e["id"] for e in disclosures] == [refused_wire["id"]]
    rejected = ["disabled-policy-permanent-requester-refusal"]
    # A real channel member without hire authority gets no owner-signed answer.
    stranger_env = dict(ENV, FOREIGN_CLIENT_KEY=secrets.token_hex(32))
    stranger_pubkey = subprocess.check_output(["node","--input-type=module","-e","import {getPublicKey} from './_common.mjs'; console.log(getPublicKey(Uint8Array.from(Buffer.from(process.env.FOREIGN_CLIENT_KEY,'hex'))));"],cwd=ROOT / "conformance/foreign-client",env=stranger_env,text=True).strip()
    bee("channels","add-member","--channel",CHANNEL,"--pubkey",stranger_pubkey)
    stranger_command = str(uuid.uuid4())
    original_request = next(e for e in read([44221],"disabled-hire-record.json") if e["id"] == refused_body["event_id"])
    stranger_payload = json.loads(original_request["content"])
    stranger_payload["commandId"] = stranger_command
    stranger_payload["action"]["requestedBy"] = stranger_pubkey
    stranger_payload["action"]["brief"] = "Unauthorized member must not trigger owner disclosure"
    stranger_template = save("stranger-hire-template.json", {"kind":44221,"tags":[["h",CHANNEL],["csl-v","csl1-1"],["csl-command",stranger_command]],"content":json.dumps(stranger_payload)})
    stranger_publish = subprocess.run(["node",str(ROOT / "conformance/foreign-client/foreign-hire-publish.mjs"),str(stranger_template),"stranger-hire.json"],env=stranger_env,capture_output=True,text=True,timeout=30)
    (WORK / "stranger-publication.log").write_text(stranger_publish.stdout + stranger_publish.stderr)
    assert stranger_publish.returncode != 0 and "only the session founder" in stranger_publish.stderr
    unauthorized = subprocess.run(listener_args + ["--once"],env=listener_env,capture_output=True,text=True,timeout=60)
    (WORK / "unauthorized-requester.log").write_text(unauthorized.stdout + unauthorized.stderr)
    assert unauthorized.returncode == 0
    assert [e["id"] for e in read([44220],"after-stranger-disclosures.json")] == [refused_wire["id"]]
    rejected.append("unauthorized-requester-no-owner-disclosure")

    # Each bad listener/selection case gets its own request. The normal disabled
    # listener seals any request that a misconfigured/unauthorized caller could
    # not answer, before the next request can be considered.
    def reject(label, option=None, replacement=None, env=None, dirty=False):
        bee("sessions","hire",*scope,"--role","builder","--content","Rejection matrix " + label,"--no-wait")
        args = list(listener_args)
        if option: args[args.index(option) + 1] = replacement
        if dirty: (code / "dirty-marker").write_text("reject dirty checkout")
        result = subprocess.run(args + ["--once"],env=env or listener_env,capture_output=True,text=True,timeout=60)
        (WORK / f"rejection-{label}.log").write_text(result.stdout + result.stderr)
        assert result.returncode != 0 or '"unanswered"' in result.stdout or 'policy_refused' in result.stdout, (label,result.stdout)
        assert '"ready"' not in result.stdout
        if dirty: (code / "dirty-marker").unlink()
        seal = list(listener_args); seal[seal.index("--policy-file")+1] = str(disabled)
        sealed = subprocess.run(seal + ["--once"],env=listener_env,capture_output=True,text=True,timeout=60)
        assert sealed.returncode == 0
        rejected.append(label)
    reject("unavailable-model", "--model", "not-offered")
    reject("unavailable-runtime", "--provider-instance", "not-commissioned")
    reject("wrong-project", "--project", f"30621:{OWNER}:absent")
    reject("wrong-actor", "--actor", "f" * 64)
    reject("unknown-genesis", "--genesis", "e" * 64)
    stranger = dict(listener_env, BEEKEEPER_PRIVATE_KEY=secrets.token_hex(32))
    stranger.pop("NOSTR_PRIVATE_KEY",None)
    reject("unauthorized-owner",env=stranger)
    reject("dirty-checkout",dirty=True)
    assert not [e for e in read([44221],"before-admission.json") if json.loads(e["content"])["action"].get("hireRef")]
    # Only this NEW request can launch after enabling policy.
    mode("hold-create")
    request = bee("sessions","hire",*scope,"--role","builder","--content","ACME_HIRE_PROOF_V1 wait for canonical assignment","--no-wait")
    save("hire-request.json",request)
    git_crashes = []
    for crash in ("crash-code","crash-agents"):
        (WORK / "git-fault-mode").write_text(crash)
        start(crash)
        until(lambda: (WORK / (crash + ".json")).exists(), crash + " after exact Git step")
        until(lambda: LISTENER.poll() is not None, "listener killed at Git boundary")
        stop_listener()
        assert not (WORK / "proxy-create.json").exists()
        reserved = next(json.loads(p.read_text()) for p in (HOME / ".local/state/buzz/host/management/operations").glob("hire-*.json") if json.loads(p.read_text())["request"]["request"]["id"] == request["event_id"])
        plan_at_crash = reserved["hire"]["plan"]
        missing = pathlib.Path(plan_at_crash["worktree"]) / "hire-proof.json" if crash == "crash-code" else pathlib.Path(plan_at_crash["agents_checkout"]) / "roles/builder.md"
        assert not missing.exists()
        git_crashes.append({"step":crash,"create":plan_at_crash["create"]["id"],"tree":plan_at_crash["worktree"],"agents":plan_at_crash["agents_checkout"],"missing_file":str(missing)})
    assert git_crashes[0]["create"] == git_crashes[1]["create"]
    assert git_crashes[0]["tree"] == git_crashes[1]["tree"]
    save("git-crash-evidence.json",git_crashes)
    (WORK / "git-fault-mode").write_text("pass")
    start("prepared")
    until(lambda: (WORK / "proxy-create.json").exists(), "durable staged create intercepted")
    staged = json.loads((WORK / "proxy-create.json").read_text())
    stop_listener()
    mode("lose-create-ack")
    start("ack-lost")
    until(lambda: (WORK / "proxy-ack-lost").exists(), "accepted create ACK dropped")
    stop_listener()
    mode("reject-grant")
    start("grant-failure")
    until(lambda: (WORK / "proxy-grant.json").exists(), "grant repair attempted")
    until(lambda: 'created_awaiting_grants' in (WORK / "listener-grant-failure.log").read_text(), "truthful incomplete grant disposition")
    stop_listener()
    mode("lose-grant-ack")
    start("grant-ack-lost")
    until(lambda: (WORK / "proxy-grant-ack-lost").exists(), "accepted grant ACK dropped")
    stop_listener()
    mode("pass")
    start("reconciled")
    def ready():
        path = WORK / "listener-reconciled.log"
        return '"ready"' in path.read_text().lower()
    until(ready, "receipt-backed grants ready")
    events = read([44221, 44223, 44224, 44228, 40099], "ready-events.json")
    creates = [e for e in events if e["kind"] == 44221 and json.loads(e["content"])["action"].get("hireRef")]
    assert len(creates) == 1, creates
    assert creates[0]["id"] == staged["id"] == git_crashes[0]["create"]
    assert creates[0]["pubkey"] == OWNER
    create = json.loads(creates[0]["content"])
    assert create["action"]["actor"] == actor
    with urllib.request.urlopen(urllib.request.Request(relay, headers={"Accept": "application/nostr+json"}), timeout=10) as response:
        relay_self = json.load(response)["self"]
    grants = [e for e in events if e["kind"] == 44228 and json.loads(e["content"])["granteePubkey"] == actor]
    assert {json.loads(e["content"])["type"] for e in grants} == {"grant-operator", "grant-seat"}
    for grant in grants:
        assert grant["pubkey"] == OWNER
        assert any(e["kind"] == 40099 and e["pubkey"] == relay_self and json.loads(e["content"]).get("acceptedEventId") == grant["id"] for e in events)
    assert any(e["kind"] == 44224 and e["pubkey"] == provider and json.loads(e["content"]).get("commandId") == create["commandId"] and json.loads(e["content"]).get("status") == "created" for e in events)
    save("exact-create.json", creates[0])
    # Reconcile exact signed request bytes from two same-host processes.
    requests = read([44221], "request-history.json")
    original = next(e for e in requests if e["id"] == request["event_id"])
    replay = save("exact-hire.json", original)
    run("node", ROOT / "conformance/foreign-client/foreign-hire-publish.mjs", replay, "duplicate-hire.json")
    concurrent = subprocess.run(listener_args + ["--once"], env=ENV, capture_output=True, text=True, timeout=60)
    (WORK / "concurrent-listener.log").write_text(concurrent.stdout + concurrent.stderr)
    assert concurrent.returncode == 0
    mutated = dict(original)
    for key in ("sig", "id", "pubkey"):
        mutated.pop(key)
    payload = json.loads(mutated["content"])
    payload["action"]["brief"] += " changed under same command ID"
    mutated["content"] = json.dumps(payload)
    template = save("changed-command-template.json", mutated)
    changed = subprocess.run(["node", str(ROOT / "conformance/foreign-client/foreign-hire-publish.mjs"), str(template), "changed-command.json"], env=ENV, capture_output=True, text=True, timeout=30)
    (WORK / "changed-command-publication.log").write_text(changed.stdout + changed.stderr)
    # Either the relay's immutable-command fence or the journal rejects it.
    if changed.returncode == 0:
        until(lambda: 'different request' in (WORK / "listener-reconciled.log").read_text().lower(), "changed request rejected")
    journals = list((HOME / ".local/state/buzz/host/management/operations").glob("hire-*.json"))
    operation = next(json.loads(p.read_text()) for p in journals if json.loads(p.read_text())["events"] and json.loads(p.read_text())["events"][0]["id"] == creates[0]["id"])
    plan = operation["hire"]["plan"]
    seat = pathlib.Path(plan["worktree"])
    pack = pathlib.Path(plan["pack_dir"])
    assert run("git", "rev-parse", "HEAD", cwd=seat) == run("git", "rev-parse", "HEAD", cwd=code)
    assert any("ACME_HIRE_SKILL_V1" in p.read_text() for p in pack.rglob("SKILL.md"))
    assert pathlib.Path(plan["agents_checkout"]).is_dir()
    save("materialization-evidence.json", {"actor": actor, "tree": str(seat), "pack": str(pack), "agents_checkout": plan["agents_checkout"], "pack_ref": plan["pack_ref"], "create": creates[0]["id"]})
    assignment = save("assignment-body.json", {"assigneeActor": actor, "assigneeRole": "builder", "objective": "Report fixture marker 42",
        "brief": "ACME_HIRE_PROOF_V1 read hire-proof.json and report marker=42", "branch": None, "baseSha": None,
        "fileOwnership": ["hire-proof.json"], "acceptanceSteps": ["Read marker 42 and publish actor report"]})
    assigned = bee("sessions", "assign", *scope, "--body", "@" + str(assignment), "--wake-to", "builder")
    save("assignment.json", assigned)
    def report():
        rows = read([44244], "work-events.json")
        return [e for e in rows if e["pubkey"] == actor and "marker=42" in e["content"]]
    reports = until(report, "actor-signed canonical report after software wake", 120)
    assert len(reports) == 1
    report_body = json.loads(reports[0]["content"])
    assert report_body["type"] == "report" and report_body["body"]["assignmentRef"] == assigned["event_id"]
    assert report_body["genesisRef"] == genesis and reports[0]["pubkey"] != OWNER
    target = next(json.loads(line)["disposition"]["target"] for line in (WORK / "listener-reconciled.log").read_text().splitlines() if '"ready"' in line)
    receipts = until(lambda: [e for e in read([44224], "delivery-receipts.json") if json.loads(e["content"]).get("commandId") == assigned["delivery"]["commandId"] and json.loads(e["content"]).get("turnId")], "signed software delivery receipt")
    assert all(e["pubkey"] == provider for e in receipts)
    # Independent terminal transcript and metadata, including C4 shared projection.
    turn_ids = {json.loads(e["content"]).get("turnId") for e in receipts}
    assert None not in turn_ids
    def terminal():
        events = read([44223, 44225, 40099], "terminal-events.json")
        return [e for e in events if e["kind"] == 44225 and json.loads(e["content"]).get("session") == target
                and json.loads(e["content"]).get("turnId") in turn_ids
                and json.loads(e["content"]).get("item", {}).get("kind") == "result"]
    results = until(terminal, "signed terminal result for exact assignment turn")
    assert all(e["pubkey"] == provider for e in results)
    assert all(not json.loads(e["content"])["item"].get("isError", False) for e in results)
    stopped = bee("sessions", "stop", "--channel", CHANNEL, "--session", target["sessionId"], "--provider-authority", provider,
                  "--expected-target", json.dumps(target), "--wait", "--timeout-secs", "60")
    assert stopped["outcome"] == "stopped"
    save("stopped.json", stopped)
    stop_listener()
    historical = subprocess.run(listener_args + ["--once"], env=ENV, capture_output=True, text=True, timeout=60)
    (WORK / "stopped-hire-replay.log").write_text(historical.stdout + historical.stderr)
    assert historical.returncode == 0
    final_creates = [e for e in read([44221], "final-commands.json") if json.loads(e["content"])["action"].get("hireRef")]
    assert [e["id"] for e in final_creates] == [creates[0]["id"]]
    # A new request after stop may reuse the durable actor, with a new tree.
    next_request = bee("sessions", "hire", *scope, "--role", "builder", "--content", "New bounded responsibility after stop", "--no-wait")
    start("new-responsibility")
    def next_ready():
        lines = (WORK / "listener-new-responsibility.log").read_text().splitlines()
        return next((json.loads(line)["disposition"]["target"] for line in lines if '"ready"' in line and next_request["event_id"] in line), None)
    next_target = until(next_ready, "new responsibility after quiescent stop")
    stop_listener()
    assert next_target != target
    new_creates = [e for e in read([44221], "rehire-commands.json") if json.loads(e["content"])["action"].get("hireRef")]
    assert len(new_creates) == 2
    assert all(json.loads(e["content"])["action"]["actor"] == actor for e in new_creates)
    save("rehire-stop.json", bee("sessions", "stop", "--channel", CHANNEL, "--session", next_target["sessionId"], "--provider-authority", provider,
         "--expected-target", json.dumps(next_target), "--wait", "--timeout-secs", "60"))
    # Canonically stop the identity, then prove provider refusal is terminal
    # and releases selection for a distinct request without deleting custody.
    save("identity-stop.json", bee("agents", "stop", "--operation-id", str(uuid.uuid4()), "--agent-pubkey", actor, "--channel", CHANNEL, "--home", HOME, "--wait-secs", "60"))
    for index in range(2):
        refused_request = bee("sessions", "hire", *scope, "--role", "builder", "--content", "Stopped identity must refuse", "--no-wait")
        label = f"terminal-refusal-{index}"
        start(label)
        until(lambda: any('ACTOR_OWNERSHIP_UNAVAILABLE' in line and refused_request["event_id"] in line for line in (WORK / f"listener-{label}.log").read_text().splitlines()), "provider-signed stopped identity refusal")
        stop_listener()
    refused_events = read([44221, 44224], "terminal-refusals.json")
    assert len([e for e in refused_events if e["kind"] == 44221 and json.loads(e["content"])["action"].get("hireRef")]) == 4
    (agents / "roles/builder.md").write_text((agents / "roles/builder.md").read_text() + "\nSource moved after installation.\n")
    run("git", "add", ".", cwd=agents)
    run("git", "commit", "-s", "-m", "Exercise stale installed source refusal", cwd=agents)
    run("git", "push", "origin", "HEAD:main", cwd=agents)
    stale = bee("sessions", "hire", *scope, "--role", "builder", "--content", "Stale installed source must refuse", "--no-wait")
    check = subprocess.run(listener_args + ["--once"], env=ENV, capture_output=True, text=True, timeout=60)
    (WORK / "stale-source.log").write_text(check.stdout + check.stderr)
    assert any(stale["event_id"] in line and "HIRE_STALE" in line for line in check.stdout.splitlines())
    rejected.append("stale-installed-source")
    save("rejection-evidence.json", rejected)
    save("acceptance.json", {"transport_pipeline": "PASS", "full_mission_verification": "not exercised", "actor": actor,
         "project": project, "session": session, "genesis": genesis, "create_event": creates[0]["id"], "report_event": reports[0]["id"],
         "faults": ["code worktree crash before checkout", "agents clone crash before checkout", "disabled refusal ACK lost and enabled replay", "restart after staging before publish", "create ACK lost", "grant publication failure", "accepted grant ACK lost", "duplicate request", "concurrent same-host listener", "changed command ID body", "stop and old hire replay"], "model": "deterministic acme stub; no native resume"})
    print("PASS signed hire/create/receipt/grants/assignment/software-wake/actor-report", flush=True)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    try:
        main()
    finally:
        stop_listener()
