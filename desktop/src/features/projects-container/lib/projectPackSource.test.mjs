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
    expectation: { kind: "unconditional" },
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

test("v2 distinguishes initial creation and replacement from v1 unconditional", () => {
  for (const sourceId of [null, "a".repeat(64)]) {
    const parsed = parseProjectPackSourceEvent(
      event({
        content: JSON.stringify({
          schema: "buzz-project-pack-source/v2",
          expectedSourceId: sourceId,
          note: "checked candidate",
        }),
      }),
    );
    assert.deepEqual(parsed?.expectation, { kind: "expected", sourceId });
    assert.equal(parsed?.note, "checked candidate");
  }
  assert.deepEqual(parseProjectPackSourceEvent(event())?.expectation, {
    kind: "unconditional",
  });
});

test("v2 refuses absent, malformed, extra and wrong-version condition fields", () => {
  const schema = "buzz-project-pack-source/v2";
  const invalidContents = [
    { schema },
    ...[
      "",
      "a".repeat(63),
      "a".repeat(65),
      "A".repeat(64),
      "g".repeat(64),
      ` ${"a".repeat(64)}`,
      1,
      false,
      [],
      {},
    ].map((expectedSourceId) => ({
      schema,
      expectedSourceId,
    })),
    { schema, expectedSourceId: null, extra: true },
    { schema, expectedSourceId: null, note: null },
    { schema: "buzz-project-pack-source/v1", expectedSourceId: null },
    { schema: "buzz-project-pack-source/v3", expectedSourceId: null },
  ];
  for (const content of invalidContents) {
    assert.equal(
      parseProjectPackSourceEvent(event({ content: JSON.stringify(content) })),
      null,
      JSON.stringify(content),
    );
  }
});

test("same-second winners use the lowest event ID independent of author and input order", () => {
  const lower = event({ id: "1".repeat(64), pubkey: OWNER });
  const higher = event({
    id: "2".repeat(64),
    content: JSON.stringify({
      schema: "buzz-project-pack-source/v2",
      expectedSourceId: null,
    }),
  });
  for (const events of [
    [lower, higher],
    [higher, lower],
  ]) {
    assert.equal(newestProjectPackSource(events)?.eventId, lower.id);
  }
  assert.equal(
    newestProjectPackSource([
      lower,
      { ...higher, created_at: lower.created_at + 1 },
    ])?.eventId,
    higher.id,
  );
});

test("v2 requires canonical project d and singleton known tags", () => {
  const original = event();
  const content = JSON.stringify({
    schema: "buzz-project-pack-source/v2",
    expectedSourceId: null,
  });
  const aliasTags = original.tags.map((tag) =>
    tag[0] === "d" ? ["d", `30621:${OWNER.toUpperCase()}:agiterra`] : tag,
  );
  assert.notEqual(
    parseProjectPackSourceEvent(event({ tags: aliasTags })),
    null,
  );
  for (const tags of [
    aliasTags,
    original.tags.filter((tag) => tag[0] !== "d"),
    [...original.tags, ["d", PROJECT_COORD]],
    [...original.tags, ["unknown", "field"]],
    [...original.tags, ["path", "roles", "extra"]],
  ]) {
    assert.equal(parseProjectPackSourceEvent(event({ tags, content })), null);
  }
});

test("v2 rejects unsafe paths and malformed refs while preserving valid defaults", () => {
  const content = JSON.stringify({
    schema: "buzz-project-pack-source/v2",
    expectedSourceId: null,
  });
  for (const path of [
    "../escape",
    "/etc",
    "roles//lead",
    "roles/./lead",
    "a".repeat(201),
  ]) {
    assert.equal(
      parseProjectPackSourceEvent(
        event({ content, tags: [...event().tags, ["path", path]] }),
      ),
      null,
    );
  }
  for (const ref of [
    "main",
    "refs/heads/..",
    "refs/heads/bad name",
    "refs/heads/a.lock",
  ]) {
    const tags = event().tags.map((tag) =>
      tag[0] === "ref" ? ["ref", ref] : tag,
    );
    assert.equal(parseProjectPackSourceEvent(event({ content, tags })), null);
  }
});

test("v2 accepts the repository root, as Rust does, and an agents repository there wins (ledger 307)", () => {
  const v2 = (expectedSourceId) =>
    JSON.stringify({ schema: "buzz-project-pack-source/v2", expectedSourceId });
  for (const path of [".", "./", " . "]) {
    const parsed = parseProjectPackSourceEvent(
      event({ content: v2(null), tags: [...event().tags, ["path", path]] }),
    );
    assert.equal(parsed?.path, ".", JSON.stringify(path));
  }
  for (const path of ["./roles", "roles/.", ".."]) {
    assert.equal(
      parseProjectPackSourceEvent(
        event({ content: v2(null), tags: [...event().tags, ["path", path]] }),
      ),
      null,
      path,
    );
  }

  // The Tank Loop pair: an older sha-pinned pack-layout record, replaced by
  // the project's agents repository at the root. The newer one must win.
  const older = event({
    id: "8".repeat(64),
    created_at: 1_790_000_000,
    tags: [
      ["d", PROJECT_COORD],
      ["repo", `30617:${AUTHOR}:agiterra-packs-43aa15fa1848`],
      ["sha", "f".repeat(40)],
    ],
    content: v2(null),
  });
  const newer = event({
    id: "4".repeat(64),
    pubkey: OWNER,
    created_at: 1_790_500_000,
    tags: [
      ["d", PROJECT_COORD],
      ["repo", `30617:${OWNER}:agiterra-beekeeper-agents`],
      ["ref", "refs/heads/main"],
      ["path", "."],
    ],
    content: v2(older.id),
  });
  for (const events of [
    [older, newer],
    [newer, older],
  ]) {
    assert.equal(newestProjectPackSource(events)?.eventId, newer.id);
  }
});

test("v2 repository coordinates match Rust normalization and Unicode length bounds", () => {
  const content = JSON.stringify({
    schema: "buzz-project-pack-source/v2",
    expectedSourceId: null,
  });
  const withRepo = (repo, body = content) =>
    event({
      content: body,
      tags: event().tags.map((tag) =>
        tag[0] === "repo" ? ["repo", repo] : tag,
      ),
    });
  for (const slug of [
    "roles",
    "a".repeat(64),
    "🧑".repeat(64),
    "roles:shared",
    " roles ",
  ]) {
    const repo = `30617:${OWNER.toUpperCase()}:${slug}`;
    assert.equal(
      parseProjectPackSourceEvent(withRepo(repo))?.repo,
      `30617:${OWNER}:${slug}`,
    );
  }
  for (const repo of [
    `30617:${OWNER}:`,
    `30617:${OWNER}:${"a".repeat(65)}`,
    `30617:${OWNER}:${"🧑".repeat(65)}`,
    `30617:${OWNER}:bad\nslug`,
    `30617:${OWNER}:bad\u0085slug`,
    ` 30617:${OWNER}:roles`,
    `30621:${OWNER}:roles`,
  ])
    assert.equal(parseProjectPackSourceEvent(withRepo(repo)), null, repo);
  // The older reader's repository acceptance remains unchanged for v1.
  assert.notEqual(
    parseProjectPackSourceEvent(
      withRepo(`30617:${OWNER}:${"a".repeat(65)}`, event().content),
    ),
    null,
  );
  assert.equal(
    parseProjectPackSourceEvent(
      withRepo(`30617:${OWNER.toUpperCase()}:roles`, event().content),
    ),
    null,
  );
});

test("v2 normalizes SHA pins as Rust does while v1 keeps its original reader behavior", () => {
  const content = JSON.stringify({
    schema: "buzz-project-pack-source/v2",
    expectedSourceId: null,
  });
  const withSha = (sha, body = content) =>
    event({
      content: body,
      tags: event().tags.map((tag) => (tag[0] === "ref" ? ["sha", sha] : tag)),
    });
  for (const sha of [
    "a".repeat(40),
    "A".repeat(40),
    ` \t${"A".repeat(40)}\n `,
  ]) {
    assert.equal(
      parseProjectPackSourceEvent(withSha(sha))?.sha,
      "a".repeat(40),
    );
  }
  for (const sha of ["", "a".repeat(39), "a".repeat(41), "g".repeat(40)]) {
    assert.equal(parseProjectPackSourceEvent(withSha(sha)), null);
  }
  for (const sha of ["A".repeat(40), ` ${"a".repeat(40)} `]) {
    assert.equal(
      parseProjectPackSourceEvent(withSha(sha, event().content)),
      null,
    );
  }
});

test("v2 refuses duplicate content fields including escaped names before accepting a condition", () => {
  const prefix = '"schema":"buzz-project-pack-source/v2"';
  for (const content of [
    `{${prefix},"expectedSourceId":null,"expectedSourceId":"${OWNER}"}`,
    `{${prefix},"expectedSourceId":"${OWNER}","expectedSourceId":null}`,
    String.raw`{${prefix},"expectedSourceId":null,"expected\u0053ourceId":null}`,
    `{${prefix},"expectedSourceId":null,"note":"first","note":"second"}`,
    `{${prefix},${prefix},"expectedSourceId":null}`,
  ]) {
    assert.equal(
      parseProjectPackSourceEvent(event({ content })),
      null,
      content,
    );
  }
  const parsed = parseProjectPackSourceEvent(
    event({
      content: JSON.stringify({
        schema: "buzz-project-pack-source/v2",
        expectedSourceId: null,
        note: 'Example: {"expectedSourceId":null,"expectedSourceId":null}',
      }),
    }),
  );
  assert.deepEqual(parsed?.expectation, { kind: "expected", sourceId: null });
});

// Both typed Rust wire bodies reject duplicate fields, including a schema
// overwritten to v1. Do not interpret ambiguous bytes as unconditional.
test("duplicate schema cannot downgrade a conditional body to v1", () => {
  assert.equal(
    parseProjectPackSourceEvent(
      event({
        content:
          '{"schema":"buzz-project-pack-source/v2","schema":"buzz-project-pack-source/v1"}',
      }),
    ),
    null,
  );
});

test("a source one-shot read settles without a WebSocket admission slot", async () => {
  const { relayClient } = await import("../../../shared/api/relayClient.ts");
  const { fetchProjectPackSource } = await import("./projectPackSource.ts");
  const original = relayClient.fetchEventsCoalesced;
  const filters = [];
  relayClient.fetchEventsCoalesced = async (filter) => {
    filters.push(filter);
    return [event()];
  };
  try {
    const source = await fetchProjectPackSource(PROJECT_COORD);
    assert.equal(source.eventId, event().id);
    assert.deepEqual(filters, [
      { kinds: [30624], "#d": [PROJECT_COORD], limit: 4 },
    ]);
  } finally {
    relayClient.fetchEventsCoalesced = original;
  }
});
