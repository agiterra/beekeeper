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
ALICE = "1" * 64
BOB = "2" * 64
SCHEMA = "buzz-project-artifact-pin/v1"
DOC = "docs/mockups/login.html"
PLAN = "plans/CURRENT_STATE.md"
FOLDER = "docs/mockups"


def eid(n):
    return f"{n:064x}"


def pin_set(target, target_kind, pinned, rank):
    return {"schema": SCHEMA, "op": "pin.set", "target": target,
            "targetKind": target_kind, "pinned": pinned, "rank": rank}


def pin_rank(target, rank):
    return {"schema": SCHEMA, "op": "pin.rank", "target": target, "rank": rank}


def ev(n, pk, t, c, kind=44251, project=P, repo=R, tags=None, content_override=None):
    if tags is None:
        tags = [["a", project], ["ar-v", "ar1-1"], ["ar-op", c["op"]],
                ["ar-repo", repo], ["ar-target", c["target"]]]
    content = content_override if content_override is not None else json.dumps(
        c, separators=(",", ":"))
    return {"id": eid(n), "pubkey": pk, "created_at": t, "kind": kind,
            "tags": tags, "content": content}


def row(target, target_kind, pinned, rank, by, updated_at):
    return {"target": target, "targetKind": target_kind, "pinned": pinned,
            "rank": rank, "by": by, "updatedAt": updated_at}


def digest(pins, ignored=0, other_repo=0, ranks_without_pin=0):
    return {"schema": "buzz-project-artifact-pin-digest/v1", "project": P, "repo": R,
            "ignored": ignored, "otherRepo": other_repo,
            "ranksWithoutPin": ranks_without_pin, "pins": list(pins)}


cases = []

cases.append({"name": "empty", "project": P, "repo": R, "events": [],
              "expected": digest([])})

# One pin is one row, and `by` is who pinned it.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"))
cases.append({
    "name": "one pin.set is one row, pinned, at the rank it entered",
    "project": P, "repo": R, "events": [e1],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 100)]),
})

# A folder is pinnable although git has no such object: the target is the
# prefix its files share, and that is what the sidebar row is.
e1 = ev(1, ALICE, 100, pin_set(FOLDER, "folder", True, "a0"))
cases.append({
    "name": "a folder of the documents tree is a pinnable target",
    "project": P, "repo": R, "events": [e1],
    "expected": digest([row(FOLDER, "folder", True, "a0", ALICE, 100)]),
})

# Un-pinning keeps the row with pinned false: the control has to know the
# target was pinned once and is not now.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"))
e2 = ev(2, BOB, 110, pin_set(DOC, "file", False, "a0"))
cases.append({
    "name": "un-pinning keeps the row with pinned false and the later author",
    "project": P, "repo": R, "events": [e1, e2],
    "expected": digest([row(DOC, "file", False, "a0", BOB, 110)]),
})

# A reorder moves the rank and leaves `by` alone: who pinned it is not who
# last nudged its order.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a1"))
e2 = ev(2, BOB, 110, pin_rank(DOC, "a0"))
cases.append({
    "name": "a pin.rank moves the rank, updates updatedAt, and leaves by alone",
    "project": P, "repo": R, "events": [e1, e2],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 110)]),
})

# Rule 4: a pin.set's own rank takes part under the set's key, so a rank op
# stamped earlier by a skewed clock loses to it.
e1 = ev(1, BOB, 90, pin_rank(DOC, "a5"))
e2 = ev(2, ALICE, 100, pin_set(DOC, "file", True, "a1"))
cases.append({
    "name": "a pin.rank stamped before the set that introduced the target loses to it",
    "project": P, "repo": R, "events": [e1, e2],
    "expected": digest([row(DOC, "file", True, "a1", ALICE, 100)]),
})

# Rule 3: a rank without a set is counted, never a row — and the order of the
# bag is not the order of the clock, so the set may arrive second.
e1 = ev(1, BOB, 110, pin_rank("docs/never-pinned.md", "a0"))
e2 = ev(2, ALICE, 100, pin_set(DOC, "file", True, "a0"))
cases.append({
    "name": "a pin.rank naming a target no pin.set introduced is counted, not a row",
    "project": P, "repo": R, "events": [e1, e2],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 100)],
                       ranks_without_pin=1),
})

# Rule 5: rows sort by rank then target, both bytewise, and a plan pins
# exactly as a document does.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a2"))
e2 = ev(2, ALICE, 101, pin_set(PLAN, "file", True, "a1"))
e3 = ev(3, ALICE, 102, pin_set(FOLDER, "folder", True, "a1"))
cases.append({
    "name": "rows sort by rank then target, and a plan pins like a document",
    "project": P, "repo": R, "events": [e1, e2, e3],
    "expected": digest([
        row(FOLDER, "folder", True, "a1", ALICE, 102),
        row(PLAN, "file", True, "a1", ALICE, 101),
        row(DOC, "file", True, "a2", ALICE, 100),
    ]),
})

# A created_at tie breaks on the event id, the same clock as every other fold.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"))
e2 = ev(2, BOB, 100, pin_set(DOC, "file", False, "a0"))
cases.append({
    "name": "a created_at tie breaks on event id",
    "project": P, "repo": R, "events": [e2, e1],
    "expected": digest([row(DOC, "file", False, "a0", BOB, 100)]),
})

# Rule 1, every arm at once.
e_ok = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"))
e_other_repo = ev(2, ALICE, 101, pin_set(PLAN, "file", True, "a1"), repo=OTHER_R)
e_other_project = ev(3, ALICE, 102, pin_set(PLAN, "file", True, "a1"), project=OTHER_P)
e_wrong_kind = ev(4, ALICE, 103, pin_set(PLAN, "file", True, "a1"), kind=44250)
e_malformed = ev(5, ALICE, 104, pin_set(PLAN, "file", True, "a1"),
                 content_override="{not json")
e_bad_rank = ev(6, ALICE, 105, pin_set(PLAN, "file", True, ""))
e_keep = ev(7, ALICE, 106, pin_set("docs/mockups/.gitkeep", "file", True, "a1"))
e_folder_as_file = ev(8, ALICE, 107, pin_set(FOLDER, "file", True, "a1"))
e_tree_root = ev(9, ALICE, 108, pin_set("docs", "folder", True, "a1"))
e_two_a = ev(10, ALICE, 109, pin_set(PLAN, "file", True, "a1"),
             tags=[["a", P], ["a", OTHER_P], ["ar-v", "ar1-1"],
                   ["ar-op", "pin.set"], ["ar-repo", R], ["ar-target", PLAN]])
cases.append({
    "name": "another repository's op is counted; malformed, mis-kinded, mis-scoped and ungrammatical ops are ignored",
    "project": P, "repo": R,
    "events": [e_ok, e_other_repo, e_other_project, e_wrong_kind, e_malformed,
               e_bad_rank, e_keep, e_folder_as_file, e_tree_root, e_two_a],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 100)],
                       ignored=8, other_repo=1),
})

# A case-variant `a` tag folds as its canonical coordinate. Ingest never
# stores one, and a reader that sees one anyway must not drop it.
UPPER_P = "30621:" + "A" * 64 + ":tank-loop"
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"), project=UPPER_P)
cases.append({
    "name": "a case-variant a tag folds as its canonical coordinate",
    "project": P, "repo": R, "events": [e1],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 100)]),
})

# Duplicate ids keep the first occurrence and count nothing.
e1 = ev(1, ALICE, 100, pin_set(DOC, "file", True, "a0"))
e1_again = ev(1, BOB, 200, pin_set(DOC, "file", False, "a9"))
cases.append({
    "name": "duplicate ids keep the first",
    "project": P, "repo": R, "events": [e1, e1_again],
    "expected": digest([row(DOC, "file", True, "a0", ALICE, 100)]),
})

out = {"schema": "buzz-project-artifact-pin-fold-vectors/v1", "cases": cases}
with open(os.path.join(HERE, "fixtures", "fold-vectors.json"), "w") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
print(f"wrote {len(cases)} cases")
