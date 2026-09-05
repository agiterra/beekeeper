/**
 * The one reader of a project's pack source for a seat, shared by the launch
 * dialog's preview (`codingSessionPackStatus.ts`) and the real staging call
 * (`codingSessionSeatedCreate.ts`, on launch, hire and team launch).
 *
 * Finding 84: the preview read the project's kind:30624 and the staging call
 * did not, so the dialog promised a pack from the project's repository and
 * the seat was staged from this computer's copy. Both now go through this
 * function, so the answer the dialog shows is the answer the host stages.
 */
import { fetchProjectPackSource } from "@/features/projects-container/lib/projectPackSource";

import type { CodingSessionProjectPackSource } from "./codingSessionActorSeatCustody";
import { codingSessionSeatPackSource } from "./codingSessionSeatedCreate";

/**
 * The project's newest 30624 in the shape the host's staging and preview
 * commands take, or `null` when the project publishes none.
 *
 * @throws when the relay could not be read. Callers decide what that means —
 * the preview renders nothing, the create refuses rather than stage the wrong
 * pack quietly.
 */
export async function fetchCodingSessionSeatPackSource(
  projectRef: string,
): Promise<CodingSessionProjectPackSource | null> {
  return codingSessionSeatPackSource(await fetchProjectPackSource(projectRef));
}
