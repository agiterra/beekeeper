import type { PlanSourceCheck } from "@/shared/api/agentsRepoTypes";

import { draftPathClass } from "./agentsRepoDraftOp";

/**
 * Ledger 249(B). Run 2's `plans/kettle.md` was committed from the Files tab
 * with its opening `---` gone, its YAML flattened and `<text>`/`<n>`
 * stripped — the text a Markdown rendering of the plan copies as. The
 * Files path itself stores bytes verbatim (its first commit of the same plan,
 * 33061ea4, is byte-identical to the source); nothing stopped the second
 * one, because nothing read the plan before it landed. This does.
 */

/** Whether `path` holds plan source (`plans/<slug>.md`). */
export function isPlanSourcePath(path: string): boolean {
  const classified = draftPathClass(path);
  return classified.ok && classified.class === "plan";
}

/** One change the commit would land. */
type PlanChange = { op: string; path: string; text: string | null };

/**
 * Every plan the commit would land that `beekeeper-plan/v1` does not read,
 * each as one sentence naming the path, the reader's code and its words.
 * Empty when there is nothing to refuse.
 */
export async function planCommitRefusals(
  changes: readonly PlanChange[],
  validate: (text: string) => Promise<PlanSourceCheck>,
): Promise<string[]> {
  const refusals: string[] = [];
  for (const change of changes) {
    if (change.op !== "file.put" || !isPlanSourcePath(change.path)) continue;
    const check = await validate(change.text ?? "");
    if (check.code !== null) {
      refusals.push(
        `${change.path} is not a readable plan (${check.code} at ${check.path ?? "?"}: ${check.message ?? "no detail"}), so nothing was committed. Fix its source, or land it with \`bee agents-repo\`.`,
      );
    }
  }
  return refusals;
}
