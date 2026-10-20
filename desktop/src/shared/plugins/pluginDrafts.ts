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
 * Nothing is signed until every draft has been checked: a kind this client
 * signs for plugins, and exactly one `a` tag naming the project the call
 * named. The relay checked too; a client never signs on trust alone.
 */
import type { PluginDraft } from "./pluginClient";

/** The kinds the desktop will sign on a plugin's behalf: NIP-TD only. */
export const PLUGIN_SIGNABLE_KINDS: readonly number[] = [44248];

export type DraftEffects = {
  /** Sign and publish one event; resolves once the relay accepted it. */
  publish: (draft: {
    kind: number;
    content: string;
    createdAt: number;
    tags: string[][];
  }) => Promise<{ id: string }>;
};

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

/** Refuse a draft this client will not sign. */
export function checkDraft(draft: PluginDraft, project: string): void {
  if (!PLUGIN_SIGNABLE_KINDS.includes(draft.kind)) {
    throw new Error(
      `The plugin asked to sign a kind ${draft.kind} event; this app signs only kinds ${PLUGIN_SIGNABLE_KINDS.join(", ")} for plugins. Nothing was saved.`,
    );
  }
  const scopes = draft.tags.filter((tag) => tag[0] === "a");
  if (scopes.length !== 1 || scopes[0]?.[1] !== project) {
    throw new Error(
      "The plugin asked to sign a change outside this project. Nothing was saved.",
    );
  }
}

export async function executePluginDrafts(
  drafts: readonly PluginDraft[],
  project: string,
  effects: DraftEffects,
): Promise<DraftRun> {
  for (const draft of drafts) checkDraft(draft, project);
  const published: DraftRun["published"] = [];
  const failed: DraftRun["failed"] = [];
  const savedGroups = new Set<string>();
  const skipped = new Set<string>();
  for (const draft of drafts) {
    if (skipped.has(draft.group)) continue;
    try {
      const event = await effects.publish({
        kind: draft.kind,
        content: draft.content,
        createdAt: draft.createdAt,
        tags: draft.tags,
      });
      published.push({ eventId: event.id, group: draft.group });
      savedGroups.add(draft.group);
    } catch (error) {
      const reason = reasonOf(error);
      if (error instanceof DraftUnconfirmedError) {
        const index = drafts.indexOf(draft);
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
