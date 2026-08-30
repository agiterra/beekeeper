export type CodingSessionParticipantAccent = {
  dot: string;
  border: string;
  soft: string;
  text: string;
};

/**
 * Existing Beekeeper identity palette with attention hues deliberately
 * excluded. State owns amber and destructive; identity never competes with it.
 */
const PARTICIPANT_ACCENTS: readonly CodingSessionParticipantAccent[] = [
  {
    dot: "bg-sky-500",
    border: "border-sky-500/55",
    soft: "bg-sky-500/10",
    text: "text-sky-700 dark:text-sky-300",
  },
  {
    dot: "bg-violet-500",
    border: "border-violet-500/55",
    soft: "bg-violet-500/10",
    text: "text-violet-700 dark:text-violet-300",
  },
  {
    dot: "bg-emerald-500",
    border: "border-emerald-500/55",
    soft: "bg-emerald-500/10",
    text: "text-emerald-700 dark:text-emerald-300",
  },
  {
    dot: "bg-primary",
    border: "border-primary/55",
    soft: "bg-primary/10",
    text: "text-primary",
  },
];

/** Stable identity accent shared by Mission participant surfaces. */
export function codingSessionParticipantAccent(
  executionKey: string,
): CodingSessionParticipantAccent {
  let hash = 0;
  for (let index = 0; index < executionKey.length; index += 1) {
    hash = (hash * 31 + executionKey.charCodeAt(index)) >>> 0;
  }
  return (
    PARTICIPANT_ACCENTS[hash % PARTICIPANT_ACCENTS.length] ??
    PARTICIPANT_ACCENTS[0]
  );
}
