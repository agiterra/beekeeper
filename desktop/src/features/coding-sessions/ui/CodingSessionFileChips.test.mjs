import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, afterEach, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * SV-32 file chips through the real pieces: the inline `code` renderer, the
 * link anchor, `FileRefProvider`, the IPC wrapper and the mocked host. The
 * host is the Tauri IPC boundary itself, so what is asserted is exactly what
 * would cross it.
 */

const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_ID = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const LOCAL_PROVIDER = "ab".repeat(32);
const OTHER_PROVIDER = "cd".repeat(32);
const CANDIDATE = "desktop/src/app/App.tsx:42";
const FULL_PATH = "/Users/someone/worktrees/session/desktop/src/app/App.tsx";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

const calls = [];
let fileRefsAnswer = null;

const tauriInternals = {
  invoke(command, args) {
    calls.push({ command, args });
    if (command === "coding_session_provider_status") {
      return Promise.resolve({
        provisioned: true,
        running: true,
        host: { reachable: true },
        providerPubkey: LOCAL_PROVIDER,
      });
    }
    if (command === "coding_session_file_refs") {
      return Promise.resolve(fileRefsAnswer(args.request));
    }
    if (
      command === "coding_session_open_file_ref" ||
      command === "coding_session_reveal_file_ref"
    ) {
      return Promise.resolve(null);
    }
    return Promise.reject(new Error(`unmocked: ${command}`));
  },
  transformCallback: () => Math.random(),
};

function localAnswer(request) {
  return {
    where: "thisComputer",
    source: "session",
    reason: null,
    checkedAt: "2026-10-07T00:00:00Z",
    refs: Object.fromEntries(
      request.candidates.map((candidate) => [
        candidate,
        {
          exists: candidate.startsWith("desktop/"),
          isDir: false,
          relativePath: candidate.startsWith("desktop/")
            ? "desktop/src/app/App.tsx"
            : null,
          fullPath: candidate.startsWith("desktop/") ? FULL_PATH : null,
          line: 42,
          column: null,
        },
      ]),
    ),
  };
}

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

beforeEach(async () => {
  calls.length = 0;
  fileRefsAnswer = localAnswer;
  const { resetFileRefResolution } = await import("../useFileRefResolution.ts");
  resetFileRefResolution();
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

function item(pubkey) {
  return {
    id: "item-1",
    type: "message",
    role: "assistant",
    title: "Assistant",
    text: `Edited \`${CANDIDATE}\` and \`origin/main\`.`,
    timestamp: "2026-10-07T00:00:00.000Z",
    channelId: CHANNEL_ID,
    providerSessionId: SESSION_ID,
    bridgeSource: { pubkey, label: "provider" },
  };
}

async function settle() {
  const { act } = await import("@testing-library/react");
  for (let i = 0; i < 5; i += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

/** Mount inline code spans the way an answer's markdown renders them. */
async function mount({
  pubkey = LOCAL_PROVIDER,
  wrap = true,
  codes,
  link,
  text,
} = {}) {
  const React = (await import("react")).default;
  const { render } = await import("@testing-library/react");
  const { createCodeComponent } = await import(
    "../../../shared/ui/markdown/MarkdownCode.tsx"
  );
  const { FileLinkAnchor } = await import(
    "../../../shared/ui/markdown/FileLinkAnchor.tsx"
  );
  const { TooltipProvider } = await import("../../../shared/ui/tooltip.tsx");
  const { FileRefProvider } = await import("../useFileRefResolution.ts");
  const MarkdownCode = createCodeComponent(true);
  const source = item(pubkey);
  const body = React.createElement(
    "p",
    null,
    ...(codes ?? [CANDIDATE, "origin/main"]).map((code) =>
      React.createElement(MarkdownCode, { key: code }, code),
    ),
    link
      ? React.createElement(
          FileLinkAnchor,
          {
            anchorProps: {},
            href: link,
            isLinearLink: false,
            label: "App.tsx",
          },
          "App.tsx",
        )
      : null,
  );
  const tree = wrap
    ? React.createElement(FileRefProvider, {
        item: source,
        text: `${text ?? source.text}${link ? ` [App.tsx](${link})` : ""}`,
        execution: { projectRef: null, isHiredSeat: false },
        children: body,
      })
    : body;
  const view = render(React.createElement(TooltipProvider, null, tree));
  await settle();
  return view;
}

test("a local answer's path is a chip labelled with its basename and line", async () => {
  const view = await mount();
  const chip = view.container.querySelector("[data-file-chip]");
  assert.ok(chip, "chip rendered");
  assert.equal(chip.textContent, "App.tsx · L42");
  assert.equal(chip.getAttribute("data-file-type"), "react");
  // `origin/main` is not a candidate and stays exactly as it was.
  const codes = [...view.container.querySelectorAll("code")];
  assert.equal(codes.length, 1);
  assert.equal(codes[0].textContent, "origin/main");
  assert.equal(codes[0].hasAttribute("data-file-ref-plain"), false);
  // One lookup for the execution, naming the candidate as written.
  const lookups = calls.filter((c) => c.command === "coding_session_file_refs");
  assert.equal(lookups.length, 1);
  assert.deepEqual(lookups[0].args.request.candidates, [CANDIDATE]);
  assert.equal(lookups[0].args.request.isLocalProvider, true);
  assert.equal(lookups[0].args.request.providerSessionId, SESSION_ID);
});

test("a click opens the candidate as written, never a computed absolute path", async () => {
  const { act, fireEvent } = await import("@testing-library/react");
  const view = await mount();
  const chip = view.container.querySelector("[data-file-chip]");
  await act(async () => {
    fireEvent.click(chip);
  });
  const opens = calls.filter(
    (c) => c.command === "coding_session_open_file_ref",
  );
  assert.equal(opens.length, 1);
  assert.equal(opens[0].args.request.candidate, CANDIDATE);
  assert.equal(opens[0].args.request.channelId, CHANNEL_ID);
  const sent = JSON.stringify(opens[0].args);
  assert.equal(sent.includes(FULL_PATH), false, sent);
  assert.equal(sent.includes("/Users/"), false, sent);
});

test("the menu reveals through the host and copies only from the answer", async () => {
  const { act, fireEvent } = await import("@testing-library/react");
  const view = await mount();
  const chip = view.container.querySelector("[data-file-chip]");
  await act(async () => {
    fireEvent.contextMenu(chip, { clientX: 10, clientY: 10 });
  });
  const menu = document.querySelector("[data-file-chip-menu]");
  assert.ok(menu, "menu opened");
  const labels = [...menu.querySelectorAll("button")].map((b) => b.textContent);
  assert.equal(labels[0], "Open");
  assert.match(
    labels[1],
    /^(Reveal in Finder|Show in Explorer|Show in file manager)$/,
  );
  assert.deepEqual(labels.slice(2), ["Copy relative path", "Copy full path"]);
  await act(async () => {
    fireEvent.click(menu.querySelectorAll("button")[1]);
  });
  const reveals = calls.filter(
    (c) => c.command === "coding_session_reveal_file_ref",
  );
  assert.equal(reveals.length, 1);
  assert.equal(reveals[0].args.request.candidate, CANDIDATE);
});

test("a channel message with the same text renders no chip", async () => {
  const view = await mount({ wrap: false });
  assert.equal(view.container.querySelector("[data-file-chip]"), null);
  assert.equal(view.container.querySelector("[data-file-ref-plain]"), null);
  const codes = [...view.container.querySelectorAll("code")].map(
    (c) => c.textContent,
  );
  assert.deepEqual(codes, [CANDIDATE, "origin/main"]);
  assert.equal(
    calls.some((c) => c.command === "coding_session_file_refs"),
    false,
    "no lookup outside a coding-session answer",
  );
});

test("another computer's answer stays plain code with the host's reason", async () => {
  fileRefsAnswer = () => ({
    where: "notLocal",
    source: null,
    reason: "Written on another computer — open it there",
    checkedAt: "2026-10-07T00:00:00Z",
    refs: {},
  });
  const view = await mount({ pubkey: OTHER_PROVIDER });
  assert.equal(view.container.querySelector("[data-file-chip]"), null);
  const plain = view.container.querySelector("[data-file-ref-plain]");
  assert.ok(plain);
  assert.equal(plain.textContent, CANDIDATE);
  assert.equal(
    plain.getAttribute("data-file-ref-reason"),
    "Written on another computer — open it there",
  );
  const lookup = calls.find((c) => c.command === "coding_session_file_refs");
  assert.equal(lookup.args.request.isLocalProvider, false);
});

test("a path the host looked for and did not find says so; an unasked one claims nothing", async () => {
  // `src/unasked.ts:9` is rendered but absent from the text the provider
  // collected from, so the host was never asked about it.
  const view = await mount({
    codes: ["src/missing.ts:3", "src/unasked.ts:9"],
    text: "Look at `src/missing.ts:3`.",
  });
  const plain = [...view.container.querySelectorAll("[data-file-ref-plain]")];
  assert.equal(plain.length, 1);
  assert.equal(plain[0].textContent, "src/missing.ts:3");
  assert.equal(
    plain[0].getAttribute("data-file-ref-reason"),
    "Not found in this session's folder",
  );
});

test("a relative link href is a chip; an external link is untouched", async () => {
  const view = await mount({ codes: [], link: "desktop/src/app/App.tsx#L42" });
  const chip = view.container.querySelector("[data-file-chip]");
  assert.ok(chip);
  assert.equal(chip.getAttribute("data-file-ref-candidate"), CANDIDATE);
  const external = await mount({ codes: [], link: "https://example.com/a.ts" });
  assert.equal(
    external.container.querySelectorAll("[data-file-chip]").length,
    0,
  );
  assert.ok(
    external.container.querySelector('a[href="https://example.com/a.ts"]'),
  );
});

test("an unknown signer is never called 'another computer'", async () => {
  const { presentScopeAnswer } = await import("../useFileRefResolution.ts");
  const remote = {
    where: "notLocal",
    reason: "Written on another computer — open it there",
    refs: {},
  };
  assert.equal(presentScopeAnswer(remote, false, false).where, "notLocal");
  assert.equal(presentScopeAnswer(remote, null, false).where, "unknown");
  const local = presentScopeAnswer(remote, true, false);
  assert.equal(local.where, "notRecorded");
  assert.doesNotMatch(local.reason, /another computer/);
});

test("the candidate collector skips fences and reads code spans and link hrefs", async () => {
  const { collectFileRefCandidates } = await import(
    "../useFileRefResolution.ts"
  );
  const text = [
    "See `desktop/src/app/App.tsx:42` and [guide](docs/guide.md#L3).",
    "```ts",
    "import x from `src/inside-fence.ts`;",
    "```",
    "Not `origin/main`, not [site](https://example.com/a.ts).",
  ].join("\n");
  assert.deepEqual(collectFileRefCandidates(text), [
    "desktop/src/app/App.tsx:42",
    "docs/guide.md:3",
  ]);
});

const SRC = fileURLToPath(new URL("../../../", import.meta.url));

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx)$/.test(name)) out.push(full);
  }
  return out;
}

test("fileRefContext (which can hold a full path) is read only by the chip renderers", () => {
  // Risk 1 of the spec: a composer or event builder that read this context
  // could put a host path into a signed event. Every importer is listed here;
  // a new one — above all any `lib/*Command*` file or event builder — fails.
  const allowed = new Set([
    "features/coding-sessions/useFileRefResolution.ts",
    "shared/ui/markdown/FileChip.tsx",
    "shared/ui/markdown/FileLinkAnchor.tsx",
    "shared/ui/markdown/MarkdownCode.tsx",
  ]);
  const importers = walk(SRC)
    .filter((file) => /fileRefContext/.test(readFileSync(file, "utf8")))
    .map((file) => path.relative(SRC, file).split(path.sep).join("/"))
    .filter((file) => file !== "shared/ui/markdown/fileRefContext.ts");
  for (const file of importers) {
    assert.ok(
      allowed.has(file),
      `unexpected importer of fileRefContext: ${file}`,
    );
    assert.doesNotMatch(file, /lib\/[^/]*Command[^/]*$/);
    assert.doesNotMatch(file, /(eventBuilder|buildEvent|Composer)/i);
  }
  const commandFiles = walk(
    path.join(SRC, "features/coding-sessions/lib"),
  ).filter((file) => /Command/.test(path.basename(file)));
  assert.ok(
    commandFiles.length > 0,
    "the guard is looking at real command files",
  );
  for (const file of commandFiles) {
    const source = readFileSync(file, "utf8");
    assert.doesNotMatch(
      source,
      /fileRefContext|useFileRefResolution|tauriCodingSessionFileRefs/,
    );
  }
});

test("messages of one execution share one lookup; more than 256 candidates split", async () => {
  const { readFileRefScope, subscribeFileRefResolution } = await import(
    "../useFileRefResolution.ts"
  );
  const scope = {
    channelId: CHANNEL_ID,
    providerSessionId: SESSION_ID,
    projectRef: null,
    isHiredSeat: false,
    isLocalProvider: true,
  };
  const asked = [];
  const resolver = (_scope, candidates) => {
    asked.push(candidates.length);
    return Promise.resolve(localAnswer({ candidates }));
  };
  let changes = 0;
  const onChange = () => {
    changes += 1;
  };
  const many = Array.from({ length: 300 }, (_, i) => `desktop/f${i}.ts:1`);
  const stops = [
    subscribeFileRefResolution(scope, many.slice(0, 150), onChange, resolver),
    subscribeFileRefResolution(scope, many.slice(100), onChange, resolver),
  ];
  assert.equal(readFileRefScope(scope).where, "pending");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(asked, [256, 44], "one batch, chunked at the host's cap");
  assert.ok(changes > 0);
  const state = readFileRefScope(scope);
  assert.equal(state.where, "thisComputer");
  assert.equal(Object.keys(state.refs).length, 300);
  // A remount asks nothing new.
  stops.push(subscribeFileRefResolution(scope, many, onChange, resolver));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(asked, [256, 44]);
  for (const stop of stops) stop();
});

test("file-type icons: React, Markdown, folder, well-known names, fallback", async () => {
  const { fileTypeIcon } = await import("../../../shared/ui/fileTypeIcon.ts");
  assert.equal(fileTypeIcon("desktop/src/app/App.tsx").family, "react");
  assert.equal(fileTypeIcon("plans/x.md").family, "markdown");
  assert.equal(fileTypeIcon("crates/a/src/lib.rs").family, "rust");
  assert.equal(fileTypeIcon("desktop/src/").family, "folder");
  assert.equal(fileTypeIcon("desktop/src", true).family, "folder");
  assert.equal(fileTypeIcon("Dockerfile").family, "docker");
  assert.equal(fileTypeIcon("desktop/package.json").family, "package");
  assert.equal(fileTypeIcon("LICENSE").family, "license");
  assert.equal(fileTypeIcon("notes/unknown.qqq").family, "file");
  assert.equal(fileTypeIcon(".env").family, "file");
});

test("the E2E bridge answers per session, records actions, and passes on unconfigured", async () => {
  const { handleFileRefsMockCommand } = await import(
    "../../../testing/e2eBridgeFileRefs.ts"
  );
  const config = {
    mock: {
      codingSessionFileRefs: {
        bySession: {
          [SESSION_ID]: {
            where: "thisComputer",
            refs: {
              [CANDIDATE]: {
                exists: true,
                relativePath: "a",
                fullPath: FULL_PATH,
              },
            },
          },
        },
      },
    },
  };
  assert.equal(
    await handleFileRefsMockCommand(
      "coding_session_file_refs",
      {},
      { mock: {} },
    ),
    null,
  );
  assert.equal(await handleFileRefsMockCommand("other", {}, config), null);
  const looked = await handleFileRefsMockCommand(
    "coding_session_file_refs",
    {
      request: {
        providerSessionId: SESSION_ID,
        candidates: [CANDIDATE, "x/y.ts"],
      },
    },
    config,
  );
  assert.equal(looked.value.where, "thisComputer");
  assert.equal(looked.value.refs[CANDIDATE].exists, true);
  assert.equal(looked.value.refs["x/y.ts"].exists, false);
  const opened = await handleFileRefsMockCommand(
    "coding_session_open_file_ref",
    { request: { providerSessionId: SESSION_ID, candidate: CANDIDATE } },
    config,
  );
  assert.deepEqual(opened, { handled: true, value: null });
  await assert.rejects(
    handleFileRefsMockCommand(
      "coding_session_reveal_file_ref",
      { request: { providerSessionId: SESSION_ID, candidate: "x/y.ts" } },
      config,
    ),
    /no longer/,
  );
  const recorded = window.__BEEKEEPER_E2E_FILE_REF_CALLS__.map(
    (c) => c.command,
  );
  assert.deepEqual(recorded, [
    "coding_session_file_refs",
    "coding_session_open_file_ref",
    "coding_session_reveal_file_ref",
  ]);
});
