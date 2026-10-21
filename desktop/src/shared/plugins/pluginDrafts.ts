/**
 * Running a plugin write: sign each draft as the user and publish it through
 * ordinary ingest, in order, under the plugin's refusal policy
 * (`docs/plugins.md` § Drafts). The same rules as `bee`'s executor:
 *
 * - `abort` — stop. Nothing landed before it: the relay's refusal is the
 *   error, and nothing was saved. Something did: a
 *   {@link PluginPartialWriteError} with the draft's partial summary (or a
 *   generic line), so a half-applied change is reported as half-applied.
 * - `skip-group` — the group is reported failed, the rest of it skipped, and
 *   the next group still runs.
 * - **unconfirmed** — whatever the policy, a publish that timed out or lost
 *   its connection may still land. The run stops there and says so
 *   (`unconfirmed`, `notAttempted`) instead of reporting a refusal a caller
 *   would retry, which could mint a duplicate.
 *
 * Nobody reviews a draft before it is signed — the user acted, the app
 * signs — so this module is the last check, not a person. Nothing is signed
 * until every draft has been sealed here ({@link sealDraft}), whatever the
 * relay already did: a kind this client signs for plugins, a `createdAt`
 * inside the window ingest will still accept, only that kind's data tags, the `a` tag written here from the project the call named (a
 * proposed one may only echo it, once), the tag caps before and after, and
 * the kind's own decoder, whose re-derived tags must be exactly the sealed
 * ones. The same rules as `bee`'s `seal_draft` and the relay's plugin host.
 */
import {
  PROJECT_TODO_OP_KIND,
  decodeTodoOp,
  isTodoVisibility,
  todoOpTags,
} from "@/features/project-todos/lib/todoOp";

import type { PluginDraft } from "./pluginClient";

/** The kinds the desktop will sign on a plugin's behalf: NIP-TD only. */
export const PLUGIN_SIGNABLE_KINDS: readonly number[] = [PROJECT_TODO_OP_KIND];

/** The tag keys a plugin may propose for each signable kind; `a` is ours. */
const DATA_TAG_KEYS: Record<number, readonly string[]> = {
  [PROJECT_TODO_OP_KIND]: ["td-v", "td-op", "td-list", "td-vis", "td-item"],
};

/** The caps the relay's plugin host holds a draft's tags to. */
const MAX_DRAFT_TAGS = 64;
const MAX_TAG_VALUES = 8;
const MAX_TAG_VALUE_BYTES = 1024;
const MAX_DRAFT_TAG_BYTES = 16 * 1024;

/**
 * The window a draft's `createdAt` must sit in around this app's clock —
 * the one the relay's plugin host and `bee` hold too
 * (`beekeeper_nip_td::DRAFT_PAST_SECS` / `DRAFT_FUTURE_SECS`). Relay ingest
 * takes ±900 s of its clock, judged after the reply, the signing and the
 * publish; 600 s behind leaves 300 s for that. A stamp outside it is
 * refused, never re-stamped: it orders the op against its rivals.
 */
export const DRAFT_PAST_SECS = 600;
export const DRAFT_FUTURE_SECS = 890;

export type DraftEffects = {
  /** Sign and publish one event; resolves once the relay accepted it. */
  publish: (draft: SealedDraft) => Promise<{ id: string }>;
  /** This app's clock in Unix seconds; the system clock unless a test pins it. */
  now?: () => number;
};

const systemNow = () => Math.floor(Date.now() / 1000);

export type DraftRun = {
  published: { eventId: string; group: string }[];
  failed: { group: string; message: string }[];
  /** A draft whose fate is unknown; the run stopped at it. */
  unconfirmed: { group: string; message: string } | null;
  /** Groups after an unconfirmed draft that were never attempted. */
  notAttempted: string[];
};

/**
 * Thrown by a `publish` effect when the relay did not answer in time or the
 * connection dropped mid-publish: the event may or may not have landed.
 */
export class DraftUnconfirmedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DraftUnconfirmedError";
  }
}

/** A write that landed in part; its message says what landed. */
export class PluginPartialWriteError extends Error {
  readonly published: DraftRun["published"];

  constructor(message: string, published: DraftRun["published"]) {
    super(message);
    this.name = "PluginPartialWriteError";
    this.published = published;
  }
}

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** What the app signs for one draft: built here, not taken from the plugin. */
export type SealedDraft = {
  kind: number;
  content: string;
  createdAt: number;
  tags: string[][];
};

const utf8 = new TextEncoder();

function refuse(why: string): never {
  throw new Error(`The plugin asked to sign ${why}. Nothing was saved.`);
}

function checkTagCaps(stage: string, tags: readonly unknown[]): void {
  if (tags.length > MAX_DRAFT_TAGS) {
    refuse(`a change with ${tags.length} ${stage} tags`);
  }
  let total = 0;
  for (const tag of tags) {
    if (
      !Array.isArray(tag) ||
      tag.length === 0 ||
      tag.length > MAX_TAG_VALUES ||
      tag.some((value) => typeof value !== "string")
    ) {
      refuse(`a change with a malformed ${stage} tag`);
    }
    for (const value of tag as string[]) {
      const bytes = utf8.encode(value).length;
      if (bytes > MAX_TAG_VALUE_BYTES) {
        refuse(`a change with a ${stage} tag value over 1 KiB`);
      }
      total += bytes;
    }
  }
  if (total > MAX_DRAFT_TAG_BYTES) {
    refuse(`a change with ${total} bytes of ${stage} tags`);
  }
}

function sameTags(a: readonly string[][], b: readonly string[][]): boolean {
  const key = (tags: readonly string[][]) =>
    tags.map((tag) => JSON.stringify(tag)).sort();
  const [x, y] = [key(a), key(b)];
  return x.length === y.length && x.every((tag, i) => tag === y[i]);
}

/**
 * Rebuild a draft for signing, or refuse it. The scope is this client's: a
 * proposed `a` tag must name exactly `project`, at most once, and is
 * replaced; every other key must be one the kind allows; the kind's decoder
 * must accept the content and re-derive exactly the sealed tags, so a tag
 * can never route somewhere the content does not say.
 */
export function sealDraft(
  draft: PluginDraft,
  project: string,
  now: number = systemNow(),
): SealedDraft {
  const dataKeys = DATA_TAG_KEYS[draft.kind];
  if (!PLUGIN_SIGNABLE_KINDS.includes(draft.kind) || !dataKeys) {
    refuse(
      `a kind ${draft.kind} event; this app signs only kinds ${PLUGIN_SIGNABLE_KINDS.join(", ")} for plugins`,
    );
  }
  const createdAt: unknown = draft.createdAt;
  if (
    typeof createdAt !== "number" ||
    !Number.isSafeInteger(createdAt) ||
    createdAt < 0
  ) {
    refuse("a change without a whole-second createdAt");
  }
  if (
    createdAt < now - DRAFT_PAST_SECS ||
    createdAt > now + DRAFT_FUTURE_SECS
  ) {
    refuse(
      `a change stamped ${createdAt}, outside ${DRAFT_PAST_SECS} s behind to ${DRAFT_FUTURE_SECS} s ahead of this app's clock (${now})`,
    );
  }
  if (!Array.isArray(draft.tags)) refuse("a change without tags");
  checkTagCaps("proposed", draft.tags);
  const tags: string[][] = [["a", project]];
  let echoed = false;
  for (const tag of draft.tags) {
    const key = tag[0] ?? "";
    if (key === "a") {
      if (echoed || tag.length !== 2 || tag[1] !== project) {
        refuse("a change outside this project");
      }
      echoed = true;
    } else if (dataKeys.includes(key)) {
      tags.push([...tag]);
    } else {
      refuse(`a change carrying a ${JSON.stringify(key)} tag`);
    }
  }
  checkTagCaps("constructed", tags);
  if (typeof draft.content !== "string") refuse("a change without content");
  const visibility = tags.find((tag) => tag[0] === "td-vis")?.[1];
  if (!isTodoVisibility(visibility))
    refuse("a to-do change with no list visibility");
  const op = decodeTodoOp(draft.content, visibility);
  if ("error" in op) refuse(`a to-do change the app cannot read (${op.error})`);
  if (!sameTags(tags, todoOpTags(project, op))) {
    refuse("a to-do change whose tags do not match what it says");
  }
  return {
    kind: draft.kind,
    content: draft.content,
    createdAt,
    tags,
  };
}

export async function executePluginDrafts(
  drafts: readonly PluginDraft[],
  project: string,
  effects: DraftEffects,
): Promise<DraftRun> {
  // Every draft is sealed before the first is published, so one this app
  // refuses — anywhere in the list — means nothing is published. That
  // avoids a preventable partial write; it is not atomicity across the
  // network writes that follow.
  const now = (effects.now ?? systemNow)();
  const sealed = drafts.map((draft) => sealDraft(draft, project, now));
  const published: DraftRun["published"] = [];
  const failed: DraftRun["failed"] = [];
  const savedGroups = new Set<string>();
  const skipped = new Set<string>();
  for (const [index, draft] of drafts.entries()) {
    if (skipped.has(draft.group)) continue;
    try {
      const event = await effects.publish(sealed[index] as SealedDraft);
      published.push({ eventId: event.id, group: draft.group });
      savedGroups.add(draft.group);
    } catch (error) {
      const reason = reasonOf(error);
      if (error instanceof DraftUnconfirmedError) {
        const notAttempted = [
          ...new Set(
            drafts
              .slice(index + 1)
              .map((d) => d.group)
              .filter(
                (g) =>
                  g !== draft.group && !skipped.has(g) && !savedGroups.has(g),
              ),
          ),
        ];
        return {
          published,
          failed,
          unconfirmed: {
            group: draft.group,
            message: `${reason} It may still land; refresh before trying again.`,
          },
          notAttempted,
        };
      }
      if (draft.ifRefused === "skip-group") {
        failed.push({
          group: draft.group,
          message:
            draft.partialSummary && savedGroups.has(draft.group)
              ? `${draft.partialSummary} (${reason})`
              : reason,
        });
        skipped.add(draft.group);
        continue;
      }
      if (published.length === 0) throw error;
      throw new PluginPartialWriteError(
        `${
          draft.partialSummary ??
          `Only part of the change was confirmed: ${published.length} of ${drafts.length} signed changes landed`
        } (${reason})`,
        published,
      );
    }
  }
  return { published, failed, unconfirmed: null, notAttempted: [] };
}
