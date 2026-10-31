"""Scripted ACP coordinator, never a production intent parser or model proof."""
import hashlib
import json
import os
import pathlib
import re
import subprocess
import time


def converse(params, cwd):
    root = pathlib.Path(cwd)
    path = root / "chat-fixture.json"
    if not path.exists():
        return None
    config = json.loads(path.read_text())
    text = "\n".join(p.get("text", "") for p in params.get("prompt", []) if isinstance(p, dict))
    source = re.search(r"Project conversation source ([0-9a-f]{64}) by ([0-9a-f]{64}) in channel ([0-9a-f-]{36}) thread ([0-9a-f]{64})", text)
    if not source:
        return None
    proof = json.loads((root / "hire-proof.json").read_text())
    assert not any(hashlib.sha256(v.encode()).hexdigest() == proof["owner_key_sha256"] for v in os.environ.values()), "owner credential reached coordinator"
    if "Could you explain this project" in text:
        return "This fixture project records a marker. I can check its recorded value myself or delegate a bounded check. Would you like a worker to check it? [scripted ACP conformance]"
    if "What remains unreviewed" in text:
        return "The worker reported a marker check; I compared the committed fixture myself. No independent code review or real model judgment occurred. [scripted ACP follow-up]"
    if "Please summarize without delegating" in text:
        return "This is a small fixture project; no worker is needed for this summary. [scripted ACP separate thread]"
    if "Yes, choose a bounded worker check" not in text:
        return None
    configured = re.search(r"Existing delegation scope: --channel ([0-9a-f-]{36}) --session-ref ([0-9a-f-]{36}) --genesis ([0-9a-f]{64})", text)
    assert configured, "owner-configured Host scope was not supplied"
    config["channel"] = configured[1]
    scope = ["--channel", configured[1], "--session-ref", configured[2], "--genesis", configured[3]]
    def bee(*args, body=None):
        p = subprocess.run([os.environ.get("BEE", "bee"), *args], input=json.dumps(body) if body else None,
                           text=True, capture_output=True, cwd=root, timeout=90)
        if p.returncode and not (p.returncode == 5 and '"accepted":true' in p.stdout.replace(" ", "")):
            raise RuntimeError(p.stderr[-1500:] + p.stdout[-500:])
        return json.loads(p.stdout)
    # Protocol script decides to delegate here; production has no such branch.
    provenance = f"Source {source[1]} author {source[2]} channel {source[3]} thread {source[4]}"
    hire = bee("sessions", "hire", *scope, "--role", "runner", "--provider-instance", "acme-primary", "--model", config["model"],
               "--content", provenance + ". Wait for the canonical assignment; do not modify files.")
    deadline = time.monotonic() + 75
    while time.monotonic() < deadline:
        rows = bee("events", "query", "--kinds", "44228", "--channel", config["channel"])
        if len([e for e in rows if json.loads(e["content"]).get("granteePubkey") == config["runner"]]) >= 2:
            break
        time.sleep(1.1)
    else:
        raise RuntimeError("runner grants not observed")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    assignment = bee("sessions", "assign", *scope, "--body", "-", "--wake-to", "runner", body={
        "assigneeActor": config["runner"], "assigneeRole": "runner", "objective": "Check the project's recorded marker",
        "brief": provenance + ". ACME_HIRE_PROOF_V1 read hire-proof.json and report the marker; no modifications.",
        "branch": None, "baseSha": head, "fileOwnership": ["hire-proof.json"], "acceptanceSteps": ["Read the committed fixture and report evidence"]})
    while time.monotonic() < deadline:
        rows = bee("events", "query", "--kinds", "44244", "--channel", config["channel"])
        reports = [e for e in rows if e["pubkey"] == config["runner"] and json.loads(e["content"]).get("type") == "report" and json.loads(e["content"])["body"].get("assignmentRef") == assignment["event_id"]]
        if reports:
            break
        time.sleep(1.1)
    else:
        raise RuntimeError("worker report absent")
    blob = subprocess.check_output(["git", "show", head + ":hire-proof.json"], cwd=root, text=True)
    assert json.loads(blob)["marker"] == "ACME_HIRE_PROOF_V1:42"
    return (f"Worker report {reports[0]['id']} for assignment {assignment['event_id']} claims marker=42. "
            f"I independently read the committed fixture at {head} and confirmed that value. "
            f"Hire {hire['event_id']}; source {source[1]}. No independent code review or arbitrary correctness claim; "
            "provider completion is execution evidence only. [scripted ACP delegation]")
