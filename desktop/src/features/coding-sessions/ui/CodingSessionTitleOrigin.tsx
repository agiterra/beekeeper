import { useUserProfileQuery } from "@/features/profile/hooks";
import type { CoordinatedSessionNameOrigin } from "@/shared/coordination/sessionCoordinationNames";

import {
  SessionNameOriginMarker,
  sessionNameOriginSentence,
} from "./SessionNameOriginMarker";

/**
 * Whose words a session name is, as the session surfaces carry it (SV-31).
 *
 * A {@link import("../lib/codingSessionTitle").CodingSessionDisplayName}
 * satisfies it, and so does a shelf entry's `labelOrigin`. Every field is
 * optional so a plain founder-keyed 44229 map (no origin at all) reads as "no
 * marker" rather than failing to type-check: absence is never upgraded into
 * a claim that a model wrote the name.
 */
export type CodingSessionTitleOriginFacts = {
  origin?: "person" | "generated";
  model?: string | null;
  signerPubkey?: string | null;
};

/** The marker's input, or null when there is nothing to mark. */
export function codingSessionTitleOrigin(
  facts: CodingSessionTitleOriginFacts | null | undefined,
): CoordinatedSessionNameOrigin | null {
  if (facts?.origin !== "generated") return null;
  return {
    origin: "generated",
    model: facts.model?.trim() || null,
    signerPubkey: facts.signerPubkey?.toLowerCase() || null,
  };
}

/**
 * The muted `text-2xs` "Auto-named" marker beside a session name in the
 * sidebar shelf, the channel Sessions menu and the founded page
 * (`data-testid="coding-session-title-origin"`). Its tooltip reads "Named
 * automatically from the first message by <provider> · <model>", the same
 * sentence Pulse and Agent Progress show, so one generated title reads the
 * same on every desktop surface. A person's name, an unnamed session and a
 * map that carries no origin render nothing.
 */
export function CodingSessionTitleOrigin({
  name,
  testId,
}: {
  name: CodingSessionTitleOriginFacts | null | undefined;
  testId?: string;
}) {
  const origin = codingSessionTitleOrigin(name);
  if (!origin) return null;
  return <SessionNameOriginMarker origin={origin} testId={testId} />;
}

/**
 * The same attribution as one muted line, for a surface that has room to say
 * it outright — the rename dialog, which says whose words the name is
 * whatever the header beside it shows. Renders nothing for a person's name.
 */
export function CodingSessionTitleAttribution({
  name,
  testId = "coding-session-title-attribution",
}: {
  name: CodingSessionTitleOriginFacts | null | undefined;
  testId?: string;
}) {
  const origin = codingSessionTitleOrigin(name);
  const profile = useUserProfileQuery(origin?.signerPubkey ?? undefined);
  if (!origin) return null;
  return (
    <p className="text-2xs text-muted-foreground" data-testid={testId}>
      {sessionNameOriginSentence(origin, profile.data?.displayName ?? null)}.
    </p>
  );
}
