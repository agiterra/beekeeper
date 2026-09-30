import { ChevronDown } from "lucide-react";

import {
  codingSessionContextLabel,
  codingSessionEffortLabel,
  codingSessionTraitsSummary,
} from "@/features/coding-sessions/lib/codingSessionModelDisplay";
import { CODING_SESSION_DEFAULT_EFFORT } from "@/features/coding-sessions/lib/codingSessionModelOptions";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

/** The value a control uses for "whatever the adapter does by default". */
const ADAPTER_DEFAULT = "__adapter_default__";
const FAST_ON = "on";
const FAST_OFF = "off";

/**
 * How hard the model thinks, and how much it can hold — the second decision.
 *
 * Both dimensions arrive as brackets on the model id, and both used to be
 * shown *inside the model name* (`gpt-5.4[high]`, `opus[1m]`), which made one
 * list answer three questions and turned seven Codex models into thirty rows.
 * They belong together and apart from the model: same shape as t3code's
 * `High · 1M` control.
 *
 * A dimension the adapter does not offer for this model is not rendered — an
 * empty section would imply a choice exists. "Adapter default" appears only
 * where the adapter also publishes the bare id, because for a model published
 * only at named values it would name an id the provider would refuse.
 *
 * When the catalog row reports the runtime's own efforts (`runtimeEfforts`),
 * the section reads "Reasoning" and lists them in the runtime's order, its own
 * `default` value shown as "Default" where the runtime placed it; a runtime
 * that names no default gets "Runtime default" for the no-token choice. Fast
 * mode is an On/Off choice shown only when the row reports the switch.
 */
export function CodingSessionTraitsPicker({
  className,
  context,
  contexts,
  disabled = false,
  fast = false,
  fastModeOffered = false,
  hasBareModel,
  onContextChange,
  onFastChange,
  onThinkingChange,
  runtimeEfforts = false,
  thinking,
  thinkingLevels,
}: {
  className?: string;
  context: string | null;
  contexts: readonly string[];
  disabled?: boolean;
  /** Fast mode is on in the current selection. */
  fast?: boolean;
  /** The selected model's row reports a fast-mode switch. */
  fastModeOffered?: boolean;
  hasBareModel: boolean;
  onContextChange: (context: string | null) => void;
  onFastChange?: (fast: boolean) => void;
  onThinkingChange: (thinking: string | null) => void;
  /** `thinkingLevels` are the runtime's own reported efforts. */
  runtimeEfforts?: boolean;
  thinking: string | null;
  thinkingLevels: readonly string[];
}) {
  const showFast = fastModeOffered && onFastChange !== undefined;
  if (thinkingLevels.length === 0 && contexts.length === 0 && !showFast) {
    return null;
  }
  const runtimeNamesDefault =
    runtimeEfforts && thinkingLevels.includes(CODING_SESSION_DEFAULT_EFFORT);
  const defaultLabel = runtimeEfforts
    ? runtimeNamesDefault
      ? codingSessionEffortLabel(CODING_SESSION_DEFAULT_EFFORT)
      : "Runtime default"
    : "Adapter default";
  const traits = codingSessionTraitsSummary({ thinking, context, fast });
  const summary =
    runtimeEfforts && thinkingLevels.length > 0 && thinking === null
      ? [defaultLabel, traits].filter(Boolean).join(" · ")
      : thinkingLevels.length === 0 && contexts.length === 0
        ? fast
          ? "Fast"
          : "Fast off"
        : (traits ?? defaultLabel);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          className={cn("h-9 justify-between gap-2", className)}
          data-testid="coding-session-traits-picker"
          disabled={disabled}
          type="button"
          variant="outline"
        >
          <span className="truncate">{summary}</span>
          <ChevronDown aria-hidden className="size-4 shrink-0 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-56">
        {thinkingLevels.length > 0 ? (
          <>
            <DropdownMenuLabel className="text-2xs text-muted-foreground">
              {runtimeEfforts ? "Reasoning" : "Thinking"}
            </DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) =>
                onThinkingChange(value === ADAPTER_DEFAULT ? null : value)
              }
              value={thinking ?? ADAPTER_DEFAULT}
            >
              {hasBareModel && !runtimeNamesDefault ? (
                <DropdownMenuRadioItem
                  data-testid="coding-session-thinking-option"
                  value={ADAPTER_DEFAULT}
                >
                  {defaultLabel}
                </DropdownMenuRadioItem>
              ) : null}
              {thinkingLevels.map((level) => {
                const isDefault =
                  runtimeEfforts && level === CODING_SESSION_DEFAULT_EFFORT;
                return (
                  <DropdownMenuRadioItem
                    data-model-default={isDefault ? "true" : undefined}
                    data-testid="coding-session-thinking-option"
                    key={level}
                    value={isDefault ? ADAPTER_DEFAULT : level}
                  >
                    {codingSessionEffortLabel(level)}
                  </DropdownMenuRadioItem>
                );
              })}
            </DropdownMenuRadioGroup>
          </>
        ) : null}
        {thinkingLevels.length > 0 && contexts.length > 0 ? (
          <DropdownMenuSeparator />
        ) : null}
        {contexts.length > 0 ? (
          <>
            <DropdownMenuLabel className="text-2xs text-muted-foreground">
              Context window
            </DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) =>
                onContextChange(value === ADAPTER_DEFAULT ? null : value)
              }
              value={context ?? ADAPTER_DEFAULT}
            >
              {hasBareModel ? (
                <DropdownMenuRadioItem
                  data-testid="coding-session-context-option"
                  value={ADAPTER_DEFAULT}
                >
                  Adapter default
                </DropdownMenuRadioItem>
              ) : null}
              {contexts.map((window) => (
                <DropdownMenuRadioItem
                  data-testid="coding-session-context-option"
                  key={window}
                  value={window}
                >
                  {codingSessionContextLabel(window)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </>
        ) : null}
        {showFast ? (
          <>
            {thinkingLevels.length > 0 || contexts.length > 0 ? (
              <DropdownMenuSeparator />
            ) : null}
            <DropdownMenuLabel className="text-2xs text-muted-foreground">
              Fast mode
            </DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) => onFastChange?.(value === FAST_ON)}
              value={fast ? FAST_ON : FAST_OFF}
            >
              <DropdownMenuRadioItem
                data-testid="coding-session-fast-option"
                value={FAST_OFF}
              >
                Off
              </DropdownMenuRadioItem>
              <DropdownMenuRadioItem
                data-testid="coding-session-fast-option"
                value={FAST_ON}
              >
                On
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
