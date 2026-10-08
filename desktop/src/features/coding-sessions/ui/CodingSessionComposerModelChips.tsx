import { Check, ChevronDown } from "lucide-react";
import * as React from "react";

import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
  createCodingSessionCommandId,
  publishCodingSessionModelSet,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildPinnedCodingSessionIngressAuthority } from "@/features/coding-sessions/lib/codingSessionIngressAuthority";
import {
  codingSessionContextLabel,
  codingSessionEffortLabel,
  codingSessionModelTitle,
  codingSessionTraitsSummary,
} from "@/features/coding-sessions/lib/codingSessionModelDisplay";
import {
  CODING_SESSION_DEFAULT_EFFORT,
  type CodingSessionModelOffer,
  codingSessionModelOptions,
  codingSessionOfferChoices,
  codingSessionProviderPickerModels,
  joinCodingSessionModelSelection,
  resolveCodingSessionModelPick,
  splitCodingSessionModelSelection,
} from "@/features/coding-sessions/lib/codingSessionModelOptions";
import {
  type CodingSessionModelSwitchMetadata,
  type CodingSessionModelSwitchReceipt,
  type CodingSessionModelSwitchRequest,
  type CodingSessionModelSwitchState,
  foldCodingSessionModelSwitch,
  isCodingSessionModelSwitchInFlight,
} from "@/features/coding-sessions/lib/codingSessionModelSwitch";
import {
  codingSessionModelSwitchNote,
  codingSessionModelSwitchRows,
} from "@/features/coding-sessions/lib/codingSessionModelSwitchRows";
import { subscribeToObservedCodingSessionEvents } from "@/features/coding-sessions/lib/codingSessionObservedEvents";
import { classifyTrustedCodingSessionIngressEvent } from "@/features/coding-sessions/lib/codingSessionTrustedIngress";
import { useTrustedCodingSessionIngress } from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { useCodingSessionProviderCatalog } from "@/features/coding-sessions/useCodingSessionProviderCatalog";

import {
  CodingSessionComposerProviderMark,
  type CodingSessionProviderMarkKind,
} from "./CodingSessionComposerProviderMark";

/**
 * Where a `thread.model.set` goes and whose answers count (SV-35).
 *
 * `providerInstanceRef` is metadata's `provider`, which is the catalog's
 * `providerInstanceRef` for the same instance: the offer is that one entry's,
 * never another provider's (spec M7).
 */
export type CodingSessionModelSwitchBinding = {
  channelId: string;
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string | null;
  providerInstanceRef: string | null;
};

type Props = {
  binding: CodingSessionModelSwitchBinding;
  /** Metadata's `model`: the only model this control ever shows as running. */
  model: string | null;
  providerLabel: string | null;
  providerMark: CodingSessionProviderMarkKind | null;
  runtimeLabel: string | null;
  chipClassName: string;
};

/**
 * The model chip and the traits chip, as in T3's composer, for an execution
 * whose metadata says `modelSwitch` and a viewer who may control it. Every
 * other case keeps the deck's display-only identity chip.
 */
export function CodingSessionComposerModelChips({
  binding,
  model,
  providerLabel,
  providerMark,
  runtimeLabel,
  chipClassName,
}: Props) {
  const offer = useCodingSessionModelSwitchOffer(binding);
  const { state, request, publishError, requestSwitch } =
    useCodingSessionModelSwitch(binding);
  const label = React.useCallback(
    (selection: string) => codingSessionSelectionLabel(selection, offer),
    [offer],
  );
  const current = model ? splitSelection(model, offer) : null;
  const inFlight = isCodingSessionModelSwitchInFlight(state);
  const note = publishError
    ? { tone: "warning" as const, text: publishError }
    : codingSessionModelSwitchNote(state, label);
  const rows = codingSessionModelSwitchRows({
    availability: "available",
    state,
    effectiveHasEffort: current?.effort != null,
    label,
  });
  const choose = (selection: string) => {
    if (inFlight || selection === model) return;
    void requestSwitch(selection);
  };
  const modelTitle = current
    ? codingSessionModelTitle(
        current.model,
        offer ? (detailName(offer, current.model) ?? null) : null,
      )
    : "Model not reported";
  const traits = current
    ? codingSessionTraitsSummary({
        thinking: current.effort,
        context: current.context,
        fast: current.fast,
      })
    : null;

  return (
    <>
      {request ? (
        <CodingSessionModelSwitchWatch
          channelId={binding.channelId}
          commandId={request.commandId}
          providerAuthorityPubkey={binding.providerAuthorityPubkey}
        />
      ) : null}
      <Popover>
        <PopoverTrigger asChild>
          <button
            aria-label="Change this execution's model"
            className={cn(chipClassName, "min-w-0 text-foreground/75")}
            data-testid="coding-session-control-identity"
            title={[providerLabel ?? runtimeLabel, modelTitle]
              .filter(Boolean)
              .join(" · ")}
            type="button"
          >
            <CodingSessionComposerProviderMark kind={providerMark} />
            <span className="max-w-48 truncate">{modelTitle}</span>
            <ChevronDown aria-hidden className="size-3 shrink-0 opacity-60" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-72 p-2"
          data-testid="coding-session-model-chip-popover"
          side="top"
        >
          <p className="px-2 pt-1 text-xs font-medium text-muted-foreground">
            {[providerLabel, runtimeLabel].filter(Boolean).join(" · ") ||
              "Model"}
          </p>
          {offer && current ? (
            <ul className="mt-1 grid gap-0.5">
              {codingSessionProviderPickerModels(offer).models.map((base) => (
                <li key={base}>
                  <SwitchOption
                    active={base === current.model}
                    disabled={inFlight}
                    label={codingSessionModelTitle(
                      base,
                      detailName(offer, base),
                    )}
                    onSelect={() =>
                      choose(
                        resolveCodingSessionModelPick({
                          offer,
                          model: base,
                          previous: current,
                        }),
                      )
                    }
                    testId={`coding-session-model-option-${base}`}
                  />
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-2 py-2 text-xs text-muted-foreground">
              This computer cannot read this provider's model list, so it has no
              other model to offer.
            </p>
          )}
          <SwitchRows rows={rows} />
        </PopoverContent>
      </Popover>

      {current && offer ? (
        <CodingSessionComposerTraitsChip
          chipClassName={chipClassName}
          current={current}
          disabled={inFlight}
          offer={offer}
          onChoose={choose}
          rows={rows}
          traits={traits}
        />
      ) : traits ? (
        <span
          className="hidden shrink-0 rounded-full px-2 py-0.5 sm:inline"
          data-testid="coding-session-control-traits"
        >
          {traits}
        </span>
      ) : null}

      {note ? (
        <span
          className={cn(
            "max-w-64 truncate text-2xs",
            note.tone === "warning"
              ? "text-amber-700 dark:text-amber-400"
              : "text-muted-foreground",
          )}
          data-testid="coding-session-model-switch-note"
          role="status"
          title={note.text}
        >
          {note.text}
        </span>
      ) : null}
    </>
  );
}

type Selection = ReturnType<typeof splitCodingSessionModelSelection>;

function CodingSessionComposerTraitsChip({
  chipClassName,
  current,
  disabled,
  offer,
  onChoose,
  rows,
  traits,
}: {
  chipClassName: string;
  current: Selection;
  disabled: boolean;
  offer: CodingSessionModelOffer;
  onChoose: (selection: string) => void;
  rows: string[];
  traits: string | null;
}) {
  const options = codingSessionModelOptions(
    offer,
    current.model,
    current.context,
  );
  const contexts =
    codingSessionOfferChoices(offer).contextByModel.get(current.model) ?? [];
  const pick = (patch: Partial<Selection>) =>
    onChoose(joinCodingSessionModelSelection({ ...current, ...patch }));
  const effortLevels = options.levels.filter(
    (level) => level !== CODING_SESSION_DEFAULT_EFFORT,
  );
  const offersDefault = options.defaultOption !== "none";
  if (effortLevels.length === 0 && contexts.length < 2 && !options.fastMode) {
    return traits ? (
      <span
        className="hidden shrink-0 rounded-full px-2 py-0.5 sm:inline"
        data-testid="coding-session-control-traits"
      >
        {traits}
      </span>
    ) : null;
  }
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          aria-label="Change this execution's effort and context"
          className={cn(chipClassName, "shrink-0")}
          data-testid="coding-session-control-traits"
          type="button"
        >
          <span>{traits ?? "Default"}</span>
          <ChevronDown aria-hidden className="size-3 shrink-0 opacity-60" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-64 p-2"
        data-testid="coding-session-traits-chip-popover"
        side="top"
      >
        {effortLevels.length > 0 ? (
          <TraitGroup title="Effort">
            {offersDefault ? (
              <SwitchOption
                active={current.effort === null}
                disabled={disabled}
                label="Default"
                onSelect={() => pick({ effort: null })}
                testId="coding-session-effort-option-default"
              />
            ) : null}
            {effortLevels.map((level) => (
              <SwitchOption
                active={current.effort === level}
                disabled={disabled}
                key={level}
                label={codingSessionEffortLabel(level)}
                onSelect={() => pick({ effort: level })}
                testId={`coding-session-effort-option-${level}`}
              />
            ))}
          </TraitGroup>
        ) : null}
        {contexts.length > 1 ? (
          <TraitGroup title="Context window">
            {contexts.map((context) => (
              <SwitchOption
                active={current.context === context}
                disabled={disabled}
                key={context}
                label={codingSessionContextLabel(context)}
                onSelect={() => pick({ context })}
                testId={`coding-session-context-option-${context}`}
              />
            ))}
          </TraitGroup>
        ) : null}
        {options.fastMode ? (
          <TraitGroup title="Speed">
            <SwitchOption
              active={current.fast}
              disabled={disabled}
              label="Fast mode"
              onSelect={() => pick({ fast: !current.fast })}
              testId="coding-session-fast-option"
            />
          </TraitGroup>
        ) : null}
        <SwitchRows rows={rows} />
      </PopoverContent>
    </Popover>
  );
}

function TraitGroup({
  children,
  title,
}: {
  children: React.ReactNode;
  title: string;
}) {
  return (
    <div className="mb-1">
      <p className="px-2 pt-1 text-xs font-medium text-muted-foreground">
        {title}
      </p>
      <div className="mt-0.5 grid gap-0.5">{children}</div>
    </div>
  );
}

function SwitchOption({
  active,
  disabled,
  label,
  onSelect,
  testId,
}: {
  active: boolean;
  disabled: boolean;
  label: string;
  onSelect: () => void;
  testId: string;
}) {
  return (
    <button
      aria-pressed={active}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
      data-testid={testId}
      disabled={disabled}
      onClick={onSelect}
      type="button"
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {active ? <Check aria-hidden className="size-3.5 shrink-0" /> : null}
    </button>
  );
}

function SwitchRows({ rows }: { rows: string[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="mt-2 grid gap-1 border-t border-border/60 px-2 pt-2">
      {rows.map((row) => (
        <p className="text-xs text-muted-foreground" key={row}>
          {row}
        </p>
      ))}
    </div>
  );
}

/**
 * Keeps a command-scoped, provider-pinned ingress mounted while a switch is
 * unanswered, so its receipt is fetched even where no other surface already
 * subscribes. Its events reach {@link useCodingSessionModelSwitch} over the
 * observed-events bus, which re-verifies them; it renders nothing.
 */
function CodingSessionModelSwitchWatch({
  channelId,
  commandId,
  providerAuthorityPubkey,
}: {
  channelId: string;
  commandId: string;
  providerAuthorityPubkey: string | null;
}) {
  const channelIds = React.useMemo(() => [channelId], [channelId]);
  useTrustedCodingSessionIngress(
    channelIds,
    commandId,
    providerAuthorityPubkey,
    undefined,
    null,
    "pinned",
  );
  return null;
}

/** The provider instance's offer, from its own signed 44222, or `null`. */
function useCodingSessionModelSwitchOffer(
  binding: CodingSessionModelSwitchBinding,
): CodingSessionModelOffer | null {
  const channelIds = React.useMemo(
    () => [binding.channelId],
    [binding.channelId],
  );
  const catalog = useCodingSessionProviderCatalog(channelIds);
  const { providerAuthorityPubkey, providerInstanceRef } = binding;
  return React.useMemo(() => {
    if (!providerAuthorityPubkey || !providerInstanceRef) return null;
    const entry = catalog.entries
      .filter(
        (candidate) =>
          candidate.channelId === binding.channelId &&
          candidate.signerPubkey === providerAuthorityPubkey,
      )
      .sort((left, right) => right.createdAt - left.createdAt)[0];
    const provider = entry?.catalog.providers.find(
      (candidate) => candidate.providerInstanceRef === providerInstanceRef,
    );
    return provider
      ? { allowedModels: provider.allowedModels, models: provider.models }
      : null;
  }, [
    catalog.entries,
    binding.channelId,
    providerAuthorityPubkey,
    providerInstanceRef,
  ]);
}

/**
 * The switch this view asked for, and what the provider made of it.
 *
 * The request lives in this mount only: it is a fact about what *this*
 * person asked, and once answered, metadata carries everything that lasts.
 */
function useCodingSessionModelSwitch(binding: CodingSessionModelSwitchBinding) {
  const targetKey = buildCodingSessionTargetKey(binding.target);
  const [request, setRequest] = React.useState<
    (CodingSessionModelSwitchRequest & { targetKey: string }) | null
  >(null);
  const [publishError, setPublishError] = React.useState<string | null>(null);
  const [receipts, setReceipts] = React.useState<
    CodingSessionModelSwitchReceipt[]
  >([]);
  const [metadata, setMetadata] = React.useState<
    CodingSessionModelSwitchMetadata[]
  >([]);
  const { channelId, providerAuthorityPubkey } = binding;
  const activeRequest = request?.targetKey === targetKey ? request : null;
  const commandId = activeRequest?.commandId ?? null;

  React.useEffect(() => {
    if (!commandId || !providerAuthorityPubkey) return;
    const authority = buildPinnedCodingSessionIngressAuthority(
      providerAuthorityPubkey,
    );
    const channels = new Set([channelId]);
    const seen = new Set<string>();
    let order = 0;
    const receive = (events: readonly RelayEvent[]) => {
      const nextReceipts: CodingSessionModelSwitchReceipt[] = [];
      const nextMetadata: CodingSessionModelSwitchMetadata[] = [];
      for (const event of events) {
        if (
          seen.has(event.id) ||
          (event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT &&
            event.kind !== KIND_CODING_SESSION_METADATA)
        ) {
          continue;
        }
        seen.add(event.id);
        const classified = classifyTrustedCodingSessionIngressEvent(
          event,
          channels,
          authority,
        );
        order += 1;
        if (
          classified.kind === "receipt" &&
          classified.receipt.commandId === commandId
        ) {
          nextReceipts.push({
            eventId: event.id,
            commandId,
            status: classified.receipt.status,
            error: classified.receipt.error,
            createdAt: event.created_at,
            order,
          });
        } else if (
          classified.kind === "metadata" &&
          classified.targetKey === targetKey
        ) {
          nextMetadata.push({
            eventId: event.id,
            model: classified.metadata.model,
            createdAt: event.created_at,
            order,
          });
        }
      }
      if (nextReceipts.length > 0) {
        setReceipts((previous) => [...previous, ...nextReceipts]);
      }
      if (nextMetadata.length > 0) {
        setMetadata((previous) => [...previous, ...nextMetadata]);
      }
    };
    return subscribeToObservedCodingSessionEvents(receive);
  }, [channelId, commandId, providerAuthorityPubkey, targetKey]);

  const state: CodingSessionModelSwitchState = React.useMemo(
    () =>
      foldCodingSessionModelSwitch({
        request: activeRequest,
        receipts,
        metadata,
      }),
    [activeRequest, receipts, metadata],
  );

  const { target } = binding;
  const requestSwitch = React.useCallback(
    async (selection: string) => {
      const next = {
        commandId: createCodingSessionCommandId(),
        selection,
        targetKey,
      };
      setReceipts([]);
      setMetadata([]);
      setPublishError(null);
      setRequest(next);
      try {
        await publishCodingSessionModelSet({
          channelId,
          commandId: next.commandId,
          target,
          selection,
        });
      } catch (error) {
        setRequest((current) =>
          current?.commandId === next.commandId ? null : current,
        );
        setPublishError(
          `Not switched: ${error instanceof Error ? error.message : "the relay did not accept the request."}`,
        );
      }
    },
    [channelId, target, targetKey],
  );

  return { state, request: activeRequest, publishError, requestSwitch };
}

function splitSelection(
  selection: string,
  offer: CodingSessionModelOffer | null,
): Selection {
  return splitCodingSessionModelSelection(
    selection,
    offer ?? { allowedModels: [] },
  );
}

function detailName(
  offer: CodingSessionModelOffer,
  base: string,
): string | null {
  return (
    codingSessionProviderPickerModels(offer).details.get(base)?.name ?? null
  );
}

/** `Sonnet 5 · High · 1M`: a selection the way a person reads it. */
export function codingSessionSelectionLabel(
  selection: string,
  offer: CodingSessionModelOffer | null,
): string {
  const split = splitSelection(selection, offer);
  const name = codingSessionModelTitle(
    split.model,
    offer ? detailName(offer, split.model) : null,
  );
  const traits = codingSessionTraitsSummary({
    thinking: split.effort,
    context: split.context,
    fast: split.fast,
  });
  return traits ? `${name} · ${traits}` : name;
}
