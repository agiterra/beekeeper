/**
 * Mock of the Files tab's host commands (`agents_repo_ls`, `agents_repo_read`,
 * `agents_repo_commit_drafts`; `desktop/src-tauri/src/managed_agents/agents_repo_read.rs`
 * and `agents_repo_commit.rs`), answered from a seed a spec plants on
 * `window.__BUZZ_E2E_AGENTS_REPO__` before the bridge installs. Unseeded, the
 * commands throw, so the tab discloses "could not read the agents
 * repository" rather than a fabricated tree — the same opt-in shape as
 * `projectAgentsInitByProject`.
 */

export type MockAgentsRepoSeed = {
  /** The listing `agents_repo_ls` answers with. */
  listing: {
    repo: string;
    branch: string;
    commit: string;
    syncedAt: string | null;
    entries: { path: string; blob: string; size: number; kind: string }[];
  };
  /** The text `agents_repo_read` answers for each path on `main`. */
  files: Record<string, string>;
  /**
   * The answer `agents_repo_commit_drafts` gives, in order, one per call;
   * the last repeats. Each is the host's whole result minus the echoed
   * `draftIds`, which the mock fills from the request.
   */
  commitResults: Record<string, unknown>[];
  /** Experimental reader failure/delay fixtures; no production transport. */
  explorerError?: string;
  explorerReadDelayMs?: number;
  explorerOutcomes?: Record<string, string>;
};

type CommitCall = { request: Record<string, unknown> };

declare global {
  interface Window {
    __BUZZ_E2E_AGENTS_REPO__?: MockAgentsRepoSeed;
    __BUZZ_E2E_AGENTS_REPO_COMMIT_CALLS__?: CommitCall[];
  }
}

function seed(command: string): MockAgentsRepoSeed {
  const value = window.__BUZZ_E2E_AGENTS_REPO__;
  if (!value) throw new Error(`Unsupported mocked Tauri command: ${command}`);
  return value;
}

/** Route one of the three commands; returns `undefined` for any other. */
export function handleMockAgentsRepoCommand(
  command: string,
  payload: unknown,
): unknown | undefined {
  switch (command) {
    case "memory_explorer_release":
      return null;
    case "memory_explorer_snapshot":
      if (seed(command).explorerError)
        throw new Error(seed(command).explorerError);
      return {
        token: "mock-snapshot",
        listing: structuredClone(seed(command).listing),
      };
    case "agents_repo_ls": {
      const { listing } = seed(command);
      return {
        ...listing,
        entries: listing.entries.map((entry) => ({ ...entry })),
      };
    }
    case "memory_explorer_read":
    case "agents_repo_read": {
      const { listing, files } = seed(command);
      const path = (payload as { path?: string }).path ?? "";
      const entry = listing.entries.find(
        (candidate) => candidate.path === path,
      );
      const text = files[path];
      if (command === "memory_explorer_read") {
        const mocked = seed(command);
        const outcome =
          mocked.explorerOutcomes?.[path] ??
          (entry && entry.size > 4 * 1024 * 1024 ? "too-large" : "on-main");
        if (mocked.explorerReadDelayMs || outcome !== "on-main") {
          return new Promise((resolve) =>
            setTimeout(
              () =>
                resolve({
                  path,
                  text: outcome === "on-main" ? text : null,
                  state: outcome,
                  blob: entry?.blob ?? null,
                  commit: listing.commit,
                  size: entry?.size ?? null,
                  syncedAt: listing.syncedAt,
                }),
              mocked.explorerReadDelayMs ?? 0,
            ),
          );
        }
      }
      if (!entry || text === undefined) {
        return {
          path,
          text: null,
          state: "not-on-main",
          blob: null,
          commit: listing.commit,
          size: null,
          syncedAt: listing.syncedAt,
        };
      }
      return {
        path,
        text,
        state: "on-main",
        blob: entry.blob,
        commit: listing.commit,
        size: entry.size,
        syncedAt: listing.syncedAt,
      };
    }
    case "agents_repo_commit_drafts": {
      const { commitResults } = seed(command);
      const request = ((payload as { request?: Record<string, unknown> })
        .request ?? {}) as Record<string, unknown>;
      window.__BUZZ_E2E_AGENTS_REPO_COMMIT_CALLS__ ??= [];
      const calls = window.__BUZZ_E2E_AGENTS_REPO_COMMIT_CALLS__;
      const index = Math.min(calls.length, commitResults.length - 1);
      calls.push({ request });
      const scripted = commitResults[index];
      if (!scripted) throw new Error("no scripted commit result");
      const drafts = (request.drafts as { id: string }[] | undefined) ?? [];
      return {
        pushed: "no",
        tipBefore: null,
        commit: null,
        tree: null,
        paths: [],
        actions: null,
        refusals: [],
        unknownReason: null,
        committerName: "Tyler",
        committerEmail: "e5ebc6cd@beekeeper.local",
        ...scripted,
        draftIds: drafts.map((draft) => draft.id),
      };
    }
    // Not buzz-core: the mock refuses only what 32cb99de broke (the opening
    // fence and the schema line), so a spec can drive both answers. The real
    // reader's rules are tested in `commands/plan_source.rs`.
    case "validate_plan_source": {
      const text = (payload as { text?: string }).text ?? "";
      return text.startsWith("---\n") &&
        /^schema: beekeeper-plan\/v1$/m.test(text)
        ? { code: null, path: null, message: null }
        : {
            code: "missing_frontmatter",
            path: "frontmatter",
            message: "a plan file opens with a `---` frontmatter fence",
          };
    }
    default:
      return undefined;
  }
}
