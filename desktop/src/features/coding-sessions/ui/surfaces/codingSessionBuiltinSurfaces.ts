import { codingSessionSurfaceAgents } from "./CodingSessionSurfaceAgents";
import { codingSessionSurfaceBrowser } from "./CodingSessionSurfaceBrowser";
import { codingSessionSurfaceDevice } from "./CodingSessionSurfaceDevice";
import { codingSessionSurfaceDiff } from "./CodingSessionSurfaceDiff";
import { codingSessionSurfaceFiles } from "./CodingSessionSurfaceFiles";
import { codingSessionSurfaceLanding } from "./CodingSessionSurfaceLanding";
import { codingSessionSurfaceMissionAudit } from "./CodingSessionSurfaceMissionAudit";
import { codingSessionSurfaceMissionContext } from "./CodingSessionSurfaceMissionContext";
import { codingSessionSurfaceMissionInspector } from "./CodingSessionSurfaceMissionInspector";
import { codingSessionSurfacePeople } from "./CodingSessionSurfacePeople";
import { codingSessionSurfacePlan } from "./CodingSessionSurfacePlan";
import { codingSessionSurfacePulse } from "./CodingSessionSurfacePulse";
import { codingSessionSurfaceTerminal } from "./CodingSessionSurfaceTerminal";
import {
  type CodingSessionSurfaceDefinition,
  type CodingSessionSurfaceRegistry,
  createCodingSessionSurfaceRegistry,
  readCodingSessionE2eExtraSurfaces,
} from "./codingSessionSurfaceRegistry";

/**
 * Every built-in surface, one line each (DB1). A new surface is a definition
 * file under `ui/surfaces/` plus one line here; nothing else is edited.
 * Order on screen comes from each definition's `order`, not from this list.
 */
export const CODING_SESSION_BUILTIN_SURFACES: readonly CodingSessionSurfaceDefinition[] =
  [
    codingSessionSurfaceMissionInspector,
    codingSessionSurfaceMissionContext,
    codingSessionSurfaceMissionAudit,
    codingSessionSurfaceAgents,
    codingSessionSurfaceDiff,
    codingSessionSurfaceTerminal,
    codingSessionSurfaceFiles,
    codingSessionSurfacePlan,
    codingSessionSurfaceLanding,
    codingSessionSurfacePeople,
    codingSessionSurfacePulse,
    codingSessionSurfaceBrowser,
    codingSessionSurfaceDevice,
  ];

// Definitions only — static code fixed at load, holding no community data —
// so this cache needs no reset in `resetCommunityState()`.
let registry: CodingSessionSurfaceRegistry | null = null;

/**
 * The app's registry: the built-ins, plus a `--mode e2e` build's declared
 * extra surfaces. Built on first use so a duplicate throws where it is read.
 */
export function codingSessionSurfaceRegistry(): CodingSessionSurfaceRegistry {
  registry ??= createCodingSessionSurfaceRegistry([
    ...CODING_SESSION_BUILTIN_SURFACES,
    ...readCodingSessionE2eExtraSurfaces(),
  ]);
  return registry;
}
