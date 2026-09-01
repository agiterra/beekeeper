import {
  CODING_SESSION_MISSION_DELIVERY_ROW_LIMIT,
  codingSessionTeamWakeDeliveryCopy,
  type CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";
import { CodingSessionMissionDeliveryBadge } from "./CodingSessionMissionDeliveryBadge";

/**
 * The persistent home for team-wake delivery.
 *
 * Before this, whether a report's wake reached the lead — or was covered by
 * Desktop, or failed outright — existed only as a toast that had already gone
 * by the time anyone asked. Toasts stay a transient extra; this list is the
 * record. Newest first, bounded, and truncation is stated rather than silent.
 *
 * `duplicateRefusedCommandIds` is disclosure attached to the delivery that owns
 * the operation. A `DUPLICATE_OPERATION` refusal is the fence working — the
 * second command spent no turn — so it is counted, never rendered as a failure.
 */
export function CodingSessionMissionDeliveryList({
  deliveries,
  loading = false,
}: {
  /** `undefined` means no projection was supplied, which is not "none". */
  deliveries: readonly CodingSessionTeamWakeDelivery[] | undefined;
  /** Only annotates the unknown row; it never turns unknown into "none". */
  loading?: boolean;
}) {
  if (deliveries === undefined) {
    // No projection is `unknown`, whether or not evidence is still loading.
    // The earlier version said "No team wake deliveries observed." once
    // `loading` went false — a positive claim of observation made when nothing
    // observed anything, and the app's steady state until Lane D is wired.
    return (
      <p
        className="text-xs text-muted-foreground"
        data-loading={loading ? "true" : undefined}
        data-testid="mission-delivery-empty"
      >
        {codingSessionTeamWakeDeliveryCopy.unknown.detail}
      </p>
    );
  }
  if (deliveries.length === 0) {
    return (
      <p
        className="text-xs text-muted-foreground"
        data-testid="mission-delivery-empty"
      >
        No team wake deliveries observed.
      </p>
    );
  }
  const ordered = [...deliveries].sort(
    (left, right) => (right.observedAtMs ?? 0) - (left.observedAtMs ?? 0),
  );
  const shown = ordered.slice(0, CODING_SESSION_MISSION_DELIVERY_ROW_LIMIT);
  const hidden = ordered.length - shown.length;
  return (
    <div>
      <ul aria-label="Team wake delivery" className="space-y-2">
        {shown.map((delivery) => (
          <li
            className={missionRowClass(
              delivery.kind === "failed" ? "attention" : "standard",
              {
                tone: delivery.kind === "failed" ? "critical" : undefined,
              },
            )}
            data-kind={delivery.kind}
            data-testid="mission-delivery-row"
            key={`${delivery.sourceEventId}:${delivery.leadTargetKey}`}
          >
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <code className={cn(missionRowMetaClass(), "shrink-0")}>
                {compactIdentifier(delivery.sourceEventId)}
              </code>
              <CodingSessionMissionDeliveryBadge delivery={delivery} />
            </div>
            <p className={cn(missionRowBodyClass(), "mt-1")}>
              {delivery.detail}
            </p>
            <p className={cn(missionRowMetaClass(), "mt-1")}>
              {delivery.owningCommandId === null
                ? "Owning command unknown"
                : `Owning command ${compactIdentifier(delivery.owningCommandId)}`}
              {delivery.duplicateRefusedCommandIds.length > 0
                ? ` · ${delivery.duplicateRefusedCommandIds.length} duplicate refused`
                : null}
            </p>
            {delivery.failures.length > 0 ? (
              <>
                <ul className={cn(missionRowMetaClass(), "mt-1 space-y-0.5")}>
                  {delivery.failures
                    .slice(0, DELIVERY_FAILURE_ROW_LIMIT)
                    .map((failure) => (
                      <li key={`${failure.commandId}:${failure.code}`}>
                        <code>{compactIdentifier(failure.commandId)}</code>{" "}
                        {failure.outcome} · {failure.code} · {failure.message}
                      </li>
                    ))}
                </ul>
                {delivery.failures.length > DELIVERY_FAILURE_ROW_LIMIT ? (
                  <p
                    className="mt-1 text-2xs text-amber-700 dark:text-amber-300"
                    role="status"
                  >
                    {delivery.failures.length - DELIVERY_FAILURE_ROW_LIMIT}{" "}
                    older failure receipts not shown.
                  </p>
                ) : null}
              </>
            ) : null}
          </li>
        ))}
      </ul>
      {hidden > 0 ? (
        <p
          className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
          data-testid="mission-delivery-truncation"
          role="status"
        >
          Showing {shown.length} of {ordered.length} observed deliveries;{" "}
          {hidden} older {hidden === 1 ? "row is" : "rows are"} not shown.
        </p>
      ) : null}
    </div>
  );
}

/**
 * Lane D declares `failures` bounded to 8, but a renderer must not rely on a
 * producer's promise to stay bounded — the UI does its own capping, visibly.
 */
const DELIVERY_FAILURE_ROW_LIMIT = 8;

/** The Inspector's existing compact form for a signed identifier. */
function compactIdentifier(value: string): string {
  return value.length > 20 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}
