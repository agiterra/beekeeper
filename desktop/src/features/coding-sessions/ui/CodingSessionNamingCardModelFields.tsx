import type {
  CodingSessionNamingProvider,
  CodingSessionNamingSettings,
} from "@/shared/api/tauriCodingSessionNaming";
import { Input } from "@/shared/ui/input";

/**
 * The per-device naming endpoint: which API, which URL, which model, which
 * key. Shown only in "Use my naming model" mode — in the other two modes no
 * request is ever built from these fields, so showing them would suggest a
 * setting that is not in force.
 *
 * The key field is write-only: the host keeps the key and the card only
 * learns whether one is stored.
 */
export function CodingSessionNamingCardModelFields({
  current,
  apiKey,
  onApiKeyChange,
  patch,
}: {
  current: CodingSessionNamingSettings;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  patch: (change: Partial<CodingSessionNamingSettings>) => void;
}) {
  return (
    <div
      className="flex flex-col gap-4 rounded-lg border border-border/60 px-3 py-3"
      data-testid="coding-session-naming-model-fields"
    >
      <div className="flex flex-col gap-1.5">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-naming-provider"
        >
          Naming endpoint
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
          {current.provider === "off" ? (
            <option disabled value="off">
              Choose an endpoint
            </option>
          ) : null}
          <option value="anthropic">Anthropic API</option>
          <option value="openai-compatible">
            OpenAI-compatible API (Ollama, LM Studio, llama.cpp, OpenAI)
          </option>
        </select>
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
              onChange={(event) => onApiKeyChange(event.target.value)}
              placeholder={
                current.hasApiKey
                  ? "•••••••• (stored)"
                  : "Not set on this computer"
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
    </div>
  );
}
