import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  CODING_SESSION_TITLE_MODES,
  getCodingSessionNamingSettings,
  setCodingSessionNamingSettings,
  testCodingSessionNaming,
  type CodingSessionNamingProvider,
  type CodingSessionNamingSettings,
} from "@/shared/api/tauriCodingSessionNaming";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";
import { codingSessionNamingTestSummary } from "../lib/codingSessionNamingTest";
import {
  CODING_SESSION_TITLE_MODE_HINTS,
  CODING_SESSION_TITLE_MODE_LABELS,
  CODING_SESSION_TYPED_NAME_WINS,
  type CodingSessionTitleModeApply,
  chooseCodingSessionTitleMode,
  codingSessionHostMismatchLine,
  codingSessionNamingDraftAfterError,
  codingSessionTitleModeApplyOnChoose,
  codingSessionTitleModeDisclosure,
  codingSessionTitleModeReset,
  codingSessionTitleModeSaveInput,
  codingSessionTitleModeUnsaved,
} from "../lib/codingSessionTitleModeCopy";
import { CodingSessionNamingCardModelFields } from "./CodingSessionNamingCardModelFields";

export const codingSessionNamingQueryKey = ["coding-session-naming"] as const;

function errorText(error: unknown): string | null {
  if (error === null || error === undefined) return null;
  return error instanceof Error ? error.message : String(error);
}

/**
 * Who titles a new coding session on this computer (D9, SV-56).
 *
 * Three modes, and each says where the first message goes for a title and
 * on which machine: the session's own agent (the default — the runtime that
 * already received the message titles it, on the computer that runs it),
 * the person's own naming model (today's per-device endpoint, which suggests
 * a name while they write), or nothing at all. Choosing a mode applies it
 * at once, as T3's settings do; only the endpoint fields take Save, and
 * "Use my naming model" with no endpoint stored yet says it is not saved.
 * When this computer's agent host was not told the stored mode, the card
 * says so until a later write lands, and offers to tell it again.
 */
export function CodingSessionNamingCard() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: codingSessionNamingQueryKey,
    queryFn: getCodingSessionNamingSettings,
    // A desktop with no host (browser preview) has no setting to read;
    // retrying would only delay the panel saying so.
    retry: false,
  });
  const stored = settingsQuery.data ?? null;

  const [draft, setDraft] = React.useState<CodingSessionNamingSettings | null>(
    null,
  );
  const [apiKey, setApiKey] = React.useState("");
  // Until the person edits, the fields mirror what is stored; after, they are
  // theirs. Without the guard a refetch would overwrite half-typed input.
  React.useEffect(() => {
    if (draft !== null || stored === null) return;
    setDraft(stored);
  }, [draft, stored]);
  const current = draft ?? stored;

  /**
   * After any failed write, re-read what is stored and put the stored key
   * state (and, for a mode change, the stored mode) back on screen: a
   * failure can come after the keychain already changed.
   */
  const resyncAfterError = async (revertMode: boolean) => {
    const fresh = await queryClient
      .fetchQuery({
        queryKey: codingSessionNamingQueryKey,
        queryFn: getCodingSessionNamingSettings,
        staleTime: 0,
      })
      .catch(() => null);
    if (fresh === null) return;
    setDraft((previous) =>
      codingSessionNamingDraftAfterError(previous, fresh, revertMode),
    );
  };

  const save = useMutation({
    mutationFn: (next: CodingSessionNamingSettings) =>
      setCodingSessionNamingSettings(
        codingSessionTitleModeSaveInput({ draft: next, stored, apiKey }),
      ),
    onSuccess: (next) => {
      queryClient.setQueryData(codingSessionNamingQueryKey, next);
      setDraft(next);
      setApiKey("");
    },
    onError: () => resyncAfterError(false),
  });

  /** A mode chosen, reset, or re-sent to the agent host: applied at once. */
  const applyMode = useMutation({
    mutationFn: (input: CodingSessionTitleModeApply) =>
      setCodingSessionNamingSettings(input),
    onSuccess: (next) => {
      queryClient.setQueryData(codingSessionNamingQueryKey, next);
      // The endpoint fields stay as typed; only what this write decided moves.
      setDraft((previous) =>
        previous === null
          ? next
          : {
              ...previous,
              titleMode: next.titleMode,
              hasApiKey: next.hasApiKey,
              hostModeMismatch: next.hostModeMismatch ?? null,
            },
      );
    },
    onError: () => resyncAfterError(true),
  });

  /**
   * Try what is in the fields, without saving it — the moment a test is
   * worth pressing is the moment the URL in the box is not the URL on disk.
   */
  const tryIt = useMutation({
    mutationFn: (next: CodingSessionNamingSettings) =>
      testCodingSessionNaming({
        provider: next.provider,
        baseUrl: next.baseUrl,
        model: next.model,
        // Omitted means "use the stored key".
        ...(apiKey.length > 0 ? { apiKey } : {}),
      }),
  });

  /** Delete the stored key without touching anything else. */
  const forgetKey = useMutation({
    mutationFn: (provider: CodingSessionNamingProvider) =>
      setCodingSessionNamingSettings({ provider, apiKey: "" }),
    onSuccess: (next) => {
      queryClient.setQueryData(codingSessionNamingQueryKey, next);
      setDraft((previous) =>
        previous === null ? next : { ...previous, hasApiKey: next.hasApiKey },
      );
      setApiKey("");
    },
    // The key may already be gone from the keychain: show what is stored.
    onError: () => resyncAfterError(false),
  });
  const saveError = errorText(save.error ?? forgetKey.error ?? applyMode.error);
  const writing = save.isPending || forgetKey.isPending || applyMode.isPending;
  /** One write at a time, and only its own error on screen. */
  const resetWrites = () => {
    save.reset();
    forgetKey.reset();
    applyMode.reset();
  };

  const testSummary = codingSessionNamingTestSummary({
    error: errorText(tryIt.error),
    isPending: tryIt.isPending,
    result: tryIt.data ?? null,
  });

  const patch = React.useCallback(
    (change: Partial<CodingSessionNamingSettings>) =>
      setDraft((previous) =>
        previous === null ? previous : { ...previous, ...change },
      ),
    [],
  );

  if (current === null) {
    return (
      <div
        className="px-4 py-4 text-sm text-muted-foreground"
        data-testid="coding-session-title-mode-unavailable"
      >
        {settingsQuery.isPending
          ? "Reading this computer's session-title setting…"
          : `Could not read this computer's session-title setting${
              settingsQuery.error
                ? `: ${errorText(settingsQuery.error)}`
                : " — no host is available."
            }`}
      </div>
    );
  }

  const mode = current.titleMode;
  const disclosure = codingSessionTitleModeDisclosure({
    mode,
    provider: current.provider,
    baseUrl: current.baseUrl,
  });
  const unsaved =
    stored === null
      ? null
      : codingSessionTitleModeUnsaved(stored.titleMode, mode);
  const reset = codingSessionTitleModeReset(stored);
  const hostMismatch = codingSessionHostMismatchLine(stored);

  return (
    <div
      className="flex flex-col gap-4 px-4 py-4"
      data-testid="coding-session-naming-card"
      data-title-mode={stored?.titleMode ?? mode}
    >
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1.5 text-xs font-medium text-muted-foreground">
          Session titles
        </legend>
        {CODING_SESSION_TITLE_MODES.map((option) => (
          <label
            className={cn(
              "flex cursor-pointer items-start gap-2.5 rounded-md border px-3 py-2",
              option === mode
                ? "border-primary/60 bg-primary/5"
                : "border-border/60",
            )}
            data-testid={`coding-session-title-mode-${option}`}
            key={option}
          >
            <input
              checked={option === mode}
              className="mt-0.5"
              name="coding-session-title-mode"
              disabled={writing}
              onChange={() => {
                const apply = codingSessionTitleModeApplyOnChoose({
                  stored,
                  draft: current,
                  mode: option,
                });
                setDraft(chooseCodingSessionTitleMode(current, option));
                if (apply !== null) {
                  resetWrites();
                  applyMode.mutate(apply);
                }
              }}
              type="radio"
              value={option}
            />
            <span className="flex flex-col gap-0.5">
              <span className="text-sm text-foreground">
                {CODING_SESSION_TITLE_MODE_LABELS[option]}
              </span>
              <span className="text-2xs text-muted-foreground">
                {CODING_SESSION_TITLE_MODE_HINTS[option]}
              </span>
            </span>
          </label>
        ))}
      </fieldset>

      <div
        className="flex flex-col gap-1.5"
        data-testid="coding-session-title-mode-disclosure"
      >
        {disclosure.map((line) => (
          <p className="text-xs text-muted-foreground" key={line}>
            {line}
          </p>
        ))}
        <p className="text-xs text-muted-foreground">
          {CODING_SESSION_TYPED_NAME_WINS}
        </p>
      </div>

      {mode === "my-model" ? (
        <CodingSessionNamingCardModelFields
          apiKey={apiKey}
          current={current}
          onApiKeyChange={setApiKey}
          patch={patch}
        />
      ) : null}

      {hostMismatch && stored ? (
        <div
          className="flex flex-wrap items-center gap-3 rounded-md border border-destructive/40 px-3 py-2"
          data-testid="coding-session-title-mode-host-mismatch"
          role="alert"
        >
          <p className="flex-1 text-xs text-destructive">{hostMismatch}</p>
          <Button
            data-testid="coding-session-title-mode-host-retry"
            disabled={writing}
            onClick={() => {
              resetWrites();
              applyMode.mutate({
                titleMode: stored.titleMode,
                provider: stored.provider,
              });
            }}
            size="sm"
            type="button"
            variant="outline"
          >
            {applyMode.isPending ? "Telling…" : "Tell it again"}
          </Button>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center gap-3">
        {mode === "my-model" ? (
          <Button
            data-testid="coding-session-naming-save"
            disabled={writing}
            onClick={() => {
              resetWrites();
              save.mutate(current);
            }}
            size="sm"
            type="button"
          >
            {save.isPending ? "Saving…" : "Save"}
          </Button>
        ) : null}
        {reset ? (
          <Button
            data-testid="coding-session-title-mode-reset"
            disabled={writing}
            onClick={() => {
              resetWrites();
              setDraft(chooseCodingSessionTitleMode(current, reset.titleMode));
              applyMode.mutate(reset);
            }}
            size="sm"
            type="button"
            variant="outline"
          >
            Reset to default
          </Button>
        ) : null}
        {mode === "my-model" && current.provider !== "off" ? (
          <Button
            data-testid="coding-session-naming-test"
            disabled={tryIt.isPending || current.model.trim().length === 0}
            onClick={() => tryIt.mutate(current)}
            size="sm"
            type="button"
            variant="outline"
          >
            {tryIt.isPending ? "Testing…" : "Test"}
          </Button>
        ) : null}
        {mode === "my-model" &&
        current.hasApiKey &&
        current.provider !== "off" ? (
          <Button
            data-testid="coding-session-naming-forget-key"
            disabled={writing}
            onClick={() => {
              resetWrites();
              forgetKey.mutate(current.provider);
            }}
            size="sm"
            type="button"
            variant="outline"
          >
            {forgetKey.isPending ? "Forgetting…" : "Forget key"}
          </Button>
        ) : null}
        {unsaved ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid="coding-session-title-mode-unsaved"
          >
            {unsaved}
          </p>
        ) : null}
        {saveError ? (
          <p className="text-xs text-destructive" role="alert">
            {saveError}
          </p>
        ) : null}
      </div>

      {mode === "my-model" && testSummary.headline ? (
        <div
          className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
          data-testid="coding-session-naming-test-result"
          role="status"
        >
          <p
            className={cn(
              "text-sm",
              testSummary.tone === "failed"
                ? "text-destructive"
                : "text-foreground",
            )}
          >
            {testSummary.headline}
          </p>
          {testSummary.tone === "ok" && testSummary.detail ? (
            <p className="text-2xs text-muted-foreground">
              {testSummary.detail}
            </p>
          ) : null}
          {/* What was sent, verbatim: the sample, never the person's draft. */}
          {tryIt.data ? (
            <p className="text-2xs text-muted-foreground">
              Sent a fixed sample, not anything you wrote: “{tryIt.data.sent}”
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
