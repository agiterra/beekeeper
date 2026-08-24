import { Bot, Check, Search, Sparkles, Star, Terminal } from "lucide-react";
import * as React from "react";

import { codingSessionModelDisplayName } from "@/features/coding-sessions/lib/codingSessionModelDisplay";
import {
  CODING_SESSION_MODEL_FAVORITES_RAIL,
  codingSessionModelFavoriteKey,
  codingSessionModelPickerInitialRail,
  codingSessionModelPickerRows,
  type CodingSessionModelPickerProvider,
} from "@/features/coding-sessions/lib/codingSessionModelPickerModel";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

/**
 * Provider and model in one control.
 *
 * Two separate `<select>`s made the person answer "whose model" before they
 * could see any model, and listed every id of every provider in one column.
 * The rail answers "whose" as a filter rather than a prerequisite, favourites
 * put the two models someone actually uses one keystroke away, and search
 * makes a long list navigable instead of scrollable.
 *
 * Reasoning effort is deliberately **not** here: it is a separate decision
 * about the same model, and folding it in is what produced thirty rows.
 */

/**
 * Runtime glyphs are lucide, not vendor marks.
 *
 * Buzz's brand marks live in the onboarding feature, and a feature may not
 * import another feature's internals. The tooltip carries the provider's real
 * name, so the glyph only has to distinguish rails at a glance.
 */
function RuntimeGlyph({
  className,
  runtime,
}: {
  className?: string;
  runtime: string;
}) {
  const normalized = runtime.trim().toLowerCase();
  const Icon =
    normalized === "claude"
      ? Sparkles
      : normalized === "codex"
        ? Terminal
        : Bot;
  return <Icon aria-hidden className={className} />;
}

function RailButton({
  active,
  children,
  label,
  onSelect,
  testId,
}: {
  active: boolean;
  children: React.ReactNode;
  label: string;
  onSelect: () => void;
  testId: string;
}) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            aria-label={label}
            aria-pressed={active}
            className={cn(
              "relative flex aspect-square w-full items-center justify-center rounded-md text-muted-foreground transition-colors",
              "hover:bg-muted/70 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
              active && "bg-muted text-foreground",
            )}
            data-testid={testId}
            onClick={onSelect}
            type="button"
          >
            {children}
            {active ? (
              <span
                aria-hidden
                className="absolute top-1/2 -right-1 h-5 w-0.5 -translate-y-1/2 rounded-l-full bg-primary"
              />
            ) : null}
          </button>
        </TooltipTrigger>
        <TooltipContent side="left">{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

export function CodingSessionModelPicker({
  disabled = false,
  favorites,
  model,
  onModelChange,
  onToggleFavorite,
  providers,
  selectionKey,
}: {
  disabled?: boolean;
  favorites: ReadonlySet<string>;
  /** Base model id — the thinking level is a separate control. */
  model: string | null;
  onModelChange: (input: { selectionKey: string; model: string }) => void;
  onToggleFavorite: (favoriteKey: string) => void;
  providers: readonly CodingSessionModelPickerProvider[];
  selectionKey: string | null;
}) {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const [rail, setRail] = React.useState(() =>
    codingSessionModelPickerInitialRail({
      providers,
      favorites,
      selectionKey,
      model,
    }),
  );
  // Reopening should land where the person is now, not where they were when
  // the component mounted.
  const openPicker = () => {
    setQuery("");
    setRail(
      codingSessionModelPickerInitialRail({
        providers,
        favorites,
        selectionKey,
        model,
      }),
    );
    setOpen(true);
  };

  const [active, setActive] = React.useState(0);
  const rows = codingSessionModelPickerRows({
    providers,
    favorites,
    rail,
    query,
  });
  const rowsKey = rows.map((row) => row.rowKey).join("|");
  // biome-ignore lint/correctness/useExhaustiveDependencies: rowsKey is the content signal for `rows`
  React.useEffect(() => setActive(0), [rowsKey]);
  const pick = React.useCallback(
    (row: { ready: boolean; selectionKey: string; model: string }) => {
      if (!row.ready) return;
      onModelChange({ selectionKey: row.selectionKey, model: row.model });
      setOpen(false);
    },
    [onModelChange],
  );
  /**
   * The hints the rows print have to work.
   *
   * ⌘N used to be rendered beside every favourite with nothing listening for
   * it — a keyboard shortcut that was only a picture of one. Arrow keys and
   * Enter come with it, because a searchable list a person cannot leave the
   * text field to use is a list they still have to reach for the mouse in.
   */
  const handlePanelKeyDown = (event: React.KeyboardEvent) => {
    // Escape is handled here rather than left to the dismissable layer: the
    // panel proved not to close on it (caught by e2e), and a keyboard-driven
    // list a person cannot leave with Escape is a trap.
    if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && /^[1-9]$/.test(event.key)) {
      const target = rows.find((row) => row.shortcut === Number(event.key));
      if (target) {
        event.preventDefault();
        pick(target);
      }
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (rows.length === 0) return;
      event.preventDefault();
      setActive((current) => {
        const next = event.key === "ArrowDown" ? current + 1 : current - 1;
        return (next + rows.length) % rows.length;
      });
      return;
    }
    if (event.key === "Enter") {
      const target = rows[active];
      if (target) {
        event.preventDefault();
        pick(target);
      }
    }
  };

  const selectedProvider =
    providers.find((provider) => provider.selectionKey === selectionKey) ??
    null;
  // The glyph already says whose model this is, so the trigger says which.
  const triggerLabel = codingSessionModelDisplayName(model ?? "");

  return (
    <Popover
      onOpenChange={(next) => (next ? openPicker() : setOpen(false))}
      open={open}
    >
      <PopoverTrigger asChild>
        <Button
          className="h-9 min-w-0 justify-start gap-2"
          data-testid="coding-session-model-picker"
          disabled={disabled || providers.length === 0}
          type="button"
          variant="outline"
        >
          {selectedProvider ? (
            <RuntimeGlyph
              className="size-4 shrink-0"
              runtime={selectedProvider.runtime}
            />
          ) : null}
          <span className="truncate">{triggerLabel}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="flex w-[22rem] gap-0 p-0"
        collisionPadding={12}
        data-testid="coding-session-model-picker-panel"
        onKeyDown={handlePanelKeyDown}
        side="bottom"
        sideOffset={6}
      >
        <div className="flex w-11 shrink-0 flex-col gap-1 border-r border-border/60 bg-muted/30 p-1">
          <RailButton
            active={rail === CODING_SESSION_MODEL_FAVORITES_RAIL}
            label="Favorites"
            onSelect={() => setRail(CODING_SESSION_MODEL_FAVORITES_RAIL)}
            testId="coding-session-model-rail-favorites"
          >
            <Star className="size-4 fill-current" />
          </RailButton>
          {providers.map((provider) => (
            <RailButton
              active={rail === provider.selectionKey}
              key={provider.selectionKey}
              label={
                provider.ready
                  ? provider.label
                  : `${provider.label} — ${provider.unavailableNote ?? "unavailable"}`
              }
              onSelect={() => setRail(provider.selectionKey)}
              testId={`coding-session-model-rail-${provider.runtime}`}
            >
              <RuntimeGlyph
                className={cn("size-4", !provider.ready && "opacity-40")}
                runtime={provider.runtime}
              />
            </RailButton>
          ))}
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-2 border-b border-border/60 px-3 py-2">
            <Search
              aria-hidden
              className="size-4 shrink-0 text-muted-foreground"
            />
            <Input
              aria-label="Search models"
              className="h-7 border-0 px-0 shadow-none focus-visible:ring-0"
              data-testid="coding-session-model-search"
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search models…"
              value={query}
            />
          </div>
          <div className="max-h-72 overflow-y-auto p-1">
            {rows.length === 0 ? (
              <p
                className="px-2 py-6 text-center text-xs text-muted-foreground"
                data-testid="coding-session-model-empty"
              >
                {rail === CODING_SESSION_MODEL_FAVORITES_RAIL && query === ""
                  ? "No favorites yet — star a model to pin it here."
                  : "No model matches that search."}
              </p>
            ) : null}
            {rows.map((row, index) => {
              const isSelected =
                row.selectionKey === selectionKey && row.model === model;
              // Inside one provider's rail the provider name is on every row
              // and tells nobody anything; the id it stands for does. In
              // Favourites, where providers mix, the opposite is true.
              const secondary =
                rail === CODING_SESSION_MODEL_FAVORITES_RAIL
                  ? row.providerLabel
                  : row.model === ""
                    ? "the adapter chooses"
                    : row.model;
              return (
                <div
                  className={cn(
                    "flex items-center gap-2 rounded-md px-2 py-1.5",
                    index === active && "bg-muted/70",
                    isSelected && "bg-muted",
                  )}
                  data-active={index === active ? "true" : undefined}
                  data-selected={isSelected ? "true" : undefined}
                  data-testid="coding-session-model-row"
                  key={row.rowKey}
                >
                  <Check
                    aria-hidden
                    className={cn(
                      "size-3.5 shrink-0",
                      isSelected ? "opacity-100" : "opacity-0",
                    )}
                  />
                  <button
                    className={cn(
                      "flex min-w-0 flex-1 flex-col items-start gap-0.5 text-left",
                      !row.ready && "opacity-60",
                    )}
                    data-model={row.model}
                    data-testid="coding-session-model-row-select"
                    disabled={!row.ready}
                    onClick={() => pick(row)}
                    onMouseEnter={() => setActive(index)}
                    type="button"
                  >
                    <span className="w-full truncate text-sm">
                      {codingSessionModelDisplayName(row.model)}
                    </span>
                    <span className="flex w-full items-center gap-1 truncate text-2xs text-muted-foreground">
                      {rail === CODING_SESSION_MODEL_FAVORITES_RAIL ? (
                        <RuntimeGlyph
                          className="size-3"
                          runtime={row.runtime}
                        />
                      ) : null}
                      {row.ready
                        ? secondary
                        : `${secondary} — ${row.unavailableNote ?? "unavailable"}`}
                    </span>
                  </button>
                  {row.shortcut === null ? null : (
                    <span className="shrink-0 rounded border border-border/60 px-1 text-2xs text-muted-foreground tabular-nums">
                      ⌘{row.shortcut}
                    </span>
                  )}
                  <button
                    aria-label={
                      row.favorite
                        ? `Unpin ${row.model}`
                        : `Pin ${row.model} to favorites`
                    }
                    aria-pressed={row.favorite}
                    className="shrink-0 rounded p-1 text-muted-foreground hover:text-foreground"
                    data-testid="coding-session-model-favorite"
                    onClick={() =>
                      onToggleFavorite(
                        codingSessionModelFavoriteKey(
                          row.selectionKey,
                          row.model,
                        ),
                      )
                    }
                    type="button"
                  >
                    <Star
                      className={cn("size-3.5", row.favorite && "fill-current")}
                    />
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
