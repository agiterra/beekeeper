"""Opt-in deterministic assignment reporting for the isolated hire fixture.

Runs inside the ACP worker, with its actor-scoped bee credentials. Never imports
an owner key or signs a report in the management test driver.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess


def report_assignment(params, cwd):
    text = "\n".join(item.get("text", "") for item in params.get("prompt", []) if isinstance(item, dict))
    if "ACME_HIRE_PROOF_V1" not in text:
        return None
    scope = re.search(r"--channel ([0-9a-f-]{36}) --session-ref ([0-9a-f-]{36}) --genesis ([0-9a-f]{64})", text)
    assignment = re.search(r"`assignmentRef` is ([0-9a-f]{64})", text)
    if not scope or not assignment:
        return None  # The hire's initial brief is not a canonical assignment.
    root = pathlib.Path(cwd)
    proof = json.loads((root / "hire-proof.json").read_text())
    assert proof["marker"] == "ACME_HIRE_PROOF_V1:42"
    assert not any(hashlib.sha256(value.encode()).hexdigest() == proof["owner_key_sha256"] for value in os.environ.values()), "owner credential reached worker"
    marker = root / (".acme-reported-" + assignment[1])
    if marker.exists():
        return "ACME_HIRE_PROOF_V1 assignment already reported"
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    body = {"assignmentRef": assignment[1], "summary": "ACME_HIRE_PROOF_V1 marker=42; actor-scoped report; owner credential absent",
            "branch": None, "baseSha": head, "headSha": head, "files": [], "tests": [],
            "redBeforeGreen": None, "deviations": [], "residuals": ["Deterministic stub; no real model or native resume"], "anomalies": []}
    bee = os.environ.get("BEE", "bee")
    result = subprocess.run([bee, "sessions", "report", "--channel", scope[1], "--session-ref", scope[2],
                             "--genesis", scope[3], "--body", "-"], input=json.dumps(body), text=True,
                            capture_output=True, cwd=root, timeout=60)
    if result.returncode:
        raise RuntimeError("actor report refused: " + result.stderr[-1200:])
    response = json.loads(result.stdout)
    marker.write_text(json.dumps({"assignment": assignment[1], "response": response}))
    return body["summary"]
