import {
  formatSurfaceClock,
  type SurfaceNameOf,
  type SurfaceSnapshotCard,
  surfaceMachineName,
  surfacePersonName,
  surfaceSnapshotCommitLabel,
} from "@/features/coding-sessions/lib/codingSessionSurfaceSnapshot";
import { rewriteRelayUrl } from "@/shared/lib/mediaUrl";

/**
 * One 44253 surface snapshot as a card (WIRE-C5 § 6), shared by the Device
 * and Browser surfaces: the image, who took it, on which machine, when, at
 * which commit (or "commit not recorded"), the page (Browser), and who asked
 * for it. Every word is read from the signed record; nothing is guessed.
 */
export function CodingSessionSurfaceSnapshotCard({
  card,
  localProviderPubkey,
  nameOf,
}: {
  card: SurfaceSnapshotCard;
  localProviderPubkey?: string | null;
  nameOf?: SurfaceNameOf;
}) {
  const machine = surfaceMachineName({
    providerPubkey: card.machine,
    localProviderPubkey,
    nameOf,
  });
  return (
    <figure
      className="flex gap-2.5 rounded-md border border-border/60 bg-muted/20 p-2"
      data-snapshot-id={card.id}
      data-surface={card.surface}
      data-testid="surface-snapshot-card"
    >
      <img
        alt={card.alt || `Snapshot ${formatSurfaceClock(card.takenAt)}`}
        className="h-16 w-auto max-w-24 shrink-0 rounded-sm border border-border/40 object-contain"
        src={rewriteRelayUrl(card.url)}
      />
      <figcaption className="flex min-w-0 flex-col gap-0.5 text-2xs text-muted-foreground">
        <span
          className="truncate font-medium text-foreground"
          data-testid="surface-snapshot-card-signer"
        >
          {surfacePersonName(card.signer, nameOf)}
        </span>
        <span className="truncate" data-testid="surface-snapshot-card-machine">
          on {machine}
        </span>
        <span data-testid="surface-snapshot-card-time">
          {formatSurfaceClock(card.takenAt)}
        </span>
        <span className="truncate" data-testid="surface-snapshot-card-commit">
          {surfaceSnapshotCommitLabel(card.commit)}
        </span>
        {card.page !== null ? (
          <span className="truncate" data-testid="surface-snapshot-card-page">
            {card.page}
          </span>
        ) : null}
        {card.requestedBy !== null ? (
          <span
            className="truncate"
            data-testid="surface-snapshot-card-requested-by"
          >
            requested by {surfacePersonName(card.requestedBy, nameOf)}
          </span>
        ) : null}
      </figcaption>
    </figure>
  );
}
