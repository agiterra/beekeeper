/**
 * Project Pulse — the per-project coordination view.
 *
 * This feature owns kind 44240 decode/fold and its presentation. Session state
 * comes from the coding-session feature's own decoders and the digest fold
 * bound to `conformance/project-pulse-fold/`; nothing here re-derives a
 * session's status a second time.
 */
export { resetProjectPulseState } from "./lib/projectPulseCache";
export {
  PULSE_ACTIVE_WINDOW_SECONDS,
  PULSE_DIGEST_SCHEMA,
  foldProjectPulseDigest,
  type ProjectPulseDigest,
} from "./lib/pulseFold.ts";
export { useProjectPulseDigest } from "./lib/pulseQueries";
export { ProjectPulseCard } from "./ui/ProjectPulseCard";
export { ProjectPulseScreen } from "./ui/ProjectPulseScreen";
export { ProjectPulseView } from "./ui/ProjectPulseView";
