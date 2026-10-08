import * as React from "react";
import { ExternalLink, PictureInPicture2 } from "lucide-react";

import {
  type SessionPreviewState,
  sessionPreviewBack,
  sessionPreviewClose,
  sessionPreviewForward,
  sessionPreviewNavigate,
  sessionPreviewPopout,
  sessionPreviewReload,
  toSessionPreviewError,
} from "@/shared/api/tauriSessionPreview";
import { Button } from "@/shared/ui/button";

import { useSessionPreviewServers } from "../hooks/useSessionPreviewServers";
import { useSessionPreviewSlot } from "../hooks/useSessionPreviewSlot";
import { useSessionPreviewState } from "../hooks/useSessionPreviewState";
import {
  defaultPreviewFloatBox,
  type PreviewFloatBox,
} from "../lib/previewFloatGeometry";
import {
  type SessionPreviewBindingOption,
  SESSION_PREVIEW_CLOSED_LABEL,
  SESSION_PREVIEW_FLOATING_LABEL,
  SESSION_PREVIEW_POPPED_LABEL,
  SESSION_PREVIEW_UNBOUND_LABEL,
  sessionPreviewBinding,
  sessionPreviewDisplayUrl,
  sessionPreviewDriverName,
  sessionPreviewDrivingText,
  sessionPreviewNormalizeUrl,
} from "../lib/previewModel";
import {
  readSessionPreviewRecents,
  rememberSessionPreviewRecent,
} from "../lib/previewRecents";
import { SessionPreviewEmptyState } from "./SessionPreviewEmptyState";
import { SessionPreviewFloating } from "./SessionPreviewFloating";
import { SessionPreviewSlot } from "./SessionPreviewSlot";
import { SessionPreviewStrip } from "./SessionPreviewStrip";
import { SessionPreviewToolbar } from "./SessionPreviewToolbar";

export type SessionPreviewSurfaceProps = {
  channelId: string;
  /** The session's executions that name a target, labelled for people. */
  options: readonly SessionPreviewBindingOption[];
  /** The execution the view is focused on, when one resolves. */
  focusedExecutionKey: string | null;
  /**
   * This machine's provider runs the focused execution. `false` is said in
   * the strip: that agent cannot reach this computer's Browser.
   */
  isLocalProvider: boolean | null;
};

function Notice({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-xs text-muted-foreground"
      data-testid="session-preview-notice"
    >
      {children}
    </div>
  );
}

/**
 * The Browser surface (SV-33 S1/S2): a local page beside the session, docked
 * in the panel, floating over the transcript, or popped out to its own
 * window. The page is a native view on this computer; this component only
 * says where it goes and what it is. Every state it shows is Rust's last
 * answer, never a guess.
 */
export function SessionPreviewSurface({
  channelId,
  focusedExecutionKey,
  isLocalProvider,
  options,
}: SessionPreviewSurfaceProps) {
  const { state, error, drivingEvent, apply } =
    useSessionPreviewState(channelId);
  const [chosen, setChosen] = React.useState<string | null>(null);
  const [refusal, setRefusal] = React.useState<string | null>(null);
  const [floatBox, setFloatBox] = React.useState<PreviewFloatBox | null>(null);
  const [recents, setRecents] = React.useState(readSessionPreviewRecents);

  const binding = sessionPreviewBinding({
    options,
    focusedExecutionKey,
    chosenExecutionKey: chosen,
  });
  const open = state?.status === "loading" || state?.status === "ready";
  // The slot is reported whenever this surface is on screen, not only while
  // docked: Rust needs a slot to dock into before the first page opens (a
  // person's navigate with no slot would pop out), to auto-dock an agent's
  // pop-out once the surface mounts (WIRE-C4 § 9.9), and for "Bring it
  // back". While popped out or empty, the slot is the panel's body; the
  // native view is only drawn there when Rust places it there.
  const slot = useSessionPreviewSlot({
    channelId,
    enabled: state !== null && state.status !== "unavailable",
  });
  const servers = useSessionPreviewServers(!open && state !== null);

  const run = React.useCallback(
    (action: () => Promise<SessionPreviewState>) => {
      action()
        .then((next) => {
          apply(next);
          setRefusal(null);
        })
        .catch((reason) => setRefusal(toSessionPreviewError(reason).message));
    },
    [apply],
  );

  const navigate = React.useCallback(
    (raw: string, title: string | null = null) => {
      if (binding.kind === "choose") return;
      const url = sessionPreviewNormalizeUrl(raw);
      const target = binding.kind === "bound" ? binding.option.target : null;
      sessionPreviewNavigate({ channelId, url, target })
        .then((next) => {
          apply(next);
          setRefusal(null);
          setRecents(
            rememberSessionPreviewRecent({
              url: next.url ?? url,
              title: next.title ?? title,
              at: Date.now(),
            }),
          );
        })
        .catch((reason) => setRefusal(toSessionPreviewError(reason).message));
    },
    [apply, binding, channelId],
  );

  if (error && !state) return <Notice>{error}</Notice>;
  if (!state) return <Notice>Asking this computer's Browser…</Notice>;
  if (state.status === "unavailable") {
    return (
      <Notice>
        {state.unavailable?.sentence ??
          "The Browser could not start a web view on this computer."}
      </Notice>
    );
  }

  const driving =
    state.driving ??
    (drivingEvent?.driving
      ? {
          executionId: drivingEvent.executionId,
          sessionId: drivingEvent.sessionId,
        }
      : null);
  const drivingText = driving
    ? sessionPreviewDrivingText(sessionPreviewDriverName(driving, options))
    : null;
  const bindingNote =
    binding.kind === "none"
      ? SESSION_PREVIEW_UNBOUND_LABEL
      : isLocalProvider === false
        ? "This session's agent runs on another computer and cannot drive this Browser."
        : binding.kind === "bound" && options.length > 1
          ? `Only ${binding.option.label} may drive it.`
          : null;
  const strip = (
    <SessionPreviewStrip bindingNote={bindingNote} drivingText={drivingText} />
  );

  if (!open) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col"
        data-testid="session-preview-surface"
        data-status={state.status}
      >
        <div className="flex min-h-0 flex-1 flex-col" ref={slot.ref}>
          <SessionPreviewEmptyState
            binding={binding}
            closedNote={
              state.status === "closed_by_person"
                ? SESSION_PREVIEW_CLOSED_LABEL
                : null
            }
            onChoose={setChosen}
            onOpen={navigate}
            recents={recents}
            servers={servers.servers}
            serversError={servers.error}
            serversLoading={servers.loading}
          />
        </div>
        {refusal ? (
          <p
            className="px-4 pb-2 text-2xs text-destructive"
            data-testid="session-preview-refusal"
            role="alert"
          >
            {refusal}
          </p>
        ) : null}
        {strip}
      </div>
    );
  }

  const floating = floatBox !== null && state.placement !== "popped_out";
  const toolbar = (
    <SessionPreviewToolbar
      actions={{
        onBack: () => run(() => sessionPreviewBack(channelId)),
        onForward: () => run(() => sessionPreviewForward(channelId)),
        onReload: () => run(() => sessionPreviewReload(channelId)),
        onNavigate: (raw) => navigate(raw),
        onPopout: () => {
          setFloatBox(null);
          run(() => sessionPreviewPopout(channelId, true));
        },
        onToggleFloat: () =>
          setFloatBox((box) =>
            box
              ? null
              : defaultPreviewFloatBox({
                  width: window.innerWidth,
                  height: window.innerHeight,
                }),
          ),
        onClose: () => {
          setFloatBox(null);
          run(() => sessionPreviewClose(channelId));
        },
      }}
      floating={floating}
      refusal={refusal}
      state={state}
    />
  );
  const live = <SessionPreviewSlot control={slot} state={state} />;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-placement={
        state.placement === "popped_out"
          ? "popped_out"
          : floating
            ? "floating"
            : "docked"
      }
      data-status={state.status}
      data-testid="session-preview-surface"
    >
      {state.placement === "popped_out" ? (
        <>
          {toolbar}
          <div className="flex min-h-0 flex-1 flex-col" ref={slot.ref}>
            <Notice>
              <div className="flex flex-col items-center gap-2">
                <ExternalLink aria-hidden className="size-5" />
                <p data-testid="session-preview-popped">
                  {SESSION_PREVIEW_POPPED_LABEL}
                </p>
                <Button
                  data-testid="session-preview-bring-back"
                  onClick={() =>
                    run(() => sessionPreviewPopout(channelId, false))
                  }
                  size="xs"
                  variant="outline"
                >
                  Bring it back
                </Button>
              </div>
            </Notice>
          </div>
          {strip}
        </>
      ) : floating && floatBox ? (
        <>
          <Notice>
            <div className="flex flex-col items-center gap-2">
              <PictureInPicture2 aria-hidden className="size-5" />
              <p>{SESSION_PREVIEW_FLOATING_LABEL}</p>
              <Button
                data-testid="session-preview-dock-panel"
                onClick={() => setFloatBox(null)}
                size="xs"
                variant="outline"
              >
                Dock it here
              </Button>
            </div>
          </Notice>
          <SessionPreviewFloating
            box={floatBox}
            onBox={setFloatBox}
            onClose={() => {
              setFloatBox(null);
              run(() => sessionPreviewClose(channelId));
            }}
            onDock={() => setFloatBox(null)}
            onMoved={slot.remeasure}
            title={
              state.title ??
              (state.url ? sessionPreviewDisplayUrl(state.url) : "Browser")
            }
          >
            {toolbar}
            {live}
            {strip}
          </SessionPreviewFloating>
        </>
      ) : (
        <>
          {toolbar}
          {live}
          {strip}
        </>
      )}
    </div>
  );
}
