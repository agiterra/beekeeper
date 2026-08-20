import { Activity, ArrowRightLeft, OctagonAlert } from "lucide-react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import {
  EmptyHint,
  SectionCard,
} from "@/features/projects-container/ui/SectionCard";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";

import { pulseAuthorLabel } from "../lib/pulseAuthors";
import {
  formatPulseAge,
  formatPulseEntryType,
  groupPulseEntries,
  groupPulseSessions,
  sortPulseEntriesByConsequence,
} from "../lib/pulseFormat";
import { useProjectPulseDigest } from "../lib/pulseQueries";
import { useProjectPulseChannelIds } from "./ProjectPulseScreen";
import { PulseWriteHint } from "./PulseWriteHint";
import { usePulseAuthorNames } from "./usePulseAuthorNames";

/**
 * The project home's Pulse summary: how many people are actively working and
 * what the newest explicit claims say.
 *
 * The card never guesses. A read that has not finished says so, a partial read
 * says so, and a completed read with nothing in it says the project is quiet —
 * three different sentences, because they are three different facts.
 *
 * Every line names its author. Without that, one person's "picking pool.rs back
 * up" stacked on another's "do not touch pool.rs" reads as the project arguing
 * with itself; the card is the surface most people actually look at, so the
 * conflict has to be legible as a conflict *between people* here, not only on
 * the full screen.
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
  const authorNames = usePulseAuthorNames(digest);
  // Ages are measured against the wall clock, never against the digest's
  // `asOf`: a cached digest that is ten minutes old must show its entries as
  // ten minutes older, not frozen at the second it was read.
  const nowSeconds = Math.floor(Date.now() / 1_000);

  const sessions = digest ? groupPulseSessions(digest) : null;
  const entries = digest ? groupPulseEntries(digest) : null;
  const count =
    (sessions?.activeWork.length ?? 0) + (entries?.active.length ?? 0);
  // Blockers first, so a standing "do not touch" is never the third of three
  // lines the card has room for.
  const topEntries = entries
    ? sortPulseEntriesByConsequence(entries.active).slice(0, 3)
    : [];

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
        <>
          <EmptyHint>
            No active work and no open claims. This read completed.
          </EmptyHint>
          <PulseWriteHint />
        </>
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

      {topEntries.length > 0 ? (
        <ul className="mt-2 flex flex-col gap-1">
          {topEntries.map((entry) => {
            const author = pulseAuthorLabel(entry.pubkey, authorNames);
            const blocker = entry.type === "blocker";
            const handoff = entry.type === "handoff";
            return (
              <li
                className={cn(
                  "flex items-start gap-1.5 text-sm text-muted-foreground",
                  blocker &&
                    "rounded border border-destructive/40 bg-destructive/5 px-1.5 py-1 text-foreground",
                )}
                data-entry-type={entry.type}
                data-testid="project-pulse-card-entry"
                key={entry.eventId}
                // A blocker cut mid-sentence with no way to read the rest is a
                // wait-signal the reader cannot act on.
                title={`${author} · ${formatPulseEntryType(entry.type)} · ${entry.text}`}
              >
                {blocker ? (
                  <OctagonAlert
                    className="mt-0.5 size-3 shrink-0 text-destructive"
                    aria-hidden
                  />
                ) : handoff ? (
                  <ArrowRightLeft
                    className="mt-0.5 size-3 shrink-0 text-primary"
                    aria-hidden
                  />
                ) : null}
                <span className="line-clamp-2">
                  <span className="font-medium text-foreground">{author}</span>{" "}
                  ·{" "}
                  <span
                    className={cn(
                      "text-foreground",
                      blocker && "font-medium text-destructive",
                    )}
                  >
                    {formatPulseEntryType(entry.type)}
                  </span>{" "}
                  · {formatPulseAge(nowSeconds - entry.createdAt)} ago ·{" "}
                  {entry.text}
                </span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </SectionCard>
  );
}
