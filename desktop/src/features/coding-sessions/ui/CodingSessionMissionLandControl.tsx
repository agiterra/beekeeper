import * as React from "react";

import type { CodingSessionMissionLandModel } from "@/features/coding-sessions/lib/codingSessionMissionLand";
import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";

/**
 * Landing the mission's commit — the command, never the push.
 *
 * Finding 27: a verifier's FAIL was on the wire and the branch landed on `main`
 * anyway. This control offers the push **only** when the relay's own
 * `require-verdict` rule would admit it, reading that rule through the same
 * Rust function the pre-receive hook and `bee git check --ref` call. When it
 * would not, the control stays and prints §1j's refusal string verbatim: a
 * missing control would leave the founder guessing which of "not approved",
 * "not read" and "not governed" they were looking at, and a paraphrase would
 * drift from the words the relay will actually print.
 *
 * **It never runs git**, and the confirm step says why in the copy a person
 * reads rather than in a comment they never will. Three reasons, each
 * disqualifying alone: the push is irreversible; the app holds no checkout and
 * cannot know which worktree is meant; and `git-credential-nostr` lives in the
 * founder's shell, not in this process, so a push from here would fail on
 * NIP-98 or succeed under a different identity.
 */
export function CodingSessionMissionLandControl({
  land,
  onCopy,
}: {
  land: CodingSessionMissionLandModel;
  /**
   * How this surface copies text. Injected so the control never reaches for a
   * browser global a test cannot see; defaults to the clipboard.
   */
  onCopy?: (text: string) => void;
}) {
  const [confirming, setConfirming] = React.useState(false);
  const [copied, setCopied] = React.useState(false);

  if (land.state !== "ready") {
    return (
      <div
        className={cn(missionRowClass("standard"), "mt-2")}
        data-land-state={land.state}
        data-testid="mission-land-control"
      >
        <p
          className={missionRowBodyClass()}
          data-testid="mission-land-sentence"
        >
          {land.sentence}
        </p>
        <FoundersLine land={land} />
      </div>
    );
  }

  return (
    <div
      className={cn(missionRowClass("standard"), "mt-2")}
      data-land-state="ready"
      data-testid="mission-land-control"
    >
      {confirming ? (
        <div data-testid="mission-land-confirm">
          <p className={cn(missionRowBodyClass(), "font-medium")}>
            Land this commit on main
          </p>
          <p
            className={cn(missionRowBodyClass(), "mt-1")}
            data-testid="mission-land-approval"
          >
            {land.approvalSentence}
          </p>
          <p
            className={cn(missionRowBodyClass(), "mt-1")}
            data-testid="mission-land-not-run"
          >
            {land.notRunSentence}
          </p>
          {/* Wrapped, not scrolled: the sha is the whole point of the line
              and a horizontally scrolling <pre> hides two thirds of it behind
              a gesture. A command a person has to copy is shown whole. */}
          <pre
            className="mt-1 whitespace-pre-wrap break-all rounded-md border border-border/60 bg-muted/30 px-2 py-1.5 text-xs text-foreground"
            data-testid="mission-land-command"
          >
            <code>{land.command}</code>
          </pre>
          <div className="mt-1.5 flex items-center gap-2">
            <button
              className="rounded-md border border-border/60 bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary hover:bg-primary/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              data-testid="mission-land-copy"
              onClick={() => {
                if (land.command === null) return;
                if (onCopy) onCopy(land.command);
                else void navigator.clipboard?.writeText(land.command);
                setCopied(true);
              }}
              type="button"
            >
              Copy command
            </button>
            <button
              className="rounded-md px-1.5 py-1 text-2xs font-medium text-muted-foreground underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              data-testid="mission-land-cancel"
              onClick={() => setConfirming(false)}
              type="button"
            >
              Close
            </button>
            {copied ? (
              <span
                className={missionRowMetaClass()}
                data-testid="mission-land-copied"
                role="status"
              >
                Copied
              </span>
            ) : null}
          </div>
        </div>
      ) : (
        <>
          <button
            className="rounded-md border border-border/60 bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary hover:bg-primary/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="mission-land-open"
            onClick={() => setConfirming(true)}
            type="button"
          >
            {land.buttonLabel}
          </button>
          <FoundersLine land={land} />
        </>
      )}
    </div>
  );
}

/**
 * Who founds this repository, and whether the viewer is one of them.
 *
 * Rendered in every state — finding 33. Until this line existed the screen
 * could say "not ready to land" without ever saying that the rule answers to a
 * key the reader does not hold, which is the difference between a mission that
 * failed and a repository that never counted you.
 */
function FoundersLine({ land }: { land: CodingSessionMissionLandModel }) {
  if (land.foundersSentence.length === 0) return null;
  return (
    <p
      className={cn(missionRowMetaClass(), "mt-1")}
      data-founder={land.viewerIsFounder ? "viewer" : "other"}
      data-testid="mission-land-founders"
    >
      {land.foundersSentence}
    </p>
  );
}
