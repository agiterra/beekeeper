#!/usr/bin/env python3
"""Independent, read-only host-step transcript checker (Python standard library).

This checks the selected run_on_host exchange, not relay membership or admission.
The arithmetic is a reference verifier, unsuitable for custody/signing in a service.
"""

import hashlib
import json
import re
import sys
import uuid

P = 2**256 - 2**32 - 977
N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
G = (0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
     0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8)
SCHEMA = "buzz-host-step/v1"


def tagged_hash(tag, data):
    digest = hashlib.sha256(tag.encode()).digest()
    return hashlib.sha256(digest + digest + data).digest()


def add(a, b):
    if a is None:
        return b
    if b is None:
        return a
    x, y = a
    u, v = b
    if x == u and (y != v or y == 0):
        return None
    slope = ((3 * x * x) * pow(2 * y, -1, P) if a == b
             else (v - y) * pow(u - x, -1, P)) % P
    z = (slope * slope - x - u) % P
    return z, (slope * (x - z) - y) % P


def mul(k, point=G):
    result = None
    while k:
        if k & 1:
            result = add(result, point)
        point = add(point, point)
        k >>= 1
    return result


def hex_value(value, length):
    if not isinstance(value, str) or not re.fullmatch(f"[0-9a-f]{{{length}}}", value):
        raise ValueError("invalid lowercase hex")
    return bytes.fromhex(value)


def verify_event(event):
    pubkey = hex_value(event["pubkey"], 64)
    signature = hex_value(event["sig"], 128)
    message = hex_value(event["id"], 64)
    canonical = json.dumps([0, event["pubkey"], event["created_at"], event["kind"],
                            event["tags"], event["content"]],
                           ensure_ascii=False, separators=(",", ":")).encode()
    if hashlib.sha256(canonical).digest() != message:
        raise ValueError("event id mismatch")
    x = int.from_bytes(pubkey, "big")
    if x >= P:
        raise ValueError("invalid public key")
    y = pow((x**3 + 7) % P, (P + 1) // 4, P)
    if y * y % P != (x**3 + 7) % P:
        raise ValueError("invalid public key")
    point = (x, y if y % 2 == 0 else P - y)
    r, s = int.from_bytes(signature[:32], "big"), int.from_bytes(signature[32:], "big")
    if r >= P or s >= N:
        raise ValueError("signature scalar out of range")
    challenge = int.from_bytes(tagged_hash("BIP0340/challenge",
                                          signature[:32] + pubkey + message), "big") % N
    recovered = add(mul(s), mul((N - challenge) % N, point))
    if recovered is None or recovered[1] % 2 or recovered[0] != r:
        raise ValueError("invalid Schnorr signature")
    return json.loads(event["content"])


def closed(record, required, optional=()):
    if not isinstance(record, dict) or not set(required) <= record.keys():
        raise ValueError("missing required payload field")
    if record.keys() - set(required) - set(optional):
        raise ValueError("unknown payload field")


def common(record):
    if record["schema"] != SCHEMA:
        raise ValueError("unsupported schema")
    for name in ("runId", "channelId"):
        if str(uuid.UUID(record[name])) != record[name]:
            raise ValueError("noncanonical UUID")
    if not re.fullmatch("[A-Za-z0-9_]{1,64}", record["stepId"]):
        raise ValueError("invalid step id")


def tags(event, expected):
    # Core compares the exact multiset, so order alone is not a rejection.
    if sorted(event["tags"]) != sorted(expected):
        raise ValueError("tags disagree with content")


def exchange(transcript):
    """Validate one correlated, already-admitted claim/result exchange.

    witnessedRelayPubkey must come from a trusted NIP-11 identity, not the event.
    acceptedClaimEventId must come from the relay's admission receipt/state.
    """
    relay = transcript["witnessedRelayPubkey"]
    hex_value(relay, 64)
    request, claim, result = (transcript[name] for name in ("request", "claim", "result"))
    q, c, r = (verify_event(event) for event in (request, claim, result))
    if [request["kind"], claim["kind"], result["kind"]] != [46013, 46022, 46023]:
        raise ValueError("unsupported exchange kinds")
    if request["pubkey"] != relay:
        raise ValueError("request not signed by witnessed relay")
    closed(q, ("schema", "runId", "workflowId", "workflowName", "stepId", "stepIndex",
               "definitionHash", "stepKind", "channelId", "project", "triggerContext",
               "expiresAt"), ("approval", "inputs"))
    closed(c, ("schema", "runId", "stepId", "requestedEventId", "channelId"), ("host",))
    closed(r, ("schema", "runId", "stepId", "requestedEventId", "channelId", "disposition",
               "timedOut"), ("claimEventId", "exitCode", "refusal", "durationMs", "headSha",
                             "agentsCommit", "dirty", "checkout", "stdoutTail", "stderrTail",
                             "truncated", "artifactPath", "routed", "artifacts"))
    for record in (q, c, r):
        common(record)
    if str(uuid.UUID(q["workflowId"])) != q["workflowId"]:
        raise ValueError("noncanonical workflow UUID")
    hex_value(q["definitionHash"], 64)
    if q["stepKind"] != "run_on_host":
        raise ValueError("unsupported action: this client checks run_on_host only")
    project = q["project"].split(":", 2)
    if (len(project) != 3 or project[0] != "30621"
            or not re.fullmatch("[0-9a-f]{64}", project[1])
            or not 1 <= len(project[2]) <= 64
            or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in project[2])):
        raise ValueError("invalid project coordinate")
    if not isinstance(q.get("inputs", {}), dict):
        raise ValueError("inputs must be an object")
    if "approval" in q:
        closed(q["approval"], ("approvalRef", "scope"))
        hex_value(q["approval"]["approvalRef"], 64)
        if q["approval"]["scope"] not in ("run", "action"):
            raise ValueError("invalid approval scope")
    if "host" in c:
        closed(c["host"], (), ("name",))
    if claim["created_at"] >= q["expiresAt"]:
        raise ValueError("claim after request expiry")
    key = f'{q["runId"]}:{q["stepId"]}'
    base = [["d", key], ["h", q["channelId"]], ["schema", SCHEMA]]
    tags(request, base + [["a", q["project"]]])
    for event, record in ((claim, c), (result, r)):
        if any(record[name] != q[name] for name in ("runId", "stepId", "channelId")):
            raise ValueError("responsibility correlation mismatch")
        if record["requestedEventId"] != request["id"]:
            raise ValueError("request correlation mismatch")
        tags(event, base + [["e", request["id"]]])
    if transcript["acceptedClaimEventId"] != claim["id"]:
        raise ValueError("claim was not recorded as admitted")
    if result["pubkey"] != claim["pubkey"] or r.get("claimEventId") != claim["id"]:
        raise ValueError("result does not belong to winning claim")
    disposition = r["disposition"]
    if disposition == "exited":
        if type(r.get("exitCode")) is not int or r["timedOut"] or "refusal" in r:
            raise ValueError("invalid exit result")
    elif disposition == "timed_out":
        if not r["timedOut"] or "refusal" in r:
            raise ValueError("invalid timeout result")
    elif disposition == "lost_on_restart":
        if "exitCode" in r or "refusal" in r:
            raise ValueError("lost result cannot assert an exit")
    elif disposition == "refused":
        closed(r.get("refusal"), ("code", "message"))
        if "exitCode" in r:
            raise ValueError("refusal cannot assert an exit")
    else:
        raise ValueError("unsupported disposition")
    for name in ("stdoutTail", "stderrTail"):
        if len(r.get(name, "").encode()) > 65536:
            raise ValueError("output tail too large")
    return {"runId": q["runId"], "requestedEventId": request["id"],
            "resultEventId": result["id"], "disposition": disposition,
            "unsupported": ["membership admission", "execution", "crash takeover",
                            "exactly-once external effects"]}


if __name__ == "__main__":
    try:
        with open(sys.argv[1], encoding="utf-8") as source:
            print(json.dumps(exchange(json.load(source))))
    except (ValueError, KeyError, TypeError, OSError, IndexError) as error:
        print(f"refused: {error}", file=sys.stderr)
        sys.exit(1)
