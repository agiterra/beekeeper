import * as React from "react";
import { Pencil } from "lucide-react";

import { useUpdateManagedAgentMutation } from "@/features/agents/hooks";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import {
  AGENT_ROW_RENAME_CANCEL,
  AGENT_ROW_RENAME_CANCEL_TESTID,
  AGENT_ROW_RENAME_EMPTY,
  AGENT_ROW_RENAME_ERROR_TESTID,
  AGENT_ROW_RENAME_FORM_TESTID,
  AGENT_ROW_RENAME_HINT,
  AGENT_ROW_RENAME_INPUT_TESTID,
  AGENT_ROW_RENAME_LABEL,
  AGENT_ROW_RENAME_SAVE,
  AGENT_ROW_RENAME_SAVE_TESTID,
  AGENT_ROW_RENAME_TESTID,
  renameAriaLabel,
  renameFailedText,
  renameUnpublishedText,
} from "./agentDirectoryCopy";

function causeMessage(cause: unknown): string {
  if (cause instanceof Error) return cause.message;
  return String(cause);
}

/**
 * The rename toggle for one directory row. Rendered beside the row button,
 * never inside it: a button inside a button is not a control anyone can
 * reach reliably.
 */
export function AgentDirectoryRenameButton({
  name,
  onClick,
}: {
  name: string;
  onClick: () => void;
}) {
  return (
    <Button
      aria-label={renameAriaLabel(name)}
      data-testid={AGENT_ROW_RENAME_TESTID}
      onClick={onClick}
      size="sm"
      type="button"
      variant="ghost"
    >
      <Pencil />
      {AGENT_ROW_RENAME_LABEL}
    </Button>
  );
}

/**
 * Rename an installed agent in place.
 *
 * Sends `update_managed_agent` with exactly `{ pubkey, name }`: the same
 * identity keeps its pubkey and role, and the backend republishes its relay
 * profile. Nothing is minted and neither the persona nor the role is touched.
 * A blank name is refused here; a failure is shown in its own words and the
 * form stays open with what was typed.
 */
export function AgentDirectoryRenameForm({
  pubkey,
  name,
  onDone,
}: {
  pubkey: string;
  name: string;
  onDone: () => void;
}) {
  const mutation = useUpdateManagedAgentMutation();
  const mutateAsync = mutation.mutateAsync;
  const [draft, setDraft] = React.useState(name);
  const [error, setError] = React.useState<string | null>(null);
  const isPending = mutation.isPending;
  const inputRef = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    inputRef.current?.focus();
  }, []);

  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const next = draft.trim();
    if (next.length === 0) {
      setError(AGENT_ROW_RENAME_EMPTY);
      return;
    }
    if (next === name) {
      onDone();
      return;
    }
    setError(null);
    try {
      const result = await mutateAsync({ pubkey, name: next });
      if (result.profileSyncError) {
        setError(renameUnpublishedText(result.profileSyncError));
        return;
      }
      onDone();
    } catch (cause) {
      setError(renameFailedText(causeMessage(cause)));
    }
  }

  return (
    <form
      className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3"
      data-testid={AGENT_ROW_RENAME_FORM_TESTID}
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Input
          aria-label={renameAriaLabel(name)}
          className="h-9 max-w-xs"
          data-testid={AGENT_ROW_RENAME_INPUT_TESTID}
          disabled={isPending}
          onChange={(event) => {
            setDraft(event.target.value);
            if (error) setError(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") onDone();
          }}
          ref={inputRef}
          value={draft}
        />
        <Button
          data-testid={AGENT_ROW_RENAME_SAVE_TESTID}
          disabled={isPending}
          size="sm"
          type="submit"
        >
          {AGENT_ROW_RENAME_SAVE}
        </Button>
        <Button
          data-testid={AGENT_ROW_RENAME_CANCEL_TESTID}
          disabled={isPending}
          onClick={onDone}
          size="sm"
          type="button"
          variant="ghost"
        >
          {AGENT_ROW_RENAME_CANCEL}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{AGENT_ROW_RENAME_HINT}</p>
      {error ? (
        <p
          className="text-sm text-destructive"
          data-testid={AGENT_ROW_RENAME_ERROR_TESTID}
          role="alert"
        >
          {error}
        </p>
      ) : null}
    </form>
  );
}
