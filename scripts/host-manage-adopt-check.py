#!/usr/bin/env python3
"""No-model acceptance of `adopt-identity` on a real relay, Host and provider with the stub runtime.

An identity built to look like one the Host only remembers: key and attestation in
place, no setup, no ownership row, no route, an old session still open. Custody is
already there, so the adoption reads no key store. The store's own paths are tested
with a fake in `adopt_identity_tests.rs`. No real Keychain is touched here.

HM_BIN_DIR names frozen binaries, as for the other host-manage checks.
"""
import hashlib, importlib.util, json, pathlib, secrets, sys, time, uuid

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("recovery", HERE / "host-manage-recovery-check.py")
mod = importlib.util.module_from_spec(spec); sys.modules["recovery"] = mod; spec.loader.exec_module(mod)
h = mod.Recovery(None)

# BIP-340, for fixture keys only: to age a journaled event, which no product clock can do.
_P = 2**256 - 2**32 - 977
_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
_G = (0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798, 0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8)
def _add(a, b):
    if a is None: return b
    if b is None: return a
    if a[0] == b[0] and (a[1] + b[1]) % _P == 0: return None
    l = (3 * a[0] * a[0] * pow(2 * a[1], -1, _P) if a == b else (b[1] - a[1]) * pow(b[0] - a[0], -1, _P)) % _P
    x = (l * l - a[0] - b[0]) % _P
    return (x, (l * (a[0] - x) - a[1]) % _P)
def _mul(k, p):
    r = None
    while k:
        if k & 1: r = _add(r, p)
        p = _add(p, p); k >>= 1
    return r
def _tagged(tag, msg):
    h = hashlib.sha256(tag.encode()).digest()
    return hashlib.sha256(h + h + msg).digest()
def resign(event, key_hex, created_at):
    """The same event at another time: a new id and signature, by the same key."""
    d = int(key_hex, 16); pub = _mul(d, _G)
    if pub[1] % 2: d = _N - d
    px = pub[0].to_bytes(32, "big")
    event = dict(event, created_at=created_at)
    body = json.dumps([0, event["pubkey"], created_at, event["kind"], event["tags"], event["content"]], separators=(",", ":"), ensure_ascii=False)
    mid = hashlib.sha256(body.encode()).digest(); event["id"] = mid.hex()
    k = int.from_bytes(_tagged("BIP0340/nonce", secrets.token_bytes(32) + px + mid), "big") % _N
    r = _mul(k, _G)
    if r[1] % 2: k = _N - k
    rx = r[0].to_bytes(32, "big")
    e = int.from_bytes(_tagged("BIP0340/challenge", rx + px + mid), "big") % _N
    event["sig"] = (rx + ((k + e * d) % _N).to_bytes(32, "big")).hex()
    return event

def fixture_hex(key):
    """A fixture's key file may hold an nsec; the signing client wants hex."""
    if not key.startswith("nsec1"):
        return key
    charset = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    acc = bits = 0; out = bytearray()
    for c in key[5:-6]:
        acc = (acc << 5) | charset.index(c); bits += 5
        while bits >= 8:
            bits -= 8; out.append((acc >> bits) & 255)
    return out.hex()

NAME = "Keystone Fixture"
failures = []
def check(what, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + what, "" if ok else str(detail)[:400], flush=True)
    if not ok:
        failures.append(what)

def state():
    return json.loads((h.state_dir() / "state.json").read_text())
def ownership():
    return json.loads((h.home / ".local/state/buzz/host/managed-ownership.json").read_text())
def routes(actor):
    path = h.state_dir() / "conversation-routes.json"
    return [r for r in json.loads(path.read_text())["routes"] if r["actorPubkey"] == actor]
def receipts(command):
    out = []
    for line in open(h.state_dir() / "outbox.jsonl"):
        x = json.loads(line)
        if x["op"] != "enqueue":
            continue
        ev = x["entry"]["event"]; ev = json.loads(ev) if isinstance(ev, str) else ev
        if ev["kind"] == 44224:
            j = json.loads(ev["content"])
            if j["commandId"] == command:
                out.append((j["status"], j["session"]["sessionId"], (j.get("error") or {}).get("code")))
    return out

def receipts_for_session(session):
    out = []
    for line in open(h.state_dir() / "outbox.jsonl"):
        x = json.loads(line)
        if x["op"] != "enqueue":
            continue
        ev = x["entry"]["event"]; ev = json.loads(ev) if isinstance(ev, str) else ev
        if ev["kind"] == 44224:
            j = json.loads(ev["content"])
            if (j.get("session") or {}).get("sessionId") == session:
                out.append((j["status"], j["commandId"][:12]))
    return out

try:
    h.start_services(); h.owner_setup(); h.start_host()
    made = h.started(NAME)
    actor, project, old_channel = made["actor"], made["project"], made["channel"]
    old = [s for s in state()["sessions"].values() if s.get("actor") == actor][0]

    # Reduce it to what an identity from before the Host looks like.
    path = h.home / ".local/state/buzz/host/managed-ownership.json"
    def remembered(made_row, name):
        who = made_row["actor"]
        h.stop_host()
        (h.state_dir() / "agent-setups" / f"{made_row['id']}.json").unlink()
        (h.state_dir() / "actors" / who / "enrollment.json").unlink(missing_ok=True)
        held = json.loads(path.read_text())
        held["transfers"] = [t for t in held["transfers"] if t["actorPubkey"] != who]
        held["history"] = [t for t in held["history"] if t["actorPubkey"] != who]
        path.write_text(json.dumps(held))
        rp = h.state_dir() / "conversation-routes.json"
        rows = json.loads(rp.read_text()); rows["routes"] = [r for r in rows["routes"] if r["actorPubkey"] != who]
        rp.write_text(json.dumps(rows))
        import shutil
        for scope in (h.state_dir() / "conversation-projects").glob(f"{who}*"):
            shutil.rmtree(scope) if scope.is_dir() else scope.unlink()
        h.secrets.append((h.state_dir() / "actors" / who / "key").read_text().strip())
        h.start_host()
        head = sorted(h.definitions(name), key=lambda e: e["created_at"])[-1]
        content = json.loads(head["content"]); content["model"] = None; content["persona_id"] = "builtin:fizz"
        h.publish_as(h.owner_key, {"kind": 30177, "content": json.dumps(content), "tags": head["tags"]}, f"unpinned-{who[:6]}")
        return content
    content = remembered(made, NAME)
    unpinned_event = sorted(h.definitions(NAME), key=lambda e: e["created_at"])[-1]
    unpinned = unpinned_event["id"]
    def head_id():
        return sorted(h.definitions(NAME), key=lambda e: e["created_at"])[-1]["id"]
    def agent_row(who):
        return next(a for a in h.inventory()["agents"] if a["actor"] == who)

    # B3: a Host's own live agent is not adopted, and nothing about it changes.
    other = h.started("Other Lead")
    other_open = agent_row(other["actor"])["openSessions"]
    code, _, message = h.manage("adopt-identity", "--pubkey", other["actor"])
    check("adopt refuses an identity this Host already runs", code != 0 and "IDENTITY_ALREADY_ENROLLED" in message, message)
    check("adopt: it wrote nothing for that identity", not (h.state_dir() / "actors" / other["actor"] / "adoption.json").exists())
    check("adopt: its open count is unchanged", agent_row(other["actor"])["openSessions"] == other_open)

    # B1: the same identity on another provider, visible on the relay, never on this Host.
    foreign_key = secrets.token_hex(32)
    foreign = h.identity(foreign_key)
    h.secrets.append(foreign_key)
    tag = json.loads(h.raw("agents", "attest", foreign, key=h.owner_key).stdout)
    tag = tag["tag"] if isinstance(tag, dict) and "tag" in tag else tag
    h.publish_as(foreign_key, {"kind": 0, "content": json.dumps({"name": "another host"}), "tags": [tag]}, "foreign-profile")
    h.owner_bee("channels", "add-member", "--channel", old_channel, "--pubkey", foreign)
    real = sorted(h.read([44223], "old-meta.json", channel=old_channel, extra={"authors": [h.provider]}), key=lambda e: e["created_at"])[-1]
    meta = json.loads(real["content"])
    foreign_session = str(uuid.uuid4())
    meta["session"] = dict(meta["session"], instanceId="foreignprovider0", sessionId=foreign_session, generation=1)
    t = meta["session"]
    def netstrings(domain):
        return domain + "|" + "".join(f"{len(f)}:{f}" for f in (t["driver"], t["instanceId"], t["sessionId"], str(t["generation"])))
    h.publish_as(foreign_key, {"kind": 44223, "content": json.dumps(meta), "tags": [["h", old_channel], ["csm-v", "csm1-1"], ["cs-target", netstrings("coding-session/v1")], ["csm-key", netstrings("coding-session-metadata/v1")]]}, "foreign-session")

    channel = h.channel(f"k{uuid.uuid4().hex[:8]}")
    def plan_spec(name=NAME, chan=channel, model=made["model"], existing=actor):
        body = json.loads(h.spec(project, chan, "lead", name, model))
        if existing:
            body["existingActor"] = existing
        return json.dumps(body)

    code, _, message = h.manage("plan", "--spec-file", "-", stdin=plan_spec())
    check("plan refuses an identity not yet adopted", code != 0 and "EXISTING_IDENTITY_NOT_ADOPTED" in message, message)
    code, _, message = h.manage("plan", "--spec-file", "-", stdin=plan_spec(existing=None))
    check("without existingActor the identity is still a duplicate", code != 0 and "IDENTITY_ALREADY_EXISTS" in message, message)

    # Delta 1's blocker: history in another channel it cannot authorize. A provider with
    # no profile, and an owner's create that never got an answer.
    unattested_key = secrets.token_hex(32)
    unattested = h.identity(unattested_key)
    h.secrets.append(unattested_key)
    h.owner_bee("channels", "add-member", "--channel", old_channel, "--pubkey", unattested)
    meta2 = json.loads(real["content"])
    meta2["session"] = dict(meta2["session"], instanceId="unattestedprovid", sessionId=str(uuid.uuid4()), generation=1)
    t = meta2["session"]
    h.publish_as(unattested_key, {"kind": 44223, "content": json.dumps(meta2), "tags": [["h", old_channel], ["csm-v", "csm1-1"], ["cs-target", netstrings("coding-session/v1")], ["csm-key", netstrings("coding-session-metadata/v1")]]}, "unattested-session")
    created = sorted((e for e in h.read([44221], "old-creates.json", channel=old_channel, extra={"authors": [h.owner]}) if '"session.create"' in e["content"]), key=lambda e: e["created_at"])[-1]
    orphan = json.loads(created["content"])
    orphan["commandId"] = str(uuid.uuid4())
    orphan["action"]["providerAuthorityPubkey"] = unattested
    h.publish_as(h.owner_key, {"kind": 44221, "content": json.dumps(orphan), "tags": [tag for tag in created["tags"] if tag[0] != "csl-command"] + [["csl-command", orphan["commandId"]]]}, "orphan-create")
    old_commands = {e["id"] for e in h.read([44221], "old-before.json", channel=old_channel)}

    code, adopted, message = h.manage("adopt-identity", "--pubkey", actor)
    check("adopt: custody already held, so no store read", code == 0 and adopted["keyStoreRead"] is False, message)
    check("adopt: the old session is the baseline", code == 0 and adopted["priorSessions"] == 1, adopted)
    code, again, message = h.manage("adopt-identity", "--pubkey", actor)
    check("adopt: repeating is the same answer", code == 0 and again["priorSessions"] == 1, message)
    assert code == 0
    code, _, message = h.manage("adopt-identity", "--pubkey", "ab" * 32)
    check("adopt: an identity this owner never defined refuses", code != 0 and "IDENTITY_NOT_OWNED" in message, message)

    code, _, message = h.manage("plan", "--spec-file", "-", stdin=plan_spec(name="Another Name"))
    check("plan refuses another name for the identity", code != 0 and "DEFINITION_MISMATCH" in message, message)

    code, planned, message = h.manage("plan", "--spec-file", "-", stdin=plan_spec())
    check("plan accepts the exact adopted identity", code == 0, message)
    check("refused plans changed no definition", head_id() == unpinned, head_id()[:8])

    # Refusals that must come before any definition change.
    actor_dir = h.state_dir() / "actors" / actor
    actor_key = fixture_hex((actor_dir / "key").read_text().strip())
    auth = json.loads((actor_dir / "auth-tag.json").read_text())
    h.publish_as(actor_key, {"kind": 0, "content": json.dumps({"name": NAME}), "tags": []}, "profile-bare")
    code, held, message = h.manage("apply", "--setup", planned["setupId"])
    check("a profile with no attestation refuses", "PROFILE_UNATTESTED" in (message + json.dumps(held or {})), message)
    check("PROFILE_UNATTESTED changed no definition", head_id() == unpinned)
    h.publish_as(actor_key, {"kind": 0, "content": json.dumps({"name": NAME}), "tags": [auth]}, "profile-back")
    historical = actor_dir / "enrollment.json"
    historical.write_text(json.dumps({"actorPubkey": actor, "nativeContextRequired": True}))
    historical.chmod(0o600)
    code, held, message = h.manage("apply", "--setup", planned["setupId"])
    check("a historical enrollment refuses", "HISTORICAL_ENROLLMENT" in (message + json.dumps(held or {})), message)
    check("HISTORICAL_ENROLLMENT changed no definition", head_id() == unpinned)
    historical.unlink()
    code, report, message = h.manage("preflight", "--spec-file", "-", stdin=plan_spec())
    row = next((c for c in (report or {}).get("checks", []) if c["code"] == "DEFINITION_WILL_CHANGE"), None)
    check("preflight says what the definition will lose", bool(row) and row["ok"] and unpinned in row["message"] and "builtin:fizz" in row["message"], row)
    code, applied, message = h.manage("apply", "--setup", planned["setupId"])
    check("apply runs", code == 0 and applied["state"] == "running", message or applied)
    if code or applied.get("state") != "running":
        for op in (h.home / ".local/state/buzz/host/management/operations").glob("*.json"):
            held = json.loads(op.read_text())
            print("op", op.name[:8], [(e["kind"], e["id"][:8], e["created_at"], [t[0] for t in e["tags"]][:4]) for e in held.get("events", [])], flush=True)
    check("apply: the same identity", applied.get("agentPubkey") == actor, applied.get("agentPubkey"))
    check("apply: context bound", applied.get("projectContext") == "bound", applied.get("projectContext"))
    check("one identity: no other session actor appeared", len({s.get("actor") for s in state()["sessions"].values() if s.get("channelId") == channel}) == 1)
    check("exactly one route", len(routes(actor)) == 1 and routes(actor)[0]["channelId"] == channel, routes(actor))
    defs = sorted({e["id"]: e for e in h.definitions(NAME)}.values(), key=lambda e: e["created_at"])
    check("the pin published a revision", defs[-1]["id"] != unpinned, defs[-1]["id"][:8])
    kept_path = h.state_dir() / "actors" / actor / "pre-adoption-definition.json"
    kept = json.loads(kept_path.read_text()) if kept_path.exists() else {}
    check("the prior definition is kept exactly", kept.get("id") == unpinned and kept.get("sig") == unpinned_event["sig"] and kept.get("content") == unpinned_event["content"], kept.get("id"))
    row = agent_row(actor)
    check("inventory separates current and prior", row["openSessions"] == 1 and row.get("priorOpenSessions") == 1 and row.get("priorTurnsInFlight") == 0, row)
    check("the definition now says the model", json.loads(defs[-1]["content"])["model"] == made["model"], defs[-1]["content"][:200])
    check("the persona link gave way to the role pack", json.loads(defs[-1]["content"]).get("persona_id") is None)
    check("name, role and project stay", all(json.loads(defs[-1]["content"]).get(k) == content.get(k) for k in ("name", "home_role", "project_digest", "respond_to")))
    new = [s for s in state()["sessions"].values() if s.get("actor") == actor and s["channelId"] == channel][0]
    check("no first turn", new["nativeHistoryStarted"] is False and new["openTurn"] is None)
    marker = [t for t in ownership()["transfers"] if t["actorPubkey"] == actor]
    check("ownership: honest marker", len(marker) == 1 and marker[0]["legacyNonce"].startswith("adopted-identity:") and marker[0]["phase"] == "adopted", marker)
    now = state()["sessions"][old["sessionId"]]
    check("old session untouched", now["closed"] == old["closed"] and now["nextSeq"] == old["nextSeq"] and now["generation"] == old["generation"])

    first = h.mention(actor, channel, "@Keystone one")
    h.answer_to(actor, channel, first, 60)
    check("a mention reaches the adopted identity", any(r[0] == "turn_started" and r[1] == new["sessionId"] for r in receipts(first)), receipts(first))

    # B2: a bad record refuses everything that would act on the old sessions.
    record_path = h.state_dir() / "actors" / actor / "adoption.json"
    record_bytes = record_path.read_bytes()
    stops_before = len(h.read([44221], "before.json", channel=old_channel))
    record_path.write_text("{not json")
    code, held, message = h.manage("stop", "--setup", planned["setupId"])
    refused_stop = code != 0 or (held or {}).get("state") == "needs_attention"
    check("a bad record: Stop refuses", refused_stop and "ADOPTION_RECORD_UNREADABLE" in (message + json.dumps(held or {})), (code, message, held))
    check("a bad record: Stop signed nothing in the old channel", len(h.read([44221], "after.json", channel=old_channel)) == stops_before)
    code, _, message = h.manage("adopt-identity", "--pubkey", actor)
    check("a bad record: adopt refuses and does not rewrite it", code != 0 and "ADOPTION_RECORD_UNREADABLE" in message and record_path.read_text() == "{not json", message)
    code, _, message = h.manage("preflight", "--spec-file", "-", stdin=plan_spec())
    code2, report, message2 = h.manage("preflight", "--spec-file", "-", stdin=plan_spec())
    check("a bad record: preflight refuses", any("ADOPTION_RECORD_UNREADABLE" in c["message"] for c in (report or {}).get("checks", []) if not c["ok"]), (message2, report))
    record_path.write_bytes(record_bytes)

    code, stopped, message = h.manage("stop", "--setup", planned["setupId"])
    check("stop", code == 0 and stopped["state"] == "stopped", message)
    foreign_stops = [r for r in h.read([44221], "stops.json", channel=old_channel) if foreign_session in r["content"]]
    check("stop signed nothing for the other provider's session", not foreign_stops, foreign_stops)
    check("stop signed nothing for the old session", not [r for r in h.read([44221], "stops2.json", channel=old_channel) if old["sessionId"] in r["content"] and "session.stop" in r["content"]])
    code, started, message = h.manage("start", "--setup", planned["setupId"])
    check("start, past the old open session", code == 0 and started["state"] == "running", message or started)
    current = [s for s in state()["sessions"].values() if s.get("actor") == actor and s["channelId"] == channel and not s["closed"]]
    check("one current execution", len(current) == 1 and current[0]["sessionId"] != new["sessionId"], current)
    held = ownership()
    check("journal: stopped in history, adopted current", any(t["phase"] == "hostStopped" and (t.get("target") or {}).get("sessionId") == new["sessionId"] and t["legacyNonce"].startswith("adopted-identity:") for t in held["history"]) and [t["target"]["sessionId"] for t in held["transfers"] if t["actorPubkey"] == actor] == [current[0]["sessionId"]], held)
    reply = json.loads(h.owner_bee("messages", "send", "--channel", channel, "--mention", actor, "--reply-to", first, "--content", "@Keystone two, in the first thread"))["event_id"]
    h.answer_to(actor, channel, reply, 60)
    check("the first thread reaches the current execution", any(r[0] == "turn_started" and r[1] == current[0]["sessionId"] for r in receipts(reply)), receipts(reply))
    now = state()["sessions"][old["sessionId"]]
    check("old session still untouched", now["closed"] == old["closed"] and now["nextSeq"] == old["nextSeq"] and now["generation"] == old["generation"], {k: (old.get(k), now.get(k)) for k in ("closed", "nextSeq", "generation", "channelId")})
    print("old-receipts", [(json.loads(json.dumps(r))) for r in receipts_for_session(old["sessionId"])], flush=True)

    # A Start that aged out, with the member gone: refuse, say how, then re-add and Retry.
    code, _, message = h.manage("stop", "--setup", planned["setupId"])
    setup_path = h.state_dir() / "agent-setups" / f"{planned['setupId']}.json"
    ops = h.home / ".local/state/buzz/host/management/operations"
    stopped_owner = path.read_bytes()
    code, _, message = h.manage("start", "--setup", planned["setupId"])
    assert code == 0, message
    code, _, message = h.manage("stop", "--setup", planned["setupId"])
    assert code == 0, message
    path.write_bytes(stopped_owner)
    setup = json.loads(setup_path.read_text())
    start_path = ops / f"{setup['startOperation']}.json"
    op = json.loads(start_path.read_text())
    aged_at = int(time.time()) - 2000
    op["events"] = [resign(e, h.owner_key, aged_at) if e["kind"] in (9000, 44221, 30177) and e["pubkey"] == h.owner else e for e in op["events"]]
    op["target"] = None; op["receiptEventId"] = None
    start_path.write_text(json.dumps(op))
    start_events = [e["id"] for e in op["events"]]
    setup.update(confirmedStart=None, pendingAction=None, state="stopped")
    setup_path.write_text(json.dumps(setup))
    def members():
        rows = h.read([39002], f"m-{uuid.uuid4().hex[:6]}.json", extra={"#d": [channel]})
        return bool(rows) and any(t[:2] == ["p", actor] for t in max(rows, key=lambda e: e["created_at"])["tags"])
    h.owner_bee("channels", "remove-member", "--channel", channel, "--pubkey", actor)
    code, held, message = h.manage("start", "--setup", planned["setupId"])
    words = message + json.dumps(held or {})
    check("aged Start, member gone: refused with the real remedy", "MEMBERSHIP_UNKNOWN" in words and "add-member" in words and "Retry" in words and "plan again" not in words, words[-300:])
    check("aged Start: the member was not added back", not members())
    check("aged Start: the old command is byte for byte the same", [e["id"] for e in json.loads(start_path.read_text())["events"]] == start_events)
    check("aged Start: nothing opened", not [s for s in state()["sessions"].values() if s.get("actor") == actor and s["channelId"] == channel and not s["closed"]])
    h.owner_bee("channels", "add-member", "--channel", channel, "--pubkey", actor)
    code, retried, message = h.manage("apply", "--setup", planned["setupId"])
    check("re-add and Retry: running", code == 0 and retried["state"] == "running", message or retried)
    check("re-add and Retry: the old command was not re-signed", [e["id"] for e in json.loads(start_path.read_text())["events"]] == start_events)
    check("re-add and Retry: the closed attempt is on record", any(a["action"] == "start" and "EXPIRED" in a["reason"] for a in (retried or {}).get("attempts", [])), (retried or {}).get("attempts"))
    check("re-add and Retry: one execution", len([s for s in state()["sessions"].values() if s.get("actor") == actor and s["channelId"] == channel and not s["closed"]]) == 1)

    # B5: an install journaled long ago is never re-signed for an existing identity.
    code, _, message = h.manage("stop", "--setup", planned["setupId"])
    setup_path = h.state_dir() / "agent-setups" / f"{planned['setupId']}.json"
    setup = json.loads(setup_path.read_text())
    op_path = h.home / ".local/state/buzz/host/management/operations" / f"{setup['installOperation']}.json"
    op = json.loads(op_path.read_text())
    aged = int(time.time()) - 2000
    op["events"] = [resign(e, h.owner_key, aged) if e["kind"] == 9000 and e["pubkey"] == h.owner else e for e in op["events"]]
    op_path.write_text(json.dumps(op))
    setup.update(agentPubkey=None, state="planned", pendingAction=None)
    setup_path.write_text(json.dumps(setup))
    def journal_events():
        return [e["id"] for e in json.loads(op_path.read_text())["events"]]
    op_events = journal_events()

    h.owner_bee("channels", "remove-member", "--channel", channel, "--pubkey", actor)
    code, resumed, message = h.manage("apply", "--setup", planned["setupId"])
    words = message + json.dumps(resumed or {})
    check("aged install, member removed: refused, and not added back", "MEMBERSHIP_UNKNOWN" in words, words[-300:])
    check("aged install: the journal was not re-signed", journal_events() == op_events)
    check("aged install: no start followed", [s["sessionId"] for s in state()["sessions"].values() if s.get("actor") == actor and s["channelId"] == channel and not s["closed"]] == [], "an execution opened")

    h.owner_bee("channels", "add-member", "--channel", channel, "--pubkey", actor)
    head = sorted(h.definitions(NAME), key=lambda e: e["created_at"])[-1]
    h.publish_as(h.owner_key, {"kind": 30177, "content": head["content"], "tags": head["tags"]}, "owner-revision")
    owner_head = head_id()
    code, resumed, message = h.manage("apply", "--setup", planned["setupId"])
    words = message + json.dumps(resumed or {})
    check("aged install, owner revised: refused", "OWNER_CHANGED_DEFINITION" in words, words[-300:])
    check("aged install: the owner's revision stands", head_id() == owner_head, head_id()[:8])
    check("aged install: the journal is still untouched", journal_events() == op_events)

    check("nothing was signed in the old channel", {e["id"] for e in h.read([44221], "old-after.json", channel=old_channel)} == old_commands)

    # Delta 1's blocker: cancel, plan another, resume the first.
    second = h.started("Keystone Second")
    remembered(second, "Keystone Second")
    code, _, message = h.manage("adopt-identity", "--pubkey", second["actor"])
    check("second identity adopted", code == 0, message)
    def second_spec(chan):
        body = json.loads(h.spec(second["project"], chan, "lead", "Keystone Second", second["model"]))
        body["existingActor"] = second["actor"]
        return json.dumps(body)
    channel_a, channel_b = h.channel(f"a{uuid.uuid4().hex[:8]}"), h.channel(f"b{uuid.uuid4().hex[:8]}")
    code, plan_a, message = h.manage("plan", "--spec-file", "-", stdin=second_spec(channel_a))
    check("A planned", code == 0, message)
    code, cancelled, message = h.manage("cancel", "--setup", plan_a["setupId"])
    residual = (cancelled or {}).get("residual", {})
    check("cancel tells the truth: no claim, a new setup may start", residual.get("identityReserved") is None and "may start" in residual.get("next", "") and residual.get("published") is False and residual.get("definitionChanged") is False, residual)
    code, plan_b, message = h.manage("plan", "--spec-file", "-", stdin=second_spec(channel_b))
    check("what cancel said is true: B plans", code == 0, message)
    code, applied_b, message = h.manage("apply", "--setup", plan_b["setupId"])
    check("B runs", code == 0 and applied_b["state"] == "running", message)
    path_a = h.state_dir() / "agent-setups" / f"{plan_a['setupId']}.json"
    journal_a = path_a.read_bytes()
    def in_channel(chan, who):
        rows = h.read([39002], f"m-{uuid.uuid4().hex[:6]}.json", extra={"#d": [chan]})
        return bool(rows) and any(t[:2] == ["p", who] for t in max(rows, key=lambda e: e["created_at"])["tags"])
    head_b = sorted(h.definitions("Keystone Second"), key=lambda e: e["created_at"])[-1]["id"]
    code, _, message = h.manage("apply", "--setup", plan_a["setupId"], "--resume-cancelled")
    check("resuming A over B refuses", code != 0 and "SETUP_ALREADY_EXISTS" in message, message)
    check("A's journal is unchanged", path_a.read_bytes() == journal_a)
    check("A's channel got no membership", not in_channel(channel_a, second["actor"]))
    check("A published no definition", sorted(h.definitions("Keystone Second"), key=lambda e: e["created_at"])[-1]["id"] == head_b)
    check("B still runs", h.setup_row(plan_b["setupId"])["effectiveState"] == "running")
    code, again, message = h.manage("cancel", "--setup", plan_a["setupId"])
    residual = (again or {}).get("residual", {})
    check("cancel names a published definition change", residual.get("published") is True and residual.get("definitionChanged") is True and "already revised" in residual.get("next", ""), residual)

    h.secrets = [s for s in h.secrets if s]
    h.scan_secrets()
finally:
    h.teardown()
print("RESULT", "PASS" if not failures else f"FAIL {failures}", flush=True)
sys.exit(1 if failures else 0)
