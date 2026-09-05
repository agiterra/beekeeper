/**
 * Production custody seam for a seated **reconnect**: staging, cleanup, and
 * the project's pack source.
 *
 * Split from `codingSessionResumeSeat.ts` so the mapping stays loadable by a
 * node test while this side keeps the Tauri and relay imports. Module-level so
 * it is reference-stable — the composer's resume callback depends on it, and a
 * fresh object each render would rebuild that callback every time.
 *
 * The `fetchPackSource` entry is half of finding 85's fix: the composer's
 * default custody object had only `stageSeat` and `clearSeat`, so a resume
 * could not have read a project's kind:30624 even had it been told which
 * project — the host was always handed `null` and always staged this
 * computer's copy.
 */
import {
  clearCodingSessionActorSeat,
  stageCodingSessionActorSeat,
} from "./codingSessionActorSeatCustody";
import type { CodingSessionSeatCustody } from "./codingSessionSeatedCreate";
import { fetchCodingSessionSeatPackSource } from "./codingSessionSeatPackSource";

export const codingSessionResumeSeatDeps: CodingSessionSeatCustody = {
  stageSeat: stageCodingSessionActorSeat,
  clearSeat: clearCodingSessionActorSeat,
  fetchPackSource: fetchCodingSessionSeatPackSource,
};
