#!/usr/bin/env python3
"""Regenerate fixtures/fold-vectors.json from hand-stated expectations.

Every expectation below is written by hand from CONTRACT.md; this script
only assembles the long hex ids and the event envelopes. Run it from the
repo root after editing, then make all three folds pass the result — never
edit the expectation to match one implementation.
"""
import json, os
HERE = os.path.dirname(os.path.abspath(__file__))
P = "30621:" + "a" * 64 + ":tank-loop"
OTHER_P = "30621:" + "b" * 64 + ":other"
R = "30617:" + "a" * 64 + ":tank-loop-beekeeper-agents"
OTHER_R = "30617:" + "a" * 64 + ":tank-loop-packs"
ALICE = "1" * 64; BOB = "2" * 64
B1 = "1" * 40; B2 = "2" * 40; C1 = "c" * 40; C2 = "d" * 40
SCHEMA = "buzz-agents-repo-draft/v1"
LEAD = "roles/lead.md"; PLAN = "plans/rpg.md"; ARCH = "plans/archive/rpg.md"

def eid(n): return f"{n:064x}"

def put(path, text, base=None, baseCommit=None, prev=None, message=None):
    return {"schema": SCHEMA, "op": "file.put", "path": path, "text": text,
            "base": base, "baseCommit": baseCommit, "prev": prev, "message": message}
def move(path, to, base=None, baseCommit=None, prev=None, message=None):
    return {"schema": SCHEMA, "op": "file.move", "path": path, "to": to,
            "base": base, "baseCommit": baseCommit, "prev": prev, "message": message}
def delete(path, base=None, baseCommit=None, prev=None, message=None):
    return {"schema": SCHEMA, "op": "file.delete", "path": path,
            "base": base, "baseCommit": baseCommit, "prev": prev, "message": message}
def record(commit, paths, drafts, message=None):
    return {"schema": SCHEMA, "op": "commit.record", "commit": commit,
            "paths": paths, "drafts": drafts, "message": message}

def paths_of(c):
    if c["op"] == "commit.record": return c["paths"]
    if c["op"] == "file.move": return [c["path"], c["to"]]
    return [c["path"]]

def ev(n, pk, t, c, kind=44250, project=P, repo=R, tags=None, content_override=None):
    if tags is None:
        tags = [["a", project], ["ad-v", "ad1-1"], ["ad-op", c["op"]], ["ad-repo", repo]]
        tags += [["ad-path", p] for p in paths_of(c)]
    content = content_override if content_override is not None else json.dumps(c, separators=(",", ":"))
    return {"id": eid(n), "pubkey": pk, "created_at": t, "kind": kind, "tags": tags, "content": content}

def row(e):
    c = json.loads(e["content"])
    return {"id": e["id"], "author": e["pubkey"], "createdAt": e["created_at"], "op": c["op"],
            "path": c["path"], "to": c.get("to"), "text": c.get("text"), "base": c["base"],
            "baseCommit": c["baseCommit"], "prev": c["prev"], "message": c["message"]}
def dpath(path, head, superseded=(), diverged=False):
    return {"path": path, "head": row(head), "superseded": [row(s) for s in superseded],
            "diverged": diverged, "updatedAt": head["created_at"]}
def crow(e):
    c = json.loads(e["content"])
    return {"id": e["id"], "commit": c["commit"], "by": e["pubkey"], "createdAt": e["created_at"],
            "paths": c["paths"], "drafts": c["drafts"], "message": c["message"]}
def digest(paths, commits=(), ignored=0, otherRepo=0):
    return {"schema": "buzz-agents-repo-draft-digest/v1", "project": P, "repo": R,
            "ignored": ignored, "otherRepo": otherRepo, "paths": paths, "commits": list(commits)}

cases = []
cases.append({"name": "empty", "project": P, "repo": R, "events": [], "expected": digest([])})

e1 = ev(1, ALICE, 100, put(LEAD, "# lead v1\n", base=B1, baseCommit=C1, message="tighten"))
cases.append({"name": "one put is the head of its path", "project": P, "repo": R,
              "events": [e1], "expected": digest([dpath(LEAD, e1)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n", base=B1, baseCommit=C1))
e2 = ev(2, BOB, 110, put(LEAD, "v2\n", base=B1, baseCommit=C1, prev=eid(1)))
e3 = ev(3, ALICE, 120, put(LEAD, "v3\n", base=B1, baseCommit=C1, prev=eid(2)))
cases.append({"name": "a chain of saves: newest is head, the rest superseded oldest first, not diverged",
              "project": P, "repo": R, "events": [e3, e1, e2],
              "expected": digest([dpath(LEAD, e3, [e1, e2])])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n", base=B1))
e2 = ev(2, BOB, 110, put(LEAD, "bob\n", base=B1, prev=eid(1)))
e3 = ev(3, ALICE, 111, put(LEAD, "alice\n", base=B1, prev=eid(1)))
cases.append({"name": "two saves from the same prev: newer is head, older superseded, diverged",
              "project": P, "repo": R, "events": [e1, e2, e3],
              "expected": digest([dpath(LEAD, e3, [e1, e2], diverged=True)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n", base=B1))
e2 = ev(2, BOB, 100, put(LEAD, "v1 too\n", base=B1))
cases.append({"name": "a created_at tie breaks on event id; a head with prev null over an open op is diverged",
              "project": P, "repo": R, "events": [e2, e1],
              "expected": digest([dpath(LEAD, e2, [e1], diverged=True)])})

e1 = ev(1, ALICE, 100, put(PLAN, "plan\n"))
e2 = ev(2, ALICE, 105, delete(PLAN, base=B1, prev=eid(1)))
cases.append({"name": "a delete after a put is the head", "project": P, "repo": R,
              "events": [e1, e2], "expected": digest([dpath(PLAN, e2, [e1])])})

e1 = ev(1, ALICE, 100, move(PLAN, ARCH, base=B1, baseCommit=C1))
cases.append({"name": "a move is the head of both its path and its destination",
              "project": P, "repo": R, "events": [e1],
              "expected": digest([dpath(ARCH, e1), dpath(PLAN, e1)])})

e1 = ev(1, ALICE, 100, move(PLAN, ARCH, base=B1))
e2 = ev(2, BOB, 110, put(ARCH, "archived, annotated\n", base=B1, prev=eid(1)))
cases.append({"name": "a put at a move's destination builds on the move there and leaves the move as head at the source",
              "project": P, "repo": R, "events": [e1, e2],
              "expected": digest([dpath(ARCH, e2, [e1]), dpath(PLAN, e1)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n", base=B1))
e2 = ev(2, BOB, 110, put(PLAN, "plan\n"))
e3 = ev(3, ALICE, 120, record(C2, [LEAD], [eid(1)], message="landed"))
e4 = ev(4, BOB, 130, put(LEAD, "after commit\n", base=B2, baseCommit=C2))
cases.append({"name": "a record closes the drafts it names, leaves the others open, and a later put on the same path is a fresh head",
              "project": P, "repo": R, "events": [e1, e2, e3, e4],
              "expected": digest([dpath(PLAN, e2), dpath(LEAD, e4)], commits=[crow(e3)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n"))
e2 = ev(2, BOB, 110, put(LEAD, "v2\n", prev=eid(1)))
e3 = ev(3, ALICE, 120, record(C2, [LEAD], [eid(1)]))
cases.append({"name": "closing a superseded op leaves the head that built on it open and not diverged",
              "project": P, "repo": R, "events": [e1, e2, e3],
              "expected": digest([dpath(LEAD, e2)], commits=[crow(e3)])})

e1 = ev(1, ALICE, 100, record(C1, [LEAD], [eid(9)]))
e2 = ev(2, BOB, 200, record(C2, [PLAN], [eid(8)], message="second"))
cases.append({"name": "records list newest first and may name ids the fold never saw",
              "project": P, "repo": R, "events": [e1, e2],
              "expected": digest([], commits=[crow(e2), crow(e1)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n"))
e2 = ev(2, ALICE, 110, put(LEAD, "for the packs repo\n"), repo=OTHER_R)
e3 = ev(3, ALICE, 120, put(LEAD, "wrong project\n"), project=OTHER_P)
e4 = ev(4, ALICE, 130, put(LEAD, "wrong kind\n"), kind=44248)
e5 = ev(5, ALICE, 140, None, content_override="{\"schema\":\"buzz-agents-repo-draft/v1\",\"op\":\"file.put\"}",
        tags=[["a", P], ["ad-v", "ad1-1"], ["ad-op", "file.put"], ["ad-repo", R], ["ad-path", LEAD]])
e6 = ev(6, ALICE, 150, put(LEAD, "two a tags\n"),
        tags=[["a", P], ["a", P], ["ad-v", "ad1-1"], ["ad-op", "file.put"], ["ad-repo", R], ["ad-path", LEAD]])
e7 = ev(7, ALICE, 160, put(LEAD, "no repo tag\n"),
        tags=[["a", P], ["ad-v", "ad1-1"], ["ad-op", "file.put"], ["ad-path", LEAD]])
cases.append({"name": "another repository's op is counted, not folded; malformed, mis-kinded and mis-scoped ops are ignored",
              "project": P, "repo": R, "events": [e1, e2, e3, e4, e5, e6, e7],
              "expected": digest([dpath(LEAD, e1)], ignored=5, otherRepo=1)})

e1 = ev(1, ALICE, 100, put(LEAD, "case-variant a\n"), project=P.replace("aaaa", "AAAA", 1))
cases.append({"name": "a case-variant a tag folds as its canonical coordinate (ingest never stores one; a reader must not drop it)",
              "project": P, "repo": R, "events": [e1],
              "expected": digest([dpath(LEAD, e1)])})

e1 = ev(1, ALICE, 100, put(LEAD, "v1\n"))
dup = dict(e1); dup["content"] = json.dumps(put(LEAD, "dup with different text\n"), separators=(",", ":"))
cases.append({"name": "duplicate ids keep the first", "project": P, "repo": R,
              "events": [e1, dup], "expected": digest([dpath(LEAD, e1)])})

e1 = ev(1, BOB, 100, put("team.yml", "schema: beekeeper-team/v1\n", base=B1))
e2 = ev(2, ALICE, 90, put("actions.yml", "schema: buzz-project-actions/v1\n", base=B2))
e3 = ev(3, ALICE, 95, put("plans/a.md", "a\n"))
cases.append({"name": "paths sort bytewise regardless of op order", "project": P, "repo": R,
              "events": [e1, e2, e3],
              "expected": digest([dpath("actions.yml", e2), dpath("plans/a.md", e3), dpath("team.yml", e1)])})

out = {"schema": "buzz-agents-repo-draft-fold-vectors/v1", "cases": cases}
with open(os.path.join(HERE, "fixtures", "fold-vectors.json"), "w") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
print(f"wrote {len(cases)} cases")
