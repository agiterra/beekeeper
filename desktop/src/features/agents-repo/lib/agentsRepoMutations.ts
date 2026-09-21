/**
 * Writing draft ops: one signed kind 44250 event per save, stamped past
 * the latest op on the same path, appended to the cached read at once and
 * reconciled by the invalidation that follows.
 *
 * The head check is the honesty of the whole feature: a save is refused
 * here, before it is signed, when the head it was edited from is no longer
 * the head — so nobody's text is silently layered over. The text stays in
 * the editor; the person reloads and re-applies.
 */
import * as React from "react";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_AGENTS_REPO_DRAFT_OP,
  KIND_DELETION,
} from "@/shared/constants/kinds";

import {
  type DraftOp,
  archiveCounterpart,
  draftMessageError,
  draftOpPaths,
  draftOpTags,
  draftPathClass,
  draftTextError,
  encodeDraftOpContent,
} from "./agentsRepoDraftOp";
import {
  type AgentsRepoDraftsRead,
  RELAY_TIMESTAMP_DRIFT_SECS,
  agentsRepoDraftsQueryKey,
  draftsReadFromEvents,
} from "./agentsRepoQueries";

const MAX_FUTURE_SKEW_SECS = RELAY_TIMESTAMP_DRIFT_SECS - 10;

/** Never below `headSecs + 1`, never below now — the CLI's rule. */
export function nextDraftCreatedAt(headSecs: number, nowSecs: number): number {
  return Math.max(headSecs + 1, nowSecs);
}

/** The head of `path` in the cached read, or null. */
function cachedHeadId(
  read: AgentsRepoDraftsRead | undefined,
  path: string,
): string | null {
  return (
    read?.digest.paths.find((entry) => entry.path === path)?.head.id ?? null
  );
}

/**
 * Refuse a save whose `prev` is not the current head of its path. `openedOn`
 * is the head the editor was opened on (null = none).
 */
export function headConflict(
  read: AgentsRepoDraftsRead | undefined,
  path: string,
  openedOn: string | null,
  authorName: (pubkey: string) => string,
): string | null {
  const entry = read?.digest.paths.find((e) => e.path === path);
  const head = entry?.head ?? null;
  if ((head?.id ?? null) === openedOn) return null;
  if (head === null) {
    return "The draft you were editing was committed or withdrawn; reload to start from what is there now.";
  }
  return `${authorName(head.author)} saved a newer draft; reload to see it. Your text is kept here until you do.`;
}

/** Sign and publish one op, then append it to the cached read. */
export async function publishDraftOp(
  queryClient: QueryClient,
  coordinate: string,
  op: DraftOp,
): Promise<RelayEvent> {
  const key = agentsRepoDraftsQueryKey(coordinate);
  const read = queryClient.getQueryData<AgentsRepoDraftsRead>(key);
  const targets =
    op.content.op === "commit.record" ? [""] : draftOpPaths(op.content);
  const head = Math.max(0, ...targets.map((t) => read?.latestByPath[t] ?? 0));
  const now = Math.floor(Date.now() / 1_000);
  const createdAt = nextDraftCreatedAt(head, now);
  if (createdAt > now + MAX_FUTURE_SKEW_SECS) {
    throw new Error(
      `The latest change to this file is stamped ${head - now}s in the future; try again in a moment.`,
    );
  }
  const event = await signRelayEvent({
    kind: KIND_AGENTS_REPO_DRAFT_OP,
    content: encodeDraftOpContent(op),
    createdAt,
    tags: draftOpTags(coordinate, op),
  });
  const accepted = await relayClient.publishEvent(
    event,
    "Timed out saving the draft.",
    "The relay refused the draft.",
  );
  queryClient.setQueryData<AgentsRepoDraftsRead>(key, (previous) =>
    draftsReadFromEvents(
      coordinate,
      op.repo,
      [...(previous?.events ?? []), accepted],
      previous?.truncated ?? false,
    ),
  );
  void queryClient.invalidateQueries({ queryKey: key });
  return accepted;
}

export type SaveDraftInput = {
  path: string;
  text: string;
  /** The blob on `main` the editor started from (null = new file). */
  base: string | null;
  baseCommit: string | null;
  /** The head the editor was opened on (null = none). */
  openedOn: string | null;
  message: string | null;
};

export type AgentsRepoMutations = {
  saveDraft: (input: SaveDraftInput) => Promise<RelayEvent>;
  moveDraft: (input: {
    path: string;
    base: string;
    baseCommit: string | null;
    openedOn: string | null;
    message: string | null;
  }) => Promise<RelayEvent>;
  deleteDraft: (input: {
    path: string;
    base: string;
    baseCommit: string | null;
    openedOn: string | null;
    message: string | null;
  }) => Promise<RelayEvent>;
  /** NIP-09: only the author's own draft. */
  withdrawDraft: (draftId: string) => Promise<void>;
  recordCommit: (input: {
    commit: string;
    paths: string[];
    drafts: string[];
    message: string | null;
  }) => Promise<RelayEvent>;
};

/** Reference-stable mutation callbacks for one project and repository. */
export function useAgentsRepoMutations(
  coordinate: string | null,
  repo: string | null,
  authorName: (pubkey: string) => string,
): AgentsRepoMutations {
  const queryClient = useQueryClient();
  return React.useMemo(() => {
    const require = (): { coordinate: string; repo: string } => {
      if (coordinate === null || repo === null) {
        throw new Error(
          "This project has no agents repository to draft against yet.",
        );
      }
      return { coordinate, repo };
    };
    const read = () =>
      queryClient.getQueryData<AgentsRepoDraftsRead>(
        agentsRepoDraftsQueryKey(coordinate ?? "none"),
      );
    const checkHead = (path: string, openedOn: string | null) => {
      const conflict = headConflict(read(), path, openedOn, authorName);
      if (conflict) throw new Error(conflict);
    };
    const checkMessage = (message: string | null) => {
      if (message !== null) {
        const error = draftMessageError(message);
        if (error) throw new Error(error);
      }
    };
    return {
      async saveDraft(input) {
        const { coordinate, repo } = require();
        const classified = draftPathClass(input.path);
        if (!classified.ok) throw new Error(classified.error);
        const textError = draftTextError(input.text);
        if (textError) throw new Error(textError);
        checkMessage(input.message);
        checkHead(input.path, input.openedOn);
        return publishDraftOp(queryClient, coordinate, {
          repo,
          message: input.message,
          content: {
            op: "file.put",
            path: input.path,
            text: input.text,
            base: input.base,
            baseCommit: input.baseCommit,
            prev: cachedHeadId(read(), input.path),
          },
        });
      },
      async moveDraft(input) {
        const { coordinate, repo } = require();
        const to = archiveCounterpart(input.path);
        if (to === null) {
          throw new Error(
            `${input.path} is not a role or plan that can be archived or restored.`,
          );
        }
        checkMessage(input.message);
        checkHead(input.path, input.openedOn);
        return publishDraftOp(queryClient, coordinate, {
          repo,
          message: input.message,
          content: {
            op: "file.move",
            path: input.path,
            to,
            base: input.base,
            baseCommit: input.baseCommit,
            prev: cachedHeadId(read(), input.path),
          },
        });
      },
      async deleteDraft(input) {
        const { coordinate, repo } = require();
        const classified = draftPathClass(input.path);
        if (!classified.ok) throw new Error(classified.error);
        if (classified.class === "root-file") {
          throw new Error(
            `${input.path} is a root file and cannot be deleted.`,
          );
        }
        checkMessage(input.message);
        checkHead(input.path, input.openedOn);
        return publishDraftOp(queryClient, coordinate, {
          repo,
          message: input.message,
          content: {
            op: "file.delete",
            path: input.path,
            base: input.base,
            baseCommit: input.baseCommit,
            prev: cachedHeadId(read(), input.path),
          },
        });
      },
      async withdrawDraft(draftId) {
        const { coordinate } = require();
        const event = await signRelayEvent({
          kind: KIND_DELETION,
          content: "",
          tags: [["e", draftId]],
        });
        await relayClient.publishEvent(
          event,
          "Timed out withdrawing the draft.",
          "The relay refused the withdrawal.",
        );
        const key = agentsRepoDraftsQueryKey(coordinate);
        queryClient.setQueryData<AgentsRepoDraftsRead>(key, (previous) =>
          previous
            ? draftsReadFromEvents(
                coordinate,
                previous.digest.repo,
                previous.events.filter((e) => e.id !== draftId),
                previous.truncated,
              )
            : previous,
        );
        void queryClient.invalidateQueries({ queryKey: key });
      },
      async recordCommit(input) {
        const { coordinate, repo } = require();
        checkMessage(input.message);
        return publishDraftOp(queryClient, coordinate, {
          repo,
          message: input.message,
          content: {
            op: "commit.record",
            commit: input.commit,
            paths: [...new Set(input.paths)],
            drafts: [...new Set(input.drafts)],
          },
        });
      },
    };
  }, [coordinate, repo, queryClient, authorName]);
}
