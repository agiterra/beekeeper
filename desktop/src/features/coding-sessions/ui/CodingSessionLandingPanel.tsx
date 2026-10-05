import type * as React from "react";
import {
  CircleCheck,
  CircleDot,
  CircleHelp,
  CircleX,
  LoaderCircle,
  RefreshCw,
} from "lucide-react";

import type {
  CodingSessionLandingModel,
  CodingSessionLandingTone,
} from "@/features/coding-sessions/lib/codingSessionLandingModel";
import { cn } from "@/shared/lib/cn";

import { CodingSessionSurfaceSubheader } from "./CodingSessionChangesRailSubheader";
import { CodingSessionLandingGateBody } from "./CodingSessionLandingGateRunning";
import { useCodingSessionLandingRead } from "./CodingSessionLandingPanelRead";
import { CodingSessionMissionLandControl } from "./CodingSessionMissionLandControl";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";

/**
 * The Landing surface (SV-24, DB4): where T3 Code shows a pull request's
 * checks, review and merge, Beekeeper shows four rows — Gate, Verdict, Land,
 * Landed — each a signed fact or a relay read that says which it is.
 *
 * Laid out like T3's pull-request detail (`PullRequestDetailPanel.tsx`,
 * `PullRequestChecksPopover.tsx`): a status glyph and word per row, failures
 * and running checks first, completed after. The Land control is the one
 * Mission already shows; it never runs git.
 */
export function CodingSessionLandingPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const read = useCodingSessionLandingRead(ctx);
  return (
    <CodingSessionLandingPanelView
      model={read.model}
      onRefreshMain={read.refreshMain}
      refreshingMain={read.refreshingMain}
    />
  );
}

/** The panel over a model: no reads, so a test can mount any state. */
export function CodingSessionLandingPanelView({
  model,
  onRefreshMain,
  refreshingMain = false,
}: {
  model: CodingSessionLandingModel;
  onRefreshMain: (() => void) | null;
  refreshingMain?: boolean;
}) {
  const { gate, head, land, landed, verdict } = model;
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-testid="coding-session-landing"
    >
      <CodingSessionSurfaceSubheader
        meta={
          head ? (
            <span
              title={`The newest commit ${
                head.source === "gate"
                  ? "a gate row names"
                  : head.source === "verdict"
                    ? "the newest verdict's report names"
                    : "the land rule answered about"
              }: ${head.sha}`}
            >
              <span
                className="font-mono"
                data-testid="coding-session-landing-head"
              >
                {head.short}
              </span>{" "}
              · signed facts and one relay read
            </span>
          ) : (
            "signed facts and one relay read"
          )
        }
        title="Landing"
      />
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain p-3">
        <LandingRow
          label="Gate"
          sentence={gate.sentence}
          testId="coding-session-landing-gate"
          tone={gate.tone}
          word={gate.word}
        >
          <CodingSessionLandingGateBody gate={gate} />
          {gate.notes.map((note) => (
            <p className="text-2xs text-muted-foreground" key={note}>
              {note}
            </p>
          ))}
        </LandingRow>
        <LandingRow
          label="Verdict"
          sentence={verdict.sentence}
          testId="coding-session-landing-verdict"
          tone={verdict.tone}
          word={verdict.word}
        >
          {verdict.eventId ? (
            <p
              className="font-mono text-2xs text-muted-foreground"
              title={verdict.eventId}
            >
              verdict {verdict.eventId.slice(0, 8)}
            </p>
          ) : null}
        </LandingRow>
        <LandingRow
          label="Land"
          sentence={land.sentence}
          testId="coding-session-landing-land"
          tone={land.tone}
          word={land.word}
        >
          {land.land ? (
            <CodingSessionMissionLandControl land={land.land} />
          ) : null}
        </LandingRow>
        <LandingRow
          actions={
            onRefreshMain ? (
              <button
                aria-label="Check main again"
                className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                data-testid="coding-session-landing-landed-refresh"
                onClick={onRefreshMain}
                title="Check main again"
                type="button"
              >
                <RefreshCw
                  aria-hidden
                  className={cn("size-3.5", refreshingMain && "animate-spin")}
                />
              </button>
            ) : null
          }
          label="Landed"
          meta={landed.checkedAt}
          sentence={landed.sentence}
          testId="coding-session-landing-landed"
          tone={landed.tone}
          word={landed.word}
        />
      </div>
    </div>
  );
}

const TONE_TEXT: Readonly<Record<CodingSessionLandingTone, string>> = {
  ok: "text-emerald-600 dark:text-emerald-400",
  running: "text-primary",
  attention: "text-destructive",
  neutral: "text-foreground",
  unknown: "text-muted-foreground",
};

function ToneIcon({ tone }: { tone: CodingSessionLandingTone }) {
  const className = cn("size-4 shrink-0", TONE_TEXT[tone]);
  switch (tone) {
    case "ok":
      return <CircleCheck aria-hidden className={className} />;
    case "attention":
      return <CircleX aria-hidden className={className} />;
    case "running":
      return (
        <LoaderCircle aria-hidden className={cn(className, "animate-spin")} />
      );
    case "unknown":
      return <CircleHelp aria-hidden className={className} />;
    default:
      return <CircleDot aria-hidden className={className} />;
  }
}

/**
 * One of the four rows: a glyph, the row's name, its word, the sentence that
 * says what the word stands on, then whatever detail belongs under it.
 * The word carries the state; colour is the third carrier, never the only one.
 */
function LandingRow({
  actions,
  children,
  label,
  meta,
  sentence,
  testId,
  tone,
  word,
}: {
  actions?: React.ReactNode;
  children?: React.ReactNode;
  label: string;
  meta?: string | null;
  sentence: string | null;
  testId: string;
  tone: CodingSessionLandingTone;
  word: string;
}) {
  return (
    <section
      aria-label={label}
      className="rounded-lg border border-border/60 bg-muted/10 p-2.5"
      data-testid={testId}
      data-tone={tone}
    >
      <div className="flex items-center gap-2">
        <ToneIcon tone={tone} />
        <h3 className="text-xs font-semibold">{label}</h3>
        <span
          className={cn("text-xs font-medium", TONE_TEXT[tone])}
          data-testid={`${testId}-word`}
        >
          {word}
        </span>
        <span className="flex-1" />
        {meta ? (
          <span
            className="shrink-0 text-2xs text-muted-foreground"
            data-testid={`${testId}-meta`}
          >
            {meta}
          </span>
        ) : null}
        {actions}
      </div>
      {sentence ? (
        <p
          className="mt-1 pl-6 text-xs leading-5 text-muted-foreground"
          data-testid={`${testId}-sentence`}
        >
          {sentence}
        </p>
      ) : null}
      {children ? <div className="mt-2 space-y-2 pl-6">{children}</div> : null}
    </section>
  );
}
