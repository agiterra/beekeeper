#!/usr/bin/env python3
"""Two isolated Hosts, outbound relay commands and signed scripted results.

Build bee, beekeeper-host, beekeeper-session-provider, beekeeper-relay and
 git-credential-nostr first. No installed Host, production key or model is used.
Reuses host-manage-check's isolated services and independent signature verifier.
"""
import argparse
import hashlib
import importlib.util
import json
import pathlib
import shutil
import signal
import time
import traceback
import uuid

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("host_manage_check", HERE / "host-manage-check.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


class Remote(base.Harness):
    def __init__(self, evidence):
        super().__init__(evidence)
        # Resolve the already-installed Node before changing HOME. A Hermit
        # shim under a fresh HOME would otherwise bootstrap another toolchain.
        node = pathlib.Path(base.out("node", "-p", "process.execPath"))
        self.env["PATH"] = str(node.parent) + ":" + self.env["PATH"]

    def require(self, name, condition, detail=""):
        self.check(name, condition, detail)
        if not condition:
            raise AssertionError(name + ": " + str(detail))

    def receipts(self, channel, provider, command):
        rows = [e for e in self.read([44224], "receipts.json", channel,
                {"authors": [provider]}) if json.loads(e["content"]).get("commandId") == command]
        self.save(f"receipts-{command}-{provider[:12]}.json", rows)
        return [json.loads(e["content"]) for e in rows]

    def create(self, channel, provider, operation, model):
        payload = {"schema": "buzz-coding-session-lifecycle-command/v1",
                   "commandId": operation, "action": {
                       "type": "session.create", "projectRef": None, "repoRef": None,
                       "sessionRef": None, "providerInstanceRef": "acme-primary",
                       "providerAuthorityPubkey": provider, "model": model,
                       "title": "Explicit remote scripted work", "initialTurn": None}}
        return self.publish_as(self.owner_key, {
            "kind": 44221, "tags": [["h", channel], ["csl-v", "csl1-1"],
                                    ["csl-command", operation]],
            "content": json.dumps(payload)}, operation)

    def replay(self, operation):
        signed = json.loads((self.work / "records" / (operation + ".json")).read_text())["event"]
        self.publish_as(self.owner_key, signed, "replay-" + operation)

    def redeliver(self, operation):
        event = json.loads((self.work / "records" / (operation + ".json")).read_text())["event"]
        # A new event ID forces delivery beyond relay event-id deduplication.
        fresh_id = self.publish_as(self.owner_key, {k: event[k] for k in ("kind", "tags", "content")},
                                   "redeliver-" + operation)
        self.require("redelivery has a fresh event ID: " + operation, fresh_id != event["id"])
        record = json.loads((self.work / "records" / ("redeliver-" + operation + ".json")).read_text())
        self.require("redelivery accepted: " + operation,
                     record["response"]["body"].get("accepted") is True)
        return fresh_id

    def run_remote(self, destination):
        b = destination
        self.require("distinct commissioned provider identities", self.provider != b.provider)
        channel = self.channel("remote-work")
        for provider in (self.provider, b.provider):
            self.owner_bee("channels", "add-member", "--channel", channel,
                           "--pubkey", provider, "--role", "bot")
        model = b.inventory()["runtimes"][0]["selectableModels"][0]["id"]
        unmapped = str(uuid.uuid4())
        self.create(channel, b.provider, unmapped, model)
        refusal = self.wait_for("unmapped scope refusal", lambda: next((r for r in
            self.receipts(channel, b.provider, unmapped) if r["status"] == "failed"), None), 60)
        self.require("unprovisioned channel cannot select a working directory",
                     (refusal.get("error") or {}).get("code") == "PROJECT_CWD_UNRESOLVED", refusal)
        self.save("scope-refusal.json", refusal)
        # Authorized local provisioning, before remote work. The sender does
        # not transmit a filesystem path; B resolves this channel locally.
        projects_path = b.state_dir() / "projects.json"
        projects = json.loads(projects_path.read_text()) if projects_path.exists() else {"version": 1}
        projects.setdefault("channels", {})[channel] = str(b.checkout)
        temporary = projects_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(projects))
        temporary.replace(projects_path)
        model = b.inventory()["runtimes"][0]["selectableModels"][0]["id"]
        operation = str(uuid.uuid4())
        self.create(channel, b.provider, operation, model)
        created = self.wait_for("destination signed create receipt", lambda: next((r for r in
            self.receipts(channel, b.provider, operation) if r["status"] == "created"), None), 90)
        self.save("created.json", created)
        target = created["session"]
        self.require("source did not execute destination create",
                     not self.receipts(channel, self.provider, operation))
        turn = str(uuid.uuid4())
        text = "relay-only harmless scripted proof " + turn
        payload = {"schema": "buzz-coding-session-command/v1", "commandId": turn,
                   "target": target, "action": {"type": "thread.turn.start", "text": text,
                                                "deliver": "steer"}}
        target_key = 'coding-session/v1|' + ''.join(
            f'{len(str(target[k]).encode())}:{target[k]}'
            for k in ("driver", "instanceId", "sessionId", "generation"))
        self.publish_as(self.owner_key, {"kind": 44220,
            "tags": [["h", channel], ["cs-v", "csc1-1"], ["cs-target", target_key]],
            "content": json.dumps(payload)}, turn)
        started = self.wait_for("signed turn_started", lambda: next((r for r in
            self.receipts(channel, b.provider, turn) if r["status"] == "turn_started"), None), 90)
        self.require("started receipt names selected execution", started.get("session") == target, started)
        turn_id = started["turnId"]
        def transcript():
            return [e for e in self.read([44225], "transcript.json", channel,
                    {"authors": [b.provider]}) if json.loads(e["content"]).get("session") == target
                    and json.loads(e["content"]).get("turnId") == turn_id]
        rows = self.wait_for("signed terminal result", lambda: (lambda rows: rows if any(
            json.loads(e["content"])["item"].get("kind") == "result" for e in rows) else None)(transcript()), 90)
        self.save("transcript.json", rows)
        items = [json.loads(e["content"])["item"] for e in rows]
        self.require("scripted adapter progress observed", "acme answered prompt 1" in json.dumps(items), items)
        results = [i for i in items if i.get("kind") == "result"]
        self.require("one signed result", len(results) == 1 and results[0].get("isError") is False
                     and results[0].get("subtype") == "success" and results[0].get("result") == "completed", results)
        self.require("scripted digest reaches source in signed progress",
                     "remote-work-sha256=" + hashlib.sha256(turn.encode()).hexdigest() in json.dumps(items))
        def effects():
            return [json.loads(line) for path in b.checkout.rglob("remote-work-proof.jsonl")
                    for line in path.read_text().splitlines()]
        self.require("destination performed exactly one durable effect", len(effects()) == 1, effects())
        self.save("turn-started.json", started)
        # Restart reconnects the same provider with its durable journal; replay
        # the EXACT signed create and turn, never a newly minted operation.
        baseline_transcript = self.read([44225], "before-replay.json", channel, {"authors": [b.provider]})
        b.stop_host()
        b.start_host({"RUST_LOG": "info,csp=debug"})
        self.replay(operation)
        self.replay(turn)
        fresh_ids = [self.redeliver(operation), self.redeliver(turn)]
        def dispatched():
            logs = b.home / ".local/state/buzz/host/service/session-provider/logs"
            lines = [line for path in logs.glob("*.log") for line in path.read_text().splitlines()
                     if "relay command dispatch completed" in line]
            return [line for line in lines if any(event_id in line for event_id in fresh_ids)]
        barrier = self.wait_for("each fresh envelope dispatched by destination",
            lambda: (lambda lines: lines if all(any(i in line for line in lines)
                for i in fresh_ids) else None)(dispatched()), 30)
        self.save("replay-dispatch-barrier.json", {"eventIds": fresh_ids, "lines": barrier})
        # Query the entire dedicated channel, including any new turn/session IDs.
        after = self.read([44225], "after-replay.json", channel, {"authors": [b.provider]})
        self.require("restart and exact replay produce no second transcript",
                     {e["id"] for e in after} == {e["id"] for e in baseline_transcript})
        self.require("restart replay did not repeat the scripted effect", len(effects()) == 1, effects())
        self.save("effects.json", effects())
        self.require("source never performed the scripted task",
                     not list(self.checkout.rglob("remote-work-proof.jsonl")))
        self.require("restart does not create another execution", len([r for r in
            self.receipts(channel, b.provider, operation) if r["status"] == "created"]) == 1)
        # A channel member without a session grant cannot steer this target.
        self.owner_bee("channels", "add-member", "--channel", channel,
                       "--pubkey", self.stranger, "--role", "member")
        denied = str(uuid.uuid4())
        denied_payload = dict(payload, commandId=denied)
        try:
            self.publish_as(self.stranger_key, {"kind": 44220,
                "tags": [["h", channel], ["cs-v", "csc1-1"], ["cs-target", target_key]],
                "content": json.dumps(denied_payload)}, denied)
        except base.subprocess.CalledProcessError:
            # A generic rejection does not establish authority enforcement.
            # Require the specific code; rate limits and malformed input fail this check.
            refusal = json.loads((self.work / "records" / (denied + ".json")).read_text())
            self.require("ungranted member refused by relay",
                         refusal["response"]["body"].get("accepted") is False
                         and refusal["response"]["body"].get("code") == "UNAUTHORIZED_OPERATOR",
                         {"authorityEstablished": False, "response": refusal["response"]})
            self.save("authority-refusal.json", refusal)
        else:
            refused = self.wait_for("signed ungranted turn refusal", lambda: next((r for r in
                self.receipts(channel, b.provider, denied) if r["status"] == "turn_refused"), None), 30)
            self.require("ungranted member gets an authority refusal",
                         (refused.get("error") or {}).get("code") == "UNAUTHORIZED_OPERATOR", refused)
            self.save("authority-refusal.json", refused)
        self.require("ungranted command causes no effect", len(effects()) == 1, effects())
        self.require("final command-wide transcript unchanged",
            {e["id"] for e in self.read([44225], "final-transcript.json", channel,
                {"authors": [b.provider]})} == {e["id"] for e in baseline_transcript})
        self.save("final-effects.json", effects())
        b.stop_host()
        absent = str(uuid.uuid4())
        self.create(channel, b.provider, absent, model)
        time.sleep(5)
        self.require("unavailable destination has no completion receipt",
                     not self.receipts(channel, b.provider, absent))
        self.save("unavailable.json", {"operation": absent, "observationSeconds": 5,
                                      "outcome": "unconfirmed", "completed": False})
        self.save("identities.json", {"sourceProvider": self.provider,
            "destinationProvider": b.provider, "channel": channel, "target": target,
            "scope": "standalone channel-owned session; no project or per-task filesystem grant",
            "transport": "outbound Nostr through isolated relay"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    a, b = Remote(args.evidence), Remote(args.evidence)
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("fixture deadline")))
    signal.alarm(600)
    print(f"source: {a.work}; destination: {b.work}", flush=True)
    try:
        a.start_services()
        a.owner_setup()
        b.relay_http = a.relay_http
        b.env["BEEKEEPER_RELAY_URL"] = a.relay_http
        # Same owner's authority, separate independently minted provider keys.
        b.owner_key, b.owner, b.key_file = a.owner_key, a.owner, b.work / "owner.key"
        b.key_file.write_text(b.owner_key + "\n")
        b.key_file.chmod(0o600)
        (b.work / "owner.env").write_text(f"BEEKEEPER_PRIVATE_KEY={b.owner_key}\n")
        commissioned = b.owner_bee("host", "commission", "--home", b.home,
            "--runtime", "acme", "--driver", base.STUB, "--provider-command", base.PROVIDER,
            "--runtime-install-root", base.ROOT / "conformance/acme-acp")
        b.provider = json.loads(commissioned)["provider_pubkey"]
        a.start_host()
        b.start_host()
        a.run_remote(b)
    except BaseException:
        error = traceback.format_exc()
        print(error, flush=True)
        a.check("fixture completed", False, error)
    finally:
        signal.alarm(0)
        b.teardown()
        a.teardown()
        for label, h in (("source", a), ("destination", b)):
            if (h.work / "evidence").exists():
                shutil.copytree(h.work / "evidence", args.evidence / label, dirs_exist_ok=True)
            # Records contain signed public events and responses, not signing keys.
            if (h.work / "records").exists():
                shutil.copytree(h.work / "records", args.evidence / (label + "-wire"), dirs_exist_ok=True)
            logs = h.home / ".local/state/buzz/host/service/session-provider/logs"
            if logs.exists():
                shutil.copytree(logs, args.evidence / (label + "-provider-logs"), dirs_exist_ok=True)
            for name in ("host.log", "relay.log"):
                if (h.work / name).exists():
                    shutil.copy(h.work / name, args.evidence / (label + "-" + name))
        summary = {"checks": a.results, "passed": sum(r["ok"] for r in a.results),
                   "failed": sum(not r["ok"] for r in a.results),
                   "binarySha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                       for p in (base.BEE, base.HOST, base.PROVIDER, base.RELAY)}}
        (args.evidence / "results.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps({k: summary[k] for k in ("passed", "failed")}), flush=True)
    return int(bool(summary["failed"]))


if __name__ == "__main__":
    raise SystemExit(main())
