/**
 * Which repository the Land control should read, when a session's own
 * creates named none but the session belongs to a project — LANE-L20 item 2.
 *
 * Finding 38 fixed the write path (new creates name their repository when
 * the launch can resolve one), but a session created before that fix, or by
 * a client that never resolves one, still carries `repoRef: null` forever —
 * a create is signed once. This is the *read*-side fallback for those:
 * exactly one repository on the session's project names it, disclosed as
 * inferred rather than passed off as the session's own claim; two or more
 * name none, because guessing among them would gate a push against the
 * wrong repository's rules — the same reasoning finding 38's write path
 * already applies, just one step later.
 */

export type CodingSessionMissionLandRepoInference =
  | { kind: "none" }
  | { kind: "inferred"; repoRef: string }
  | { kind: "multiple"; count: number };

/**
 * Pure: given the project's own repository addresses (`30617:<owner>:<d>`,
 * already deduplicated by the caller's read), decide the fallback.
 */
export function inferCodingSessionMissionLandRepo(
  projectRepoAddresses: readonly string[],
): CodingSessionMissionLandRepoInference {
  if (projectRepoAddresses.length === 1) {
    return { kind: "inferred", repoRef: projectRepoAddresses[0] };
  }
  if (projectRepoAddresses.length >= 2) {
    return { kind: "multiple", count: projectRepoAddresses.length };
  }
  return { kind: "none" };
}
