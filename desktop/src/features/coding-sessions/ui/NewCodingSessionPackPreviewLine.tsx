import type { CodingSessionPackStatusResult } from "@/features/coding-sessions/lib/codingSessionPackStatus";

/**
 * "This is the pack this seat would stage" — the create-dialog's own
 * preview of LANE-L23's staging rule, sourced from
 * {@link import("@/features/coding-sessions/lib/useCodingSessionPackStatusPreview").useCodingSessionPackStatusPreview}.
 *
 * `preview === null` — no project known yet, no role typed, the probe is in
 * flight, or (today, always, until Lane B lands `coding_session_pack_status`)
 * the probe failed — renders nothing. A form that cannot answer "what pack
 * would this be" must not guess one.
 */
export function NewCodingSessionPackPreviewLine({
  preview,
}: {
  preview: CodingSessionPackStatusResult | null;
}) {
  if (preview === null) return null;
  return (
    <p
      className="text-2xs text-muted-foreground"
      data-testid="new-coding-session-pack-preview"
    >
      {preview.note}
    </p>
  );
}
