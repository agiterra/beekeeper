import type { CodingSessionMissionStateInput } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";

/**
 * **Removed surface — staged deletion.**
 *
 * The pinned Mission transaction card sat above the first stream row and
 * restated, out of chronological order, the same signed transactions the
 * stream now renders in place (`CodingSessionMissionTransactionRow`), plus a
 * state line the Inspector now owns (`CodingSessionMissionStatePanel`). It was
 * one of five bordered chrome bands stacked before any narrative, and its
 * canonical-chain list was the third copy of the same handoff.
 *
 * This lane may not edit `CodingSessionUmbrellaWorkspace.tsx`, which still
 * mounts this component, so the deletion lands in two steps: the component
 * renders nothing now, and the finalizer deletes **both** this file and the
 * mount at `CodingSessionUmbrellaWorkspace.tsx` in the same commit. Nothing
 * imports it after that. See REPORT-U.md § Finalizer wiring.
 */
export function CodingSessionMissionTransactionCard(_props: {
  state: CodingSessionMissionStateInput;
}) {
  return null;
}
