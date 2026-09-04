import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * LANE-L30 — the "Create packs repository" action's repository-id and name
 * fields (`ProjectPacksSettingsSection.tsx`'s `ProjectPacksCreateRepoAction`):
 * a default id derived from the project slug, editable and validated as a
 * 30617 `d` tag before submit is allowed, and a name that defaults to the id
 * until the viewer types into it directly. `TeamReadinessCard.test.mjs` is
 * this file's template for driving a real (not `renderToStaticMarkup`) tree
 * under jsdom with a mocked `__TAURI_INTERNALS__.invoke`.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

let calls = [];
let answers = {};

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle,
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      const answer = answers[command];
      if (typeof answer === "function") return answer(args);
      if (answer !== undefined) return answer;
      throw new Error(`unmocked Tauri command: ${command}`);
    },
  };
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  calls = [];
  answers = {};
});

after(() => dom.window.close());

const OWNER = "a".repeat(64);
const PROJECT = {
  id: "proj-1",
  dtag: "agiterra",
  owner: OWNER,
  name: "agiterra",
  description: "",
  createdAt: 1_800_000_000,
  address: `30621:${OWNER}:agiterra`,
  repoAddrs: [],
  agentAddrs: [],
  channelIds: [],
  visibility: "public",
  members: [],
  icon: null,
  color: null,
};

async function renderSection() {
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { render, screen, fireEvent } = await import("@testing-library/react");
  const { ProjectPacksSettingsSection, projectPackSourceQueryKey } =
    await import("./ProjectPacksSettingsSection.tsx");
  const { projectRosterQueryKey } = await import("../lib/projectMembers.ts");

  const client = new QueryClient({
    defaultOptions: { queries: { gcTime: Infinity, retry: false } },
  });
  client.setQueryData(["identity"], { pubkey: OWNER });
  client.setQueryData(projectRosterQueryKey(PROJECT.address), []);
  client.setQueryData(projectPackSourceQueryKey(PROJECT.address), null);

  render(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectPacksSettingsSection, { project: PROJECT }),
    ),
  );

  fireEvent.click(screen.getByTestId("project-packs-create-repo-open"));
  await screen.findByTestId("project-packs-create-repo-panel");
  return { fireEvent, screen };
}

test("the repository id field defaults to <project-slug>-packs", async () => {
  const { screen } = await renderSection();
  assert.equal(
    screen.getByTestId("project-packs-create-repo-id").value,
    "agiterra-packs",
  );
});

test("the name field defaults to the id, and follows it until edited directly", async () => {
  const { screen, fireEvent } = await renderSection();
  const idField = screen.getByTestId("project-packs-create-repo-id");
  const nameField = screen.getByTestId("project-packs-create-repo-name");

  assert.equal(nameField.value, "agiterra-packs");

  fireEvent.change(idField, { target: { value: "shared-org-packs" } });
  assert.equal(
    idField.value,
    "shared-org-packs",
    "the id field takes what was typed",
  );
  assert.equal(
    nameField.value,
    "shared-org-packs",
    "an untouched name keeps following the id",
  );

  fireEvent.change(nameField, { target: { value: "Agiterra Shared Packs" } });
  fireEvent.change(idField, { target: { value: "another-id" } });
  assert.equal(
    nameField.value,
    "Agiterra Shared Packs",
    "a name the viewer typed is never clobbered by a later id edit",
  );
});

test("a custom id is what reaches the host, with the name it resolved to", async () => {
  answers.project_packs_init = {
    repoRef: `30617:${OWNER}:shared-org-packs`,
    sourceEventId: "b".repeat(64),
    seedCommitSha: "c".repeat(40),
    seedError: null,
    commitIdentityName: "agiterra",
    commitIdentityEmail: `${OWNER.slice(0, 8)}@beekeeper.local`,
    pushRecordEventId: "d".repeat(64),
    announcementWithdrawnEventId: null,
    announcementWithdrawalError: null,
  };
  const { screen, fireEvent } = await renderSection();
  fireEvent.change(screen.getByTestId("project-packs-create-repo-id"), {
    target: { value: "shared-org-packs" },
  });
  fireEvent.click(screen.getByTestId("project-packs-create-repo-submit"));

  await screen.findByTestId("project-packs-create-repo-result");
  const call = calls.find((entry) => entry.command === "project_packs_init");
  assert.ok(call, "project_packs_init was called");
  assert.equal(call.args.projectRef, PROJECT.address);
  assert.equal(call.args.repoId, "shared-org-packs");
  assert.equal(call.args.name, "shared-org-packs");

  // The result line prints the repository coordinate the host made.
  assert.match(
    screen.getByTestId("project-packs-create-repo-coordinate").textContent,
    /30617:a{64}:shared-org-packs/,
  );
});

test("an invalid id is refused with the sentence, and never reaches the host", async () => {
  const { screen, fireEvent } = await renderSection();
  const idField = screen.getByTestId("project-packs-create-repo-id");
  fireEvent.change(idField, { target: { value: "Not A Repo Id!" } });

  const errorText = screen.getByTestId(
    "project-packs-create-repo-id-error",
  ).textContent;
  assert.match(
    errorText,
    /may only contain lowercase letters, digits, '\.', '_', and '-'/,
  );

  const submit = screen.getByTestId("project-packs-create-repo-submit");
  assert.equal(submit.disabled, true);
  fireEvent.click(submit);
  assert.equal(
    calls.find((entry) => entry.command === "project_packs_init"),
    undefined,
    "a refused id never reaches project_packs_init",
  );
});

test("an id starting with '.' or containing '..' is refused, distinctly from the character-class sentence", async () => {
  const { screen, fireEvent } = await renderSection();
  const idField = screen.getByTestId("project-packs-create-repo-id");

  fireEvent.change(idField, { target: { value: ".hidden-packs" } });
  assert.match(
    screen.getByTestId("project-packs-create-repo-id-error").textContent,
    /must not start with '\.'/,
  );

  fireEvent.change(idField, { target: { value: "agiterra..packs" } });
  assert.match(
    screen.getByTestId("project-packs-create-repo-id-error").textContent,
    /must not contain '\.\.'/,
  );
});

// --- LANE-L31 (Finding 66): the seed outcome as a sentence, not raw stderr ---

test("a successful seed prints 'seeded as <identity>, commit <sha8>' — never a raw error", async () => {
  answers.project_packs_init = {
    repoRef: `30617:${OWNER}:agiterra-packs`,
    sourceEventId: "b".repeat(64),
    seedCommitSha: "c".repeat(40),
    seedError: null,
    commitIdentityName: "Waggle Bot",
    commitIdentityEmail: "aaaaaaaa@beekeeper.local",
    pushRecordEventId: "d".repeat(64),
    announcementWithdrawnEventId: null,
    announcementWithdrawalError: null,
  };
  const { screen, fireEvent } = await renderSection();
  fireEvent.click(screen.getByTestId("project-packs-create-repo-submit"));

  const sentence = await screen.findByTestId(
    "project-packs-create-repo-seed-outcome",
  );
  assert.equal(
    sentence.textContent,
    "Seeded as Waggle Bot <aaaaaaaa@beekeeper.local>, commit cccccccc.",
  );
  assert.equal(
    screen.queryByTestId("project-packs-create-repo-seed-error-details"),
    null,
    "no details disclosure when there was nothing to disclose",
  );
});

test("a failed seed prints what failed, the identity, and what was withdrawn — raw text stays behind a details disclosure", async () => {
  const rawError =
    "Author identity unknown\n\n*** Please tell me who you are. …\nfatal: unable to auto-detect email address (got 'brian@MacBookPro.(none)')";
  answers.project_packs_init = {
    repoRef: `30617:${OWNER}:agiterra-packs`,
    sourceEventId: null,
    seedCommitSha: null,
    seedError: rawError,
    commitIdentityName: "Waggle Bot",
    commitIdentityEmail: "aaaaaaaa@beekeeper.local",
    pushRecordEventId: null,
    announcementWithdrawnEventId: "e".repeat(64),
    announcementWithdrawalError: null,
  };
  const { screen, fireEvent } = await renderSection();
  fireEvent.click(screen.getByTestId("project-packs-create-repo-submit"));

  const sentence = await screen.findByTestId(
    "project-packs-create-repo-seed-outcome",
  );
  assert.match(sentence.textContent, /seeding failed as Waggle Bot/);
  assert.match(sentence.textContent, /aaaaaaaa@beekeeper\.local/);
  assert.match(sentence.textContent, /withdrawn \(eeeeeeee\)/);
  assert.doesNotMatch(
    sentence.textContent,
    /Author identity unknown/,
    "the product sentence must not carry git's own words",
  );

  // The raw text this sentence replaces is still reachable, just not thrust
  // at the viewer by default.
  const details = screen.getByTestId(
    "project-packs-create-repo-seed-error-details",
  );
  assert.equal(details.tagName, "DETAILS");
  assert.match(details.textContent, /Author identity unknown/);
  assert.match(details.textContent, /unable to auto-detect email address/);
});

test("a failed withdrawal is reported too, with the coordinate to delete by hand", async () => {
  answers.project_packs_init = {
    repoRef: `30617:${OWNER}:agiterra-packs`,
    sourceEventId: null,
    seedCommitSha: null,
    seedError: "push failed: connection refused",
    commitIdentityName: "Waggle Bot",
    commitIdentityEmail: "aaaaaaaa@beekeeper.local",
    pushRecordEventId: null,
    announcementWithdrawnEventId: null,
    announcementWithdrawalError: "relay rejected the tombstone",
  };
  const { screen, fireEvent } = await renderSection();
  fireEvent.click(screen.getByTestId("project-packs-create-repo-submit"));

  const sentence = await screen.findByTestId(
    "project-packs-create-repo-seed-outcome",
  );
  assert.match(sentence.textContent, /could not be withdrawn/);
  assert.match(
    sentence.textContent,
    new RegExp(`delete 30617:${OWNER}:agiterra-packs by hand`),
  );
});
