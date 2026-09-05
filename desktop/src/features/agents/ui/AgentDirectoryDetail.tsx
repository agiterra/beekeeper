import { X } from "lucide-react";

import type { AgentDirectoryRow } from "@/features/agents/lib/agentDirectoryModel";
import { Button } from "@/shared/ui/button";
import {
  AGENT_DETAIL_CLOSE_TESTID,
  AGENT_DETAIL_CONTROLS_TESTID,
  AGENT_DETAIL_EDIT_TESTID,
  AGENT_DETAIL_INSTALLED_NO,
  AGENT_DETAIL_INSTALLED_YES,
  AGENT_DETAIL_NOT_MANAGED,
  AGENT_DETAIL_PUBKEY_TESTID,
  AGENT_DETAIL_RESTART_TESTID,
  AGENT_DETAIL_SEATS_EMPTY,
  AGENT_DETAIL_SEATS_TESTID,
  AGENT_DETAIL_SEAT_NO_ROLE,
  AGENT_DETAIL_SEAT_ROW_TESTID,
  AGENT_DETAIL_SEAT_UNPLACED,
  AGENT_DETAIL_START_TESTID,
  AGENT_DETAIL_STOP_TESTID,
  AGENT_DETAIL_TESTID,
  OFFERS_ELIGIBILITY_FOOTNOTE,
  launchesAsText,
  seatDetailPackShaText,
  seatDetailStatusText,
} from "./agentDirectoryCopy";
import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_SHARED_HOME_LABEL,
} from "./AgentHomeRoleBadges";

function agentDetailPackText(row: AgentDirectoryRow): string | null {
  if (row.packRefusedSharedHome === true) return AGENT_SHARED_HOME_LABEL;
  if (row.hasRolePack === false) return AGENT_NO_ROLE_PACK_LABEL;
  return null;
}

export function AgentDirectoryDetail({
  row,
  onClose,
  onSelectSeat,
  onStart,
  onStop,
  onRestart,
  onEdit,
  canEdit,
  isStartPending,
  isRestartPending,
  isPending,
}: {
  row: AgentDirectoryRow;
  onClose: () => void;
  onSelectSeat: (channelId: string, generationId: string) => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onEdit: () => void;
  canEdit: boolean;
  isStartPending: boolean;
  isRestartPending: boolean;
  isPending: boolean;
}) {
  const packText = agentDetailPackText(row);

  return (
    <div
      className="flex h-full flex-col gap-6 overflow-y-auto rounded-2xl border bg-card p-5"
      data-testid={AGENT_DETAIL_TESTID}
    >
      <div className="flex items-start justify-between gap-3">
        <h2 className="text-lg font-semibold">{row.name}</h2>
        <Button
          aria-label="Close"
          data-testid={AGENT_DETAIL_CLOSE_TESTID}
          onClick={onClose}
          size="icon"
          variant="ghost"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      <section className="flex flex-col gap-2 text-sm">
        <div className="flex justify-between gap-3">
          <span className="text-muted-foreground">Model</span>
          <span>{row.modelLabel}</span>
        </div>
        <div className="flex justify-between gap-3">
          <span className="text-muted-foreground">Launches as</span>
          <span>{launchesAsText(row.homeRole)}</span>
        </div>
        {packText ? (
          <div className="flex justify-between gap-3">
            <span className="text-muted-foreground">Pack / nest</span>
            <span>{packText}</span>
          </div>
        ) : null}
        <div className="flex justify-between gap-3">
          <span className="text-muted-foreground">Installed here</span>
          <span>
            {row.isInstalled
              ? AGENT_DETAIL_INSTALLED_YES
              : AGENT_DETAIL_INSTALLED_NO}
          </span>
        </div>
        <div className="flex justify-between gap-3">
          <span className="text-muted-foreground">Pubkey</span>
          <code
            className="truncate text-2xs"
            data-testid={AGENT_DETAIL_PUBKEY_TESTID}
          >
            {row.pubkey}
          </code>
        </div>
      </section>

      <section
        className="flex flex-col gap-2"
        data-testid={AGENT_DETAIL_SEATS_TESTID}
      >
        <h3 className="text-sm font-medium">Seat history</h3>
        {row.seats.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {AGENT_DETAIL_SEATS_EMPTY}
          </p>
        ) : (
          <div className="flex flex-col gap-1">
            {row.seats.map((seat) => (
              <button
                className="flex flex-col gap-0.5 rounded-md border border-border px-3 py-2 text-left text-sm hover:bg-muted/60"
                data-seat-key={seat.key}
                data-testid={AGENT_DETAIL_SEAT_ROW_TESTID}
                key={seat.key}
                onClick={() => onSelectSeat(seat.channelId, seat.generationId)}
                type="button"
              >
                <span className="font-medium">
                  {seat.projectName ?? AGENT_DETAIL_SEAT_UNPLACED}
                </span>
                <span className="text-xs text-muted-foreground">
                  {seat.role ?? AGENT_DETAIL_SEAT_NO_ROLE} ·{" "}
                  {seatDetailStatusText(seat)} ·{" "}
                  <span title={seat.packSha ?? undefined}>
                    {seatDetailPackShaText(seat)}
                  </span>
                </span>
              </button>
            ))}
          </div>
        )}
        <p className="text-xs text-muted-foreground">
          {OFFERS_ELIGIBILITY_FOOTNOTE}
        </p>
      </section>

      <section
        className="flex flex-col gap-2"
        data-testid={AGENT_DETAIL_CONTROLS_TESTID}
      >
        <h3 className="text-sm font-medium">Controls</h3>
        {row.isInstalled ? (
          <div className="flex flex-wrap gap-2">
            <Button
              data-testid={AGENT_DETAIL_START_TESTID}
              disabled={isPending || row.isRunning}
              onClick={onStart}
              size="sm"
              variant="outline"
            >
              {isStartPending ? "Starting…" : "Start"}
            </Button>
            <Button
              data-testid={AGENT_DETAIL_STOP_TESTID}
              disabled={isPending || !row.isRunning}
              onClick={onStop}
              size="sm"
              variant="outline"
            >
              Stop
            </Button>
            <Button
              data-testid={AGENT_DETAIL_RESTART_TESTID}
              disabled={isPending}
              onClick={onRestart}
              size="sm"
              variant="outline"
            >
              {isRestartPending ? "Restarting…" : "Restart"}
            </Button>
            <Button
              data-testid={AGENT_DETAIL_EDIT_TESTID}
              disabled={!canEdit}
              onClick={onEdit}
              size="sm"
              variant="outline"
            >
              Edit
            </Button>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            {AGENT_DETAIL_NOT_MANAGED}
          </p>
        )}
      </section>
    </div>
  );
}
