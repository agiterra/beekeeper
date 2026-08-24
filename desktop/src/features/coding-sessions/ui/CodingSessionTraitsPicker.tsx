import { ChevronDown } from "lucide-react";

import {
  codingSessionContextLabel,
  codingSessionTraitsSummary,
} from "@/features/coding-sessions/lib/codingSessionModelDisplay";
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
 */
export function CodingSessionTraitsPicker({
  className,
  context,
  contexts,
  disabled = false,
  hasBareModel,
  onContextChange,
  onThinkingChange,
  thinking,
  thinkingLevels,
}: {
  className?: string;
  context: string | null;
  contexts: readonly string[];
  disabled?: boolean;
  hasBareModel: boolean;
  onContextChange: (context: string | null) => void;
  onThinkingChange: (thinking: string | null) => void;
  thinking: string | null;
  thinkingLevels: readonly string[];
}) {
  if (thinkingLevels.length === 0 && contexts.length === 0) return null;
  const summary =
    codingSessionTraitsSummary({ thinking, context }) ?? "Adapter default";

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
              Thinking
            </DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) =>
                onThinkingChange(value === ADAPTER_DEFAULT ? null : value)
              }
              value={thinking ?? ADAPTER_DEFAULT}
            >
              {hasBareModel ? (
                <DropdownMenuRadioItem
                  data-testid="coding-session-thinking-option"
                  value={ADAPTER_DEFAULT}
                >
                  Adapter default
                </DropdownMenuRadioItem>
              ) : null}
              {thinkingLevels.map((level) => (
                <DropdownMenuRadioItem
                  data-testid="coding-session-thinking-option"
                  key={level}
                  value={level}
                >
                  {level}
                </DropdownMenuRadioItem>
              ))}
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
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
