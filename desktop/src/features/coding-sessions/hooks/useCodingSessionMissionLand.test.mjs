import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionRepoAddress,
  readCodingSessionRepository,
  readProjectOwners,
} from "./useCodingSessionMissionLand.ts";

const OWNER = "6c".repeat(32);
const CREATOR = "6c".repeat(32);
const CO_OWNER = "3d".repeat(32);
const COLLABORATOR = "77".repeat(32);
const COORDINATE = `30621:${CREATOR}:bee-keeper`;

/** A fetcher that answers each kind from a fixed table. */
function fetcher(byKind) {
  return async (filter) => byKind[filter.kinds[0]] ?? [];
}

function announcement(tags) {
  return {
    id: "a".repeat(64),
    pubkey: OWNER,
    created_at: 10,
    kind: 30617,
    tags,
    content: "",
    sig: "",
  };
}

test("L18: a project's Owners are its creator plus every roster row marked owner", async () => {
  const owners = await readProjectOwners(
    COORDINATE,
    fetcher({
      39010: [
        {
          created_at: 20,
          tags: [
            ["d", COORDINATE],
            ["p", CO_OWNER, "", "owner"],
            ["p", COLLABORATOR, "", "collaborator"],
          ],
        },
      ],
    }),
  );
  assert.deepEqual(owners, [CREATOR, CO_OWNER]);
});

test("L18: with no 39010 projection the project head's own p tags are read", async () => {
  const owners = await readProjectOwners(
    COORDINATE,
    fetcher({
      39010: [],
      30621: [
        {
          created_at: 5,
          tags: [
            ["d", "bee-keeper"],
            ["p", CO_OWNER, "", "owner"],
          ],
        },
      ],
    }),
  );
  assert.deepEqual(owners, [CREATOR, CO_OWNER]);
});

test("L18: an announcement with no project back-reference has a read, empty roster", async () => {
  const repository = await readCodingSessionRepository(
    `30617:${OWNER}:agiterra-beekeeper`,
    fetcher({ 30617: [announcement([["d", "agiterra-beekeeper"]])] }),
  );
  // Read, not unread: there is no roster, so nothing is missing from the set.
  assert.deepEqual(repository.projectOwnerPubkeys, []);
});

test("L18: a roster read that throws leaves the set marked unread, never empty", async () => {
  const repository = await readCodingSessionRepository(
    `30617:${OWNER}:agiterra-beekeeper`,
    async (filter) => {
      if (filter.kinds[0] === 30617) {
        return [
          announcement([
            ["d", "agiterra-beekeeper"],
            ["project", COORDINATE],
          ]),
        ];
      }
      throw new Error("relay is down");
    },
  );
  assert.equal(
    repository.projectOwnerPubkeys,
    null,
    "an unread roster is null — guessing [] is finding 33's own shape",
  );
});

test("L18: the roster is read through the announcement's project tag", async () => {
  const repository = await readCodingSessionRepository(
    `30617:${OWNER}:agiterra-beekeeper`,
    fetcher({
      30617: [
        announcement([
          ["d", "agiterra-beekeeper"],
          ["maintainers", CO_OWNER],
          ["project", COORDINATE],
        ]),
      ],
      39010: [{ created_at: 20, tags: [["p", CO_OWNER, "", "owner"]] }],
    }),
  );
  assert.deepEqual(repository.projectOwnerPubkeys, [CREATOR, CO_OWNER]);
  assert.equal(repository.ownerPubkey, OWNER);
  assert.ok(
    repository.protectionTags.some((tag) => tag[0] === "maintainers"),
    "the whole tag list reaches the rule, maintainers included",
  );
});

test("L18: the repo address parser is unchanged by the roster read", () => {
  assert.deepEqual(
    codingSessionRepoAddress(`30617:${OWNER}:agiterra-beekeeper`),
    { ownerPubkey: OWNER, identifier: "agiterra-beekeeper" },
  );
  assert.equal(codingSessionRepoAddress("nonsense"), null);
});
