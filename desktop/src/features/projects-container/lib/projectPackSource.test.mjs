import assert from "node:assert/strict";
import test from "node:test";

import {
  canSetProjectPackSource,
  newestProjectPackSource,
  parseProjectPackSourceEvent,
} from "./projectPackSource.ts";

const OWNER = "a".repeat(64);
const AUTHOR = "b".repeat(64);
const PROJECT_COORD = `30621:${OWNER}:agiterra`;
const REPO_COORD = `30617:${OWNER}:agiterra-packs`;

function event(overrides = {}) {
  return {
    id: "e".repeat(64),
    pubkey: AUTHOR,
    created_at: 1_800_000_000,
    kind: 30624,
    tags: [
      ["d", PROJECT_COORD],
      ["repo", REPO_COORD],
      ["ref", "refs/heads/main"],
    ],
    content: JSON.stringify({ schema: "buzz-project-pack-source/v1" }),
    sig: "s".repeat(128),
    ...overrides,
  };
}

test("parses a well-formed ref-pinned source", () => {
  const parsed = parseProjectPackSourceEvent(event());
  assert.deepEqual(parsed, {
    eventId: "e".repeat(64),
    author: AUTHOR,
    createdAt: 1_800_000_000,
    repo: REPO_COORD,
    ref: "refs/heads/main",
    sha: null,
    path: "personas/roles",
    note: null,
  });
});

test("parses a well-formed sha-pinned source with a path and a note", () => {
  const sha = "c".repeat(40);
  const parsed = parseProjectPackSourceEvent(
    event({
      tags: [
        ["d", PROJECT_COORD],
        ["repo", REPO_COORD],
        ["sha", sha],
        ["path", "roles"],
      ],
      content: JSON.stringify({
        schema: "buzz-project-pack-source/v1",
        note: "pinned for the release",
      }),
    }),
  );
  assert.equal(parsed?.sha, sha);
  assert.equal(parsed?.ref, null);
  assert.equal(parsed?.path, "roles");
  assert.equal(parsed?.note, "pinned for the release");
});

test("refuses the wrong kind", () => {
  assert.equal(parseProjectPackSourceEvent(event({ kind: 1 })), null);
});

test("refuses a missing repo tag", () => {
  assert.equal(
    parseProjectPackSourceEvent(
      event({
        tags: [
          ["d", PROJECT_COORD],
          ["ref", "refs/heads/main"],
        ],
      }),
    ),
    null,
  );
});

test("refuses neither ref nor sha, and refuses both at once", () => {
  assert.equal(
    parseProjectPackSourceEvent(
      event({
        tags: [
          ["d", PROJECT_COORD],
          ["repo", REPO_COORD],
        ],
      }),
    ),
    null,
    "neither ref nor sha",
  );
  assert.equal(
    parseProjectPackSourceEvent(
      event({
        tags: [
          ["d", PROJECT_COORD],
          ["repo", REPO_COORD],
          ["ref", "refs/heads/main"],
          ["sha", "c".repeat(40)],
        ],
      }),
    ),
    null,
    "both ref and sha",
  );
});

test("refuses a malformed sha", () => {
  assert.equal(
    parseProjectPackSourceEvent(
      event({
        tags: [
          ["d", PROJECT_COORD],
          ["repo", REPO_COORD],
          ["sha", "deadbeef"],
        ],
      }),
    ),
    null,
  );
});

test("refuses content that is not this schema", () => {
  assert.equal(
    parseProjectPackSourceEvent(event({ content: JSON.stringify({}) })),
    null,
  );
  assert.equal(
    parseProjectPackSourceEvent(event({ content: "not json" })),
    null,
  );
});

test("newestProjectPackSource picks the highest created_at among valid events, skipping malformed ones", () => {
  const older = event({ id: "1".repeat(64), created_at: 100 });
  const newer = event({ id: "2".repeat(64), created_at: 200 });
  const malformed = event({
    id: "3".repeat(64),
    created_at: 300,
    tags: [["d", PROJECT_COORD]],
  });
  assert.equal(
    newestProjectPackSource([older, newer, malformed])?.eventId,
    "2".repeat(64),
  );
});

test("newestProjectPackSource is null over an empty or all-malformed set", () => {
  assert.equal(newestProjectPackSource([]), null);
  assert.equal(
    newestProjectPackSource([event({ tags: [["d", PROJECT_COORD]] })]),
    null,
  );
});

test("canSetProjectPackSource: the project owner may always set it", () => {
  assert.equal(
    canSetProjectPackSource({
      self: OWNER,
      project: { owner: OWNER },
      roster: [],
      repo: null,
    }),
    true,
  );
});

test("canSetProjectPackSource: a roster owner may set it", () => {
  assert.equal(
    canSetProjectPackSource({
      self: AUTHOR,
      project: { owner: OWNER },
      roster: [{ pubkey: AUTHOR, role: "owner" }],
      repo: null,
    }),
    true,
  );
});

test("canSetProjectPackSource: a repo's own signer or maintainer may set it (finding 33)", () => {
  const signer = "d".repeat(64);
  const maintainer = "e".repeat(64);
  assert.equal(
    canSetProjectPackSource({
      self: signer,
      project: { owner: OWNER },
      roster: [],
      repo: { owner: signer, maintainers: [] },
    }),
    true,
  );
  assert.equal(
    canSetProjectPackSource({
      self: maintainer,
      project: { owner: OWNER },
      roster: [],
      repo: { owner: signer, maintainers: [maintainer] },
    }),
    true,
  );
});

test("canSetProjectPackSource: a stranger may not, and an unread repo denies rather than guesses", () => {
  const stranger = "f".repeat(64);
  assert.equal(
    canSetProjectPackSource({
      self: stranger,
      project: { owner: OWNER },
      roster: [],
      repo: null,
    }),
    false,
  );
  assert.equal(
    canSetProjectPackSource({
      self: null,
      project: { owner: OWNER },
      roster: [],
      repo: null,
    }),
    false,
  );
});
