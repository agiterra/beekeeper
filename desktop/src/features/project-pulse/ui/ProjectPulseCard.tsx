import { Activity } from "lucide-react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import {
  EmptyHint,
  SectionCard,
} from "@/features/projects-container/ui/SectionCard";
import { Button } from "@/shared/ui/button";

import {
  formatPulseAge,
  formatPulseEntryType,
  groupPulseEntries,
  groupPulseSessions,
} from "../lib/pulseFormat";
import { useProjectPulseDigest } from "../lib/pulseQueries";
import { useProjectPulseChannelIds } from "./ProjectPulseScreen";

/**
 * The project home's Pulse summary: how many people are actively working and
 * what the newest explicit claims say.
 *
 * The card never guesses. A read that has not finished says so, a partial read
 * says so, and a completed read with nothing in it says the project is quiet —
 * three different sentences, because they are three different facts.
 */
export function ProjectPulseCard({
  project,
  onOpenPulse,
}: {
  project: ProjectContainer;
  onOpenPulse: () => void;
}) {
  const { channelIds, unresolved } = useProjectPulseChannelIds(project);
  const pulse = useProjectPulseDigest(project.address, channelIds, unresolved);
  const digest = pulse.digest;
  // Ages are measured against the wall clock, never against the digest's
  // `asOf`: a cached digest that is ten minutes old must show its entries as
  // ten minutes older, not frozen at the second it was read.
  const nowSeconds = Math.floor(Date.now() / 1_000);

  const sessions = digest ? groupPulseSessions(digest) : null;
  const entries = digest ? groupPulseEntries(digest) : null;
  const count =
    (sessions?.activeWork.length ?? 0) + (entries?.active.length ?? 0);

  return (
    <SectionCard
      action={
        <Button
          className="h-7 px-2 text-2xs"
          data-testid="project-screen-open-pulse"
          onClick={onOpenPulse}
          variant="ghost"
        >
          Open Pulse
        </Button>
      }
      count={count}
      icon={<Activity className="size-4" />}
      testId="project-pulse-card"
      title="Pulse"
    >
      {pulse.kind === "loading" && !digest ? (
        <EmptyHint>Reading this project's Pulse…</EmptyHint>
      ) : null}

      {pulse.kind === "partial" ? (
        <EmptyHint>
          Partial read — some sources did not answer, so this is not the whole
          project.
        </EmptyHint>
      ) : null}

      {digest && count === 0 && pulse.kind === "ready" ? (
        <EmptyHint>
          No active work and no open claims. This read completed.
        </EmptyHint>
      ) : null}

      {sessions && sessions.activeWork.length > 0 ? (
        <p
          className="text-sm text-foreground"
          data-testid="project-pulse-card-active"
        >
          {sessions.activeWork.length} session
          {sessions.activeWork.length === 1 ? "" : "s"} observed working
          {sessions.lastSeen.length > 0
            ? `, ${sessions.lastSeen.length} last seen earlier`
            : ""}
          .
        </p>
      ) : null}

      {entries && entries.active.length > 0 ? (
        <ul className="mt-2 flex flex-col gap-1">
          {entries.active.slice(0, 3).map((entry) => (
            <li
              className="truncate text-sm text-muted-foreground"
              data-testid="project-pulse-card-entry"
              key={entry.eventId}
            >
              <span className="text-foreground">
                {formatPulseEntryType(entry.type)}
              </span>{" "}
              · {formatPulseAge(nowSeconds - entry.createdAt)} ago ·{" "}
              {entry.text}
            </li>
          ))}
        </ul>
      ) : null}
    </SectionCard>
  );
}
