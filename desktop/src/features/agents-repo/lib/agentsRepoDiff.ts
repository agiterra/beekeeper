/**
 * A unified diff of a draft against `main`, for the editor's Diff tab,
 * rendered by the messages feature's `DiffViewer`.
 */
import { createTwoFilesPatch } from "diff";

export function draftPatch(
  path: string,
  mainText: string | null,
  draftText: string | null,
): string {
  return createTwoFilesPatch(
    `a/${path}`,
    `b/${path}`,
    mainText ?? "",
    draftText ?? "",
    mainText === null ? "not on main" : "main",
    draftText === null ? "removed" : "draft",
    { context: 3 },
  );
}
