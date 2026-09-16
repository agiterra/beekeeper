import * as React from "react";

import { setCodingSessionWorkdir } from "@/shared/api/tauriCodingSessionWorkdirs";
import { listCodingSessionWorktreeBranches } from "@/shared/api/tauriCodingSessionWorktrees";
import { Button } from "@/shared/ui/button";

/**
 * "Use this folder as <project>'s checkout" — the one click that unblocks a
 * founding form whose folder is already correct.
 *
 * Ledger 135(c): with "Use roles" ticked, `CHECKOUT_NOT_RECORDED` blocked the
 * form until the operator went and found Project settings → This computer →
 * Repository folder, while the folder the session was going to run in was
 * typed into the same form a few rows below. The write this button performs is
 * the same store write that settings screen performs — `setCodingSessionWorkdir`
 * with scope `project` — and nothing else.
 *
 * ## It offers only what it checked
 *
 * The button appears only once this component has confirmed the folder is a
 * git checkout, by asking the host for its branches: an empty list means the
 * directory is not one. A folder that is not a checkout gets a sentence saying
 * so rather than a button that would record something a worktree cannot be cut
 * from. While the answer is unknown, neither is shown — an offer is a claim.
 */
export function UseThisFolderButton({
  candidate,
  onRecorded,
  projectLabel,
  projectRef,
}: {
  candidate: string | null;
  onRecorded?: () => void;
  projectLabel: string | null;
  projectRef: string | null;
}) {
  const folder = candidate?.trim() || null;
  const [isCheckout, setIsCheckout] = React.useState<boolean | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!folder || !projectRef) {
      setIsCheckout(null);
      return;
    }
    let cancelled = false;
    setIsCheckout(null);
    void listCodingSessionWorktreeBranches({ workdir: folder })
      .then((answer) => {
        if (cancelled) return;
        // A directory that is not a git checkout answers with no branches and
        // no HEAD; that is the host's own definition, not a guess made here.
        setIsCheckout(answer.branches.length > 0 || answer.headBranch !== null);
      })
      .catch(() => {
        // Could not ask is not a no: stay silent rather than telling a person
        // their folder is wrong on the strength of a failed call.
        if (!cancelled) setIsCheckout(null);
      });
    return () => {
      cancelled = true;
    };
  }, [folder, projectRef]);

  if (!folder || !projectRef) return null;
  if (isCheckout === false) {
    return (
      <span
        className="text-2xs text-muted-foreground"
        data-testid="team-readiness-folder-not-a-checkout"
      >
        {folder} is not a git checkout, so a worktree cannot be cut from it.
      </span>
    );
  }
  if (isCheckout === null) return null;

  const record = () => {
    setBusy(true);
    setError(null);
    void setCodingSessionWorkdir({
      scope: "project",
      key: projectRef,
      path: folder,
    })
      .then(() => onRecorded?.())
      .catch((failure: unknown) => {
        setError(
          failure instanceof Error
            ? failure.message
            : "This folder could not be recorded for the project.",
        );
      })
      .finally(() => setBusy(false));
  };

  return (
    <span className="flex flex-col items-start gap-1">
      <Button
        data-testid="team-readiness-use-this-folder"
        disabled={busy}
        onClick={record}
        size="sm"
        type="button"
        variant="outline"
      >
        Use this folder as {projectLabel?.trim() || "this project"}&rsquo;s
        checkout
      </Button>
      <span className="text-3xs text-muted-foreground">{folder}</span>
      {error ? (
        <span className="text-2xs text-destructive" role="alert">
          {error}
        </span>
      ) : null}
    </span>
  );
}
