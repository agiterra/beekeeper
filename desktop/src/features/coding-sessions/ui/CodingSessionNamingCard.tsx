import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  getCodingSessionNamingSettings,
  setCodingSessionNamingSettings,
  type CodingSessionNamingProvider,
  type CodingSessionNamingSettings,
} from "@/shared/api/tauriCodingSessionNaming";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

export const codingSessionNamingQueryKey = ["coding-session-naming"] as const;

/**
 * Which model names a coding session from its first message.
 *
 * Off by default, and the copy says exactly what turning it on means: the
 * first message — the words someone typed about their own work — leaves this
 * computer for the endpoint named here. That is a real trade for a
 * four-word title, so it is stated rather than buried, and the local option
 * is offered first-class so the trade can be declined without losing the
 * feature.
 */
export function CodingSessionNamingCard() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: codingSessionNamingQueryKey,
    queryFn: getCodingSessionNamingSettings,
    // A desktop with no host (browser preview) simply has no namer; retrying
    // would only delay the panel rendering the "off" state truthfully.
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

  const save = useMutation({
    mutationFn: (next: CodingSessionNamingSettings) =>
      setCodingSessionNamingSettings({
        provider: next.provider,
        baseUrl: next.baseUrl,
        model: next.model,
        // Omitted rather than empty: an empty string deletes the stored key,
        // and saving a model change must not silently sign you out.
        ...(apiKey.length > 0 ? { apiKey } : {}),
      }),
    onSuccess: (next) => {
      queryClient.setQueryData(codingSessionNamingQueryKey, next);
      setDraft(next);
      setApiKey("");
    },
  });

  /** Delete the stored key without touching anything else. */
  const forgetKey = useMutation({
    mutationFn: (provider: CodingSessionNamingProvider) =>
      setCodingSessionNamingSettings({ provider, apiKey: "" }),
    onSuccess: (next) => {
      queryClient.setQueryData(codingSessionNamingQueryKey, next);
      setDraft(next);
      setApiKey("");
    },
  });
  const failure = save.error ?? forgetKey.error;
  const saveError =
    failure === null || failure === undefined
      ? null
      : failure instanceof Error
        ? failure.message
        : String(failure);

  const patch = React.useCallback(
    (change: Partial<CodingSessionNamingSettings>) =>
      setDraft((previous) =>
        previous === null ? previous : { ...previous, ...change },
      ),
    [],
  );

  if (current === null) {
    return (
      <div className="px-4 py-4 text-sm text-muted-foreground">
        {settingsQuery.isPending
          ? "Reading this computer's naming settings…"
          : "This computer cannot name sessions — no host is available."}
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-4 px-4 py-4"
      data-testid="settings-coding-session-naming"
    >
      <div className="flex flex-col gap-1.5">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-naming-provider"
        >
          Namer
        </label>
        <select
          className="h-9 w-full max-w-sm rounded-md border border-input bg-transparent px-3 text-sm"
          data-testid="coding-session-naming-provider"
          id="coding-session-naming-provider"
          onChange={(event) =>
            patch({
              provider: event.target.value as CodingSessionNamingProvider,
            })
          }
          value={current.provider}
        >
          <option value="off">Off — name sessions yourself</option>
          <option value="anthropic">Anthropic API</option>
          <option value="openai-compatible">
            OpenAI-compatible API (Ollama, LM Studio, llama.cpp, OpenAI)
          </option>
        </select>
        <p className="text-xs text-muted-foreground">
          {namerDisclosure(current.provider, current.baseUrl)}
        </p>
      </div>

      {current.provider === "openai-compatible" ? (
        <div className="flex flex-col gap-1.5">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-naming-base-url"
          >
            API URL
          </label>
          <Input
            autoComplete="off"
            className="max-w-sm font-mono text-xs"
            data-testid="coding-session-naming-base-url"
            id="coding-session-naming-base-url"
            onChange={(event) => patch({ baseUrl: event.target.value })}
            placeholder="http://127.0.0.1:11434/v1"
            spellCheck={false}
            value={current.baseUrl}
          />
          <p className="text-2xs text-muted-foreground">
            The root that `/chat/completions` hangs off. Ollama serves it at
            <span className="font-mono"> http://127.0.0.1:11434/v1</span>.
          </p>
        </div>
      ) : null}

      {current.provider !== "off" ? (
        <>
          <div className="flex flex-col gap-1.5">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor="coding-session-naming-model"
            >
              Model
            </label>
            <Input
              autoComplete="off"
              className="max-w-sm font-mono text-xs"
              data-testid="coding-session-naming-model"
              id="coding-session-naming-model"
              onChange={(event) => patch({ model: event.target.value })}
              placeholder={
                current.provider === "anthropic" ? "claude-opus-5" : "llama3.2"
              }
              spellCheck={false}
              value={current.model}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor="coding-session-naming-key"
            >
              API key{" "}
              <span className="font-normal">
                {current.provider === "openai-compatible"
                  ? "(leave empty for a local model)"
                  : ""}
              </span>
            </label>
            <Input
              autoComplete="off"
              className="max-w-sm font-mono text-xs"
              data-testid="coding-session-naming-key"
              id="coding-session-naming-key"
              onChange={(event) => setApiKey(event.target.value)}
              placeholder={
                current.hasApiKey ? "•••••••• (stored)" : "Not set on this Mac"
              }
              spellCheck={false}
              type="password"
              value={apiKey}
            />
            <p className="text-2xs text-muted-foreground">
              Kept in this computer's keychain and never shown again. Leave it
              blank to keep the stored key.
            </p>
          </div>
        </>
      ) : null}

      <div className="flex items-center gap-3">
        <Button
          data-testid="coding-session-naming-save"
          disabled={save.isPending}
          onClick={() => save.mutate(current)}
          size="sm"
          type="button"
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
        {current.hasApiKey && current.provider !== "off" ? (
          <Button
            data-testid="coding-session-naming-forget-key"
            disabled={forgetKey.isPending || save.isPending}
            onClick={() => forgetKey.mutate(current.provider)}
            size="sm"
            type="button"
            variant="outline"
          >
            {forgetKey.isPending ? "Forgetting…" : "Forget key"}
          </Button>
        ) : null}
        {saveError ? (
          <p className="text-xs text-destructive" role="alert">
            {saveError}
          </p>
        ) : null}
      </div>
    </div>
  );
}

/** What turning this namer on actually does, in one sentence. */
export function namerDisclosure(
  provider: CodingSessionNamingProvider,
  baseUrl: string,
): string {
  if (provider === "off") {
    return "Nothing is sent anywhere. The name field stays yours to fill in.";
  }
  if (provider === "anthropic") {
    return "Your first message is sent to api.anthropic.com every few seconds while you write it, and when you leave the field.";
  }
  const target =
    baseUrl.trim().length > 0 ? baseUrl.trim() : "the API URL below";
  return `Your first message is sent to ${target} every few seconds while you write it, and when you leave the field. A local address keeps it on this computer.`;
}
