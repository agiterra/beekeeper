import { useUserProfileQuery } from "@/features/profile/hooks";
import type { CoordinatedSessionNameOrigin } from "@/shared/coordination/sessionCoordinationNames";
import { truncatePubkey } from "@/shared/lib/pubkey";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

/**
 * The muted "Auto-named" marker beside a session name on a row that reads the
 * shared coordination fold — Pulse session cards and Agent Progress lanes
 * (SV-31).
 *
 * A generated title (44252) is a model's words, so it never reads as a
 * person's name: the marker says so, and its tooltip names the provider that
 * wrote it and the model. A person's name (`person`), an unnamed session and
 * an unknown origin render nothing — no marker is ever guessed.
 *
 * The tooltip names the signer by profile when one resolves, and always by its
 * truncated key, so the attribution stays checkable. The profile is read only
 * while the tooltip is open: Radix mounts the content on open.
 *
 * A hover tooltip alone would leave keyboard and screen-reader users without
 * the attribution, and the marker sits inside row buttons, so it cannot take
 * focus itself. Its accessible name therefore carries the whole sentence —
 * with the signer's truncated key and the model, read without a profile fetch
 * per row — and the row button's name includes it. The visible text stays
 * "Auto-named".
 */
export function SessionNameOriginMarker({
  origin,
  testId = "coding-session-title-origin",
}: {
  origin: CoordinatedSessionNameOrigin | null | undefined;
  testId?: string;
}) {
  if (origin?.origin !== "generated") return null;
  return (
    // Its own provider: these rows render outside any app-level tooltip
    // provider in tests and in some panels, and Radix requires one.
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            aria-label={sessionNameOriginAccessibleLabel(origin)}
            className="shrink-0 cursor-default text-2xs font-normal text-muted-foreground"
            data-testid={testId}
            // `img` makes the label valid ARIA on an inline element (a plain
            // span's aria-label is prohibited and ignored by some readers).
            role="img"
          >
            Auto-named
          </span>
        </TooltipTrigger>
        <TooltipContent
          className="max-w-xs"
          data-testid={`${testId}-detail`}
          side="top"
        >
          <SessionNameOriginDetail origin={origin} />
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function SessionNameOriginDetail({
  origin,
}: {
  origin: CoordinatedSessionNameOrigin;
}) {
  const signer = origin.signerPubkey;
  const profile = useUserProfileQuery(signer ?? undefined);
  return (
    <>{sessionNameOriginSentence(origin, profile.data?.displayName ?? null)}</>
  );
}

/**
 * "Named automatically from the first message by <provider> · <model>".
 * Exported for tests; the provider is its profile name and truncated key when
 * the name is known, the truncated key alone otherwise.
 */
export function sessionNameOriginSentence(
  origin: CoordinatedSessionNameOrigin,
  signerDisplayName: string | null,
): string {
  const signer = origin.signerPubkey;
  const name = signerDisplayName?.trim() || null;
  const who =
    signer === null
      ? "the session's provider"
      : name
        ? `${name} (${truncatePubkey(signer)})`
        : truncatePubkey(signer);
  const model = origin.model ? ` · ${origin.model}` : "";
  return `Named automatically from the first message by ${who}${model}`;
}

/**
 * The marker's accessible name: "Auto-named: Named automatically from the
 * first message by <key> · <model>". Exported for tests.
 */
export function sessionNameOriginAccessibleLabel(
  origin: CoordinatedSessionNameOrigin,
): string {
  return `Auto-named: ${sessionNameOriginSentence(origin, null)}`;
}
