#!/usr/bin/env python3
"""Ordinary Host conversation and scripted model-chosen delegation; no real model."""
import hashlib
import json
import secrets
import subprocess
import time
import uuid
import hire_acceptance as f


def main():
    # The independent foreign signer uses second-resolution NIP-98 tokens.
    # Space identical reads so the relay replay guard is exercised honestly.
    foreign_read = f.read
    def read(kinds, name):
        time.sleep(1.05)
        return foreign_read(kinds, name)
    f.read = read
    slug = "chat-" + uuid.uuid4().hex[:12]
    project = f"30621:{f.OWNER}:{slug}"
    relay = f.ENV["BEEKEEPER_RELAY_URL"]
    provider = (f.WORK / "provider.pubkey").read_text().strip()
    config = json.loads((f.HOME / ".local/state/buzz/host/host.json").read_text())
    model = config["runtimes"][0]["allowedModels"][0]
    f.save("project-create.json", f.bee("projects", "create", slug, "--channel", f.CHANNEL))
    agents, code = f.WORK / "agents", f.WORK / "code"
    f.run("git", "clone", f"{relay}/git/{f.OWNER}/{slug}-beekeeper-agents", agents)
    f.run("git", "clone", f"{relay}/git/{f.OWNER}/{slug}", code)
    (agents / "team.yml").write_text(f"schema: beekeeper-team/v1\nname: chat-proof\nversion: 0.1.0\nlead: lead\nroles:\n  lead:\n    runtime: acme\n    model: {model}\n    workspace: {{ agents_repo: read }}\n  runner:\n    runtime: acme\n    model: {model}\n    workspace: {{ agents_repo: read }}\n")
    for role in ("lead", "runner"):
        (agents / f"roles/{role}.md").write_text(f"---\nname: {role}\nrole: {role}\n---\nConverse about this project, clarify intent and choose whether a bounded delegation helps. Use existing session operations and qualify evidence.\n")
    f.run("git", "add", ".", cwd=agents)
    f.run("git", "commit", "-s", "-m", "Chat fixture roles", cwd=agents)
    f.run("git", "push", "origin", "HEAD:main", cwd=agents)
    (code / "hire-proof.json").write_text(json.dumps({"marker": "ACME_HIRE_PROOF_V1:42", "owner_key_sha256": hashlib.sha256(f.ENV["BEEKEEPER_PRIVATE_KEY"].encode()).hexdigest()}))
    (code / "inspection.txt").write_text("A bounded project fact: marker 42.\n")
    f.run("git", "add", ".", cwd=code)
    f.run("git", "commit", "-s", "-m", "Chat fixture bytes", cwd=code)
    f.run("git", "push", "origin", "HEAD:main", cwd=code)
    f.save("trust.json", f.bee("host", "trust-project", "--home", f.HOME, "--project", project, "--checkout", code, "--agents-repo", agents))
    actors = {}
    for role in ("lead", "runner"):
        installed = f.bee("sessions", "hire-host", "install-role", "--operation-id", str(uuid.uuid4()), "--home", f.HOME,
                          "--project", project, "--channel", f.CHANNEL, "--role", role, "--name", "Chat " + role,
                          "--provider-instance", "acme-primary", "--model", model, "--parallelism", "2", "--templates", f.ROOT / "personas/templates")
        f.save(role + "-install.json", installed)
        actors[role] = installed["agent_pubkey"]
    # Both participants use the existing Host, not another chat responder.
    f.save("runner-start.json", f.bee("agents", "start", "--operation-id", str(uuid.uuid4()), "--agent-pubkey", actors["runner"],
        "--home", f.HOME, "--channel", f.CHANNEL, "--display-name", "Chat runner", "--system-prompt", "Follow the canonical role pack staged for this responsibility.",
        "--cwd", code, "--project", project, "--provider-instance", "acme-primary", "--model", model, "--wait-secs", "60"))
    session = str(uuid.uuid4())
    found = f.bee("sessions", "found", "--home", f.HOME, "--operation-id", str(uuid.uuid4()), "--channel", f.CHANNEL, "--session-ref", session)
    f.save("found.json", found)
    genesis = found["event"]
    state = f.HOME / ".local/state/buzz/host/service/session-provider" / provider
    f.save("umbrella-create.json", f.bee("sessions", "create", "--channel", f.CHANNEL, "--session-ref", session, "--genesis", genesis["id"],
        "--provider-instance", "acme-primary", "--provider-authority", provider, "--model", model, "--project", project, "--cwd", code,
        "--projects-file", state / "projects.json", "--wait", "--timeout-secs", "60"))
    for verb, role in (("grant", "collaborator"), ("grant-seat", "lead")):
        f.save(verb + ".json", f.bee("sessions", verb, "--channel", f.CHANNEL, "--genesis", genesis["id"], "--pubkey", actors["lead"], "--role", role))
    (code / "chat-fixture.json").write_text(json.dumps({"model": model, "runner": actors["runner"]}))
    f.run("git", "add", ".", cwd=code)
    f.run("git", "commit", "-s", "-m", "Public conversation fixture coordinates", cwd=code)
    f.run("git", "push", "origin", "HEAD:main", cwd=code)
    f.save("lead-start.json", f.bee("agents", "start", "--operation-id", str(uuid.uuid4()), "--agent-pubkey", actors["lead"],
        "--home", f.HOME, "--channel", f.CHANNEL, "--display-name", "Chat lead", "--system-prompt", "Follow the canonical role pack staged for this responsibility.",
        "--cwd", code, "--project", project, "--provider-instance", "acme-primary", "--model", model, "--wait-secs", "60"))
    policy = f.save("policy.json", {"enabled": True, "allowedRoles": ["runner"], "maxSeatsPerUmbrella": 2, "allowedProviderInstanceRefs": ["acme-primary"]})
    policy.chmod(0o600)
    command = [f.BEE, "sessions", "hire-host", "run", "--home", str(f.HOME), "--project", project, "--channel", f.CHANNEL,
               "--genesis", genesis["id"], "--lead", actors["lead"], "--actor", actors["runner"], "--provider-instance", "acme-primary", "--model", model,
               "--policy-file", str(policy), "--isolated-cutover", "--wait-secs", "1", "--deadline-secs", "30"]
    output = (f.WORK / "chat-consumer.log").open("a")
    f.LISTENER = subprocess.Popen(command, env=f.ENV, stdout=output, stderr=subprocess.STDOUT)
    output.close()
    def poll():
        assert f.LISTENER.poll() is None, "owner hire supervisor exited"
    stranger_env = dict(f.ENV, FOREIGN_CLIENT_KEY=secrets.token_hex(32))
    stranger = subprocess.check_output(["node", "--input-type=module", "-e", "import {getPublicKey} from './_common.mjs'; console.log(getPublicKey(Uint8Array.from(Buffer.from(process.env.FOREIGN_CLIENT_KEY,'hex'))));"], cwd=f.ROOT / "conformance/foreign-client", env=stranger_env, text=True).strip()
    f.bee("channels", "add-member", "--channel", f.CHANNEL, "--pubkey", stranger)
    template = f.save("stranger-template.json", {"kind": 9, "tags": [["h", f.CHANNEL], ["p", actors["lead"]]], "content": "Could you explain this project and choose whether to delegate?"})
    subprocess.run(["node", str(f.ROOT / "conformance/foreign-client/foreign-hire-publish.mjs"), str(template), "stranger-chat.json"], env=stranger_env, check=True, capture_output=True, text=True, timeout=30)
    before = f.read([44221], "before-chat.json")
    poll()
    assert len(f.read([44221], "after-stranger.json")) == len(before), "membership became execution authority"
    requests = []
    def ask(content, root=None, marker=None):
        args = ["messages", "send", "--channel", f.CHANNEL, "--content", content]
        if root: args.extend(["--reply-to", root])
        else: args.extend(["--mention", actors["lead"]])
        request = f.bee(*args)["event_id"]
        expected_root = root or request
        requests.append((expected_root, request))
        def answered():
            poll()
            rows = f.read([9, 40002], "chat-replies.json")
            return [e for e in rows if e["pubkey"] == actors["lead"] and marker in e["content"] and ["e", request, "", "reply"] in e["tags"]]
        reply = f.until(answered, marker, 150)[0]
        assert ["e", expected_root, "", "root"] in reply["tags"] or (expected_root == request and ["e", request, "", "reply"] in reply["tags"])
        return request, reply
    root, first = ask("Could you explain this project and suggest a useful check?", marker="Would you like a worker")
    _, delegated = ask("Yes, choose a bounded worker check and report the actual evidence.", root, "scripted ACP delegation")
    _, followup = ask("What remains unreviewed?", root, "scripted ACP follow-up")
    _, separate = ask("Please summarize without delegating.", marker="scripted ACP separate thread")
    replies = [first, delegated, followup, separate]
    rows = f.read([44221, 44220, 44244, 44224, 44225, 9, 40002], "before-replay.json")
    stranger_sources = {e["id"] for e in rows if e["pubkey"] == stranger and e["kind"] in [9,40002]}
    assert stranger_sources
    assert not [e for e in rows if e["pubkey"] == actors["lead"] and e["kind"] == 9 and any(t[0] == "e" and t[1] in stranger_sources for t in e["tags"])]
    hires = [e for e in rows if e["kind"] == 44221 and json.loads(e["content"])["action"]["type"] == "session.hire"]
    assert len(hires) == 1 and all(e["pubkey"] == actors["lead"] for e in hires)
    for _, request in requests:
        event = next(e for e in rows if e["id"] == request)
        path = f.save("replay.json", event)
        f.run("node", f.ROOT / "conformance/foreign-client/foreign-hire-publish.mjs", path, "replay-ack.json")
    def stopped():
        poll()
        return [e for e in f.read([44224], "stop-receipts.json") if e["pubkey"] == provider and json.loads(e["content"]).get("status") == "stopped"]
    stops = f.until(stopped, "bounded owner cleanup after ready", 60)
    f.stop_listener()
    restarted = subprocess.run(command + ["--once"], env=f.ENV, capture_output=True, text=True, timeout=45)
    (f.WORK / "restarted-listener.log").write_text(restarted.stdout + restarted.stderr)
    assert restarted.returncode == 0
    time.sleep(3)
    after = f.read([44221, 44220, 44244, 9, 40002], "after-replay.json")
    def effect_ids(events):
        return {e["id"] for e in events if e["kind"] in [44221,44220,44244,9,40002] and not (e["kind"] == 44221 and json.loads(e["content"])["action"]["type"] == "session.stop")}
    assert effect_ids(after) == effect_ids(rows), "replay created a second effect"
    work = [e for e in rows if e["kind"] == 44244]
    assignments = [e for e in work if json.loads(e["content"])["type"] == "assignment"]
    reports = [e for e in work if json.loads(e["content"])["type"] == "report"]
    assert len(assignments) == len(reports) == 1
    assert assignments[0]["pubkey"] == actors["lead"] and reports[0]["pubkey"] == actors["runner"]
    assigned = json.loads(assignments[0]["content"])
    assert json.loads(reports[0]["content"])["body"]["assignmentRef"] == assignments[0]["id"]
    assert requests[1][1] in assigned["body"]["brief"]
    receipts = [json.loads(e["content"]) for e in rows if e["kind"] == 44224 and e["pubkey"] == provider and json.loads(e["content"]).get("commandId") == assigned["deliveryCommandId"]]
    turns = {r.get("turnId") for r in receipts} - {None}
    terminals = [e for e in rows if e["kind"] == 44225 and e["pubkey"] == provider and json.loads(e["content"]).get("turnId") in turns]
    assert terminals, "assignment terminal missing"
    assert stops, "bounded owner stop missing"
    f.save("chat-result.json", {"passed": True, "requests": requests, "reply_ids": [e["id"] for e in replies], "hire_ids": [e["id"] for e in hires], "actors": actors, "desktop": "absent", "worker": "scripted ACP coordinator and worker; no model judgment; owner credential absence asserted in both"})
    print("PASS ordinary conversation + clarification + scripted delegation + follow-up + separate thread; unauthorized/restart/replay; no real model judgment")


if __name__ == "__main__":
    try:
        main()
    finally:
        f.stop_listener()
