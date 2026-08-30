import * as React from "react";

import {
  codingSessionModelChoices,
  joinCodingSessionModelId,
  resolveCodingSessionContext,
  resolveCodingSessionThinking,
  splitCodingSessionModelId,
} from "@/features/coding-sessions/lib/codingSessionModelChoice";
import {
  readCodingSessionModelFavorites,
  toggleCodingSessionModelFavorite,
  writeCodingSessionModelFavorites,
} from "@/features/coding-sessions/lib/codingSessionModelFavorites";
import { codingSessionProviderBaseModels } from "@/features/coding-sessions/lib/codingSessionModelPickerModel";
import { formatCodingSessionRuntimeLabel } from "../lib/codingSessionLabels";
import { resolveCodingSessionSeatIdentityModel } from "../lib/codingSessionHireModel";
import {
  codingSessionAuthRemediation,
  formatCodingSessionProviderLabel,
  isNewCodingSessionTargetReady,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import { CodingSessionAccessNotice } from "./CodingSessionAccessNotice";
import { CodingSessionModelPicker } from "./CodingSessionModelPicker";
import { CodingSessionRuntimeConnect } from "./CodingSessionRuntimeConnect";
import { CodingSessionTraitsPicker } from "./CodingSessionTraitsPicker";
import {
  codingSessionCreateModelDisclosure,
  type CodingSessionCreateModelCatalog,
} from "./useNewCodingSessionCreate";

/**
 * Which provider runs the session, on which model, with how much thinking.
 *
 * Shared by every surface that founds or joins an execution — the create
 * dialog, the add-provider dialog, and the pending screen's remediation —
 * because all three are asking the same question and a second copy would
 * drift the moment one of them learned about a new runtime.
 */
export function NewCodingSessionProviderPicker({
  disabled,
  model,
  noteForTarget,
  onLoginLaunched,
  onModelChange,
  onTargetChange,
  selectedTarget,
  targets,
}: {
  disabled: boolean;
  model: string | null;
  /**
   * Extra parenthetical for an option, e.g. "already in this session" in the
   * join flow. Availability suffixes still win — a signed-out runtime's
   * remediation is the more urgent thing to say.
   */
  noteForTarget?: (target: NewCodingSessionTarget) => string | null;
  /** Forwarded to each unavailable runtime's Connect button. */
  onLoginLaunched?: (input: { runtime: string; headless: boolean }) => void;
  onModelChange: (model: string) => void;
  onTargetChange: (selectionKey: string) => void;
  selectedTarget: NewCodingSessionTarget | null;
  targets: readonly NewCodingSessionTarget[];
}) {
  const models = selectedTarget?.provider.allowedModels ?? [];
  // One control per decision. Codex encodes reasoning effort in the model id,
  // so live discovery (§2 item 39) turned four models into thirty rows (§2
  // item 45); the picker lists models and this list supplies the levels.
  const choices = React.useMemo(
    () => codingSessionModelChoices(models),
    [models],
  );
  const selected = splitCodingSessionModelId(model ?? "");
  const thinkingLevels = choices.thinkingByModel.get(selected.model) ?? [];
  const [favorites, setFavorites] = React.useState<ReadonlySet<string>>(
    readCodingSessionModelFavorites,
  );
  const toggleFavorite = React.useCallback((favoriteKey: string) => {
    setFavorites((current) => {
      const next = toggleCodingSessionModelFavorite(current, favoriteKey);
      writeCodingSessionModelFavorites(next);
      return next;
    });
  }, []);
  // Every target is a rail, ready or not: a signed-out runtime is exactly what
  // a person needs to *see* to fix it, and its rows say why they are disabled.
  const pickerProviders = React.useMemo(
    () =>
      targets.map((target) => ({
        selectionKey: target.selectionKey,
        runtime: target.provider.runtime,
        label: formatCodingSessionProviderLabel({
          runtime: target.provider.runtime,
          providerInstanceRef: target.provider.providerInstanceRef,
        }),
        models: codingSessionProviderBaseModels(target.provider.allowedModels),
        ready: isNewCodingSessionTargetReady(target),
        unavailableNote:
          target.availability?.state === "needs_auth"
            ? "sign-in needed"
            : target.availability?.state === "missing"
              ? "not installed"
              : (noteForTarget?.(target) ?? null),
      })),
    [noteForTarget, targets],
  );
  // One remediation row per unavailable runtime — a rail glyph and a disabled
  // row say *that* something is wrong; only this says what to do about it.
  const unavailableRuntimes = [
    ...new Map(
      targets.flatMap((target) =>
        !isNewCodingSessionTargetReady(target) && target.availability?.hint
          ? [
              [
                target.provider.runtime,
                {
                  runtime: target.provider.runtime,
                  label:
                    target.availability.label ??
                    formatCodingSessionRuntimeLabel(target.provider.runtime),
                  state: target.availability.state,
                  hint: target.availability.hint,
                },
              ] as const,
            ]
          : [],
      ),
    ).values(),
  ];
  // Picking a model may also change provider: the rail is a filter over one
  // list, so the two selections settle together instead of in two steps.
  const handlePick = React.useCallback(
    (pick: { selectionKey: string; model: string }) => {
      if (pick.selectionKey !== selectedTarget?.selectionKey) {
        onTargetChange(pick.selectionKey);
      }
      const target = targets.find(
        (candidate) => candidate.selectionKey === pick.selectionKey,
      );
      const nextChoices = codingSessionModelChoices(
        target?.provider.allowedModels ?? [],
      );
      onModelChange(
        joinCodingSessionModelId(
          pick.model,
          resolveCodingSessionThinking(
            nextChoices,
            pick.model,
            selected.thinking,
          ),
          resolveCodingSessionContext(
            nextChoices,
            pick.model,
            selected.context,
          ),
        ),
      );
    },
    [
      onModelChange,
      onTargetChange,
      selected.context,
      selected.thinking,
      selectedTarget,
      targets,
    ],
  );
  return (
    <div className="flex flex-col gap-3 sm:flex-row">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <span className="text-xs font-medium text-muted-foreground">
          Provider and model
        </span>
        <CodingSessionModelPicker
          disabled={disabled}
          favorites={favorites}
          model={selected.model === "" ? null : selected.model}
          onModelChange={handlePick}
          onToggleFavorite={toggleFavorite}
          providers={pickerProviders}
          selectionKey={selectedTarget?.selectionKey ?? null}
        />
        {unavailableRuntimes.map((entry) => (
          <div className="flex flex-col gap-1.5" key={entry.runtime}>
            <p className="text-2xs text-muted-foreground">{entry.hint}</p>
            {entry.state === "needs_auth" ? (
              <CodingSessionRuntimeConnect
                disabled={disabled}
                label={entry.label}
                onLoginLaunched={onLoginLaunched}
                runtime={entry.runtime}
              />
            ) : null}
          </div>
        ))}
      </div>
      <div className="flex shrink-0 flex-col gap-2">
        <span className="text-xs font-medium text-muted-foreground">
          Thinking
        </span>
        <CodingSessionTraitsPicker
          className="min-w-36"
          context={selected.context}
          contexts={choices.contextByModel.get(selected.model) ?? []}
          disabled={disabled}
          hasBareModel={choices.bareModels.has(selected.model)}
          onContextChange={(context) =>
            onModelChange(
              joinCodingSessionModelId(
                selected.model,
                selected.thinking,
                context,
              ),
            )
          }
          onThinkingChange={(thinking) =>
            onModelChange(
              joinCodingSessionModelId(
                selected.model,
                thinking,
                selected.context,
              ),
            )
          }
          thinking={selected.thinking}
          thinkingLevels={thinkingLevels}
        />
      </div>
      <div className="flex shrink-0 flex-col gap-2">
        <span className="text-xs font-medium text-muted-foreground">
          Access
        </span>
        <div className="flex h-9 items-center">
          <CodingSessionAccessNotice />
        </div>
      </div>
    </div>
  );
}

/**
 * What the create is about to record about the model, when that is not a model.
 *
 * Rendered directly under the picker on every surface that founds a session.
 * Twice on 2026-08-28 a seat was created carrying the id `default` — a label,
 * not a model — and no screen said so, so the record of which weights ran the
 * session simply does not exist. Nothing here changes what is written; it
 * stops the screen from implying something else was.
 */
export function NewCodingSessionModelDisclosure({
  catalog,
  model,
  note = null,
}: {
  /** The runtime's published list, when it has been read. */
  catalog?: CodingSessionCreateModelCatalog | null;
  /** The id the create would carry right now. */
  model: string | null;
  /**
   * One more thing this surface owes the person about the model — today, a
   * seated identity whose record names one the runtime cannot run.
   */
  note?: string | null;
}) {
  const disclosure = codingSessionCreateModelDisclosure({ catalog, model });
  if (disclosure === null && note === null) return null;
  return (
    <>
      {disclosure === null ? null : (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-model-disclosure"
        >
          {disclosure}
        </p>
      )}
      {note === null ? null : (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-seat-model-note"
        >
          {note}
        </p>
      )}
    </>
  );
}

/**
 * The model a seated identity asks this create to run on, if any.
 *
 * A managed agent's record names a model; the create dialog used to ignore it
 * and preselect the adapter's own default, which on claude-primary is the id
 * `default` — so seating an identity produced a session that named no model at
 * all (item 89a, live twice on 2026-08-28).
 *
 * Three answers, all of them said out loud rather than guessed at: the
 * record's model when this runtime publishes it, nothing when the person has
 * chosen a model by hand (their pick outranks the record), and nothing *plus a
 * note* when the record names a model this runtime does not offer — a silent
 * downgrade to the runtime default is the same class of lie as the id it would
 * replace.
 *
 * The matching itself is {@link resolveCodingSessionSeatIdentityModel}, the
 * same table the hire host reads an identity's model through: matching here by
 * exact string is what made a record saying `claude-fable-5` unrunnable on a
 * runtime publishing `claude-fable-5[1m]` (item 90 lane C).
 */
export function resolveNewCodingSessionSeatModel(input: {
  /** The seated identity's own model id, or null when it names none. */
  agentModel: string | null;
  /** Model ids the selected runtime actually publishes. */
  allowedModels: readonly string[];
  /** Whether the person has picked a model by hand. */
  selectionExplicit: boolean;
}): { model: string | null; note: string | null } {
  return resolveCodingSessionSeatIdentityModel(input);
}

/** The remediation a failed receipt asks for: which runtime, and how to fix it. */
export function ProviderLoginNeeded({
  onLoginLaunched,
  runtime,
}: {
  /** Forwarded to the Connect button under the remediation text. */
  onLoginLaunched?: (input: { runtime: string; headless: boolean }) => void;
  runtime?: { runtime: string; label?: string } | null;
}) {
  const remediation = codingSessionAuthRemediation(runtime);
  // The remediation copy falls back to claude with no runtime context; the
  // Connect button targets the same fallback so the two never disagree.
  const runtimeSlug = runtime?.runtime ?? "claude";
  // Split the message around the command so it renders as an inline <code>
  // block; a runtime with no known command shows the sentence as-is.
  const [before, after] = remediation.command
    ? remediation.message.split(`\`${remediation.command}\``)
    : [remediation.message, undefined];
  return (
    <div
      className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-sm"
      data-testid="new-coding-session-auth-required"
      role="alert"
    >
      <p className="font-medium">{remediation.title}</p>
      <p className="mt-1 text-muted-foreground">
        {before}
        {remediation.command && after !== undefined ? (
          <>
            <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
              {remediation.command}
            </code>
            {after}
          </>
        ) : null}
      </p>
      <div className="mt-2">
        <CodingSessionRuntimeConnect
          label={runtime?.label ?? formatCodingSessionRuntimeLabel(runtimeSlug)}
          onLoginLaunched={onLoginLaunched}
          runtime={runtimeSlug}
        />
      </div>
    </div>
  );
}
