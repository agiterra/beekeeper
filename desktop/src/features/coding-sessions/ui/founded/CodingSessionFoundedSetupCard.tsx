import { CircleAlert, LoaderCircle, Rocket, Trash2 } from "lucide-react";
import type { ReactNode } from "react";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { useDeleteCodingSessionDialog } from "../../hooks/useDeleteCodingSessionDialog";
import { MAX_CODING_SESSION_GOAL_BYTES } from "../../lib/codingSessionGoal";
import { codingSessionLaunchBlockersBySurface } from "../../lib/codingSessionLaunchForm";
import { MAX_CODING_SESSION_NAME_BYTES } from "../../lib/codingSessionName";
import {
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
} from "../../lib/newCodingSessionModel";
import { NewCodingSessionBenchField } from "../NewCodingSessionBenchField";
import {
  CodingSessionLaunchGoalNotes,
  CodingSessionLaunchSteps,
} from "../NewCodingSessionLaunchNotes";
import { NewCodingSessionLeadField } from "../NewCodingSessionLeadField";
import { NewCodingSessionPolicyField } from "../NewCodingSessionPolicyField";
import {
  NewCodingSessionModelDisclosure,
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "../NewCodingSessionProviderPicker";
import { NewCodingSessionReadiness } from "../NewCodingSessionReadiness";
import { NewCodingSessionRolesField } from "../NewCodingSessionRolesField";
import { CodingSessionFoundedModeSwitch } from "./CodingSessionFoundedModeSwitch";
import { CodingSessionFoundedWhereField } from "./CodingSessionFoundedWhereField";
import type { CodingSessionFoundedGoal } from "./CodingSessionFoundedWorkspace";
import {
  type CodingSessionFoundedSetupModel,
  useCodingSessionFoundedSetup,
} from "./useCodingSessionFoundedSetup";
import { useCodingSessionFoundedStart } from "./useCodingSessionFoundedStart";

/**
 * The setup card — the whole form, on the founded page: **Solo | Team** ·
 * Name · Initial prompt · [Team] who leads · runtime and model (with the
 * override reason) · [Team] bench · [Team] policy · [Team, project] roles ·
 * where it runs · steps · readiness · status · **Discard** · **Start**.
 * Presentational: everything it shows is the model from
 * `useCodingSessionFoundedSetup`, so a test can hand it a state and read
 * what the button and the sentence under it say.
 *
 * Every disabled Start has a sentence: the button reads the inline blockers,
 * rendered above it; the one blocker surfaced on press (a blank prompt) is
 * rendered under the prompt field once Start is pressed.
 */
export function CodingSessionFoundedSetupCard({
  channelId,
  onDiscard,
  onStart,
  projectRef,
  setup,
}: {
  channelId: string;
  /** Offered only while the session is founded and nothing is in flight. */
  onDiscard: (() => void) | null;
  onStart: () => void;
  projectRef: string | null;
  setup: CodingSessionFoundedSetupModel;
}) {
  const { text } = setup;
  const team = setup.mode === "team";
  const locked = setup.interactionLocked;
  const workdirEditable =
    !locked || isCodingSessionWorkdirFailure(setup.failureCode);
  const busy = setup.isLaunching || setup.isPublishing;
  // A blank prompt does not disable the button: it is the one blocker
  // surfaced by pressing Start, under the prompt field. Every other blocker
  // is inline and disables the button, so a disabled control never goes
  // silent.
  const surfaces = codingSessionLaunchBlockersBySurface(setup.readiness);
  const inlineReadiness = { ...setup.readiness, blockers: surfaces.inline };
  const promptBlocker =
    surfaces.onAttempt.find((blocker) => blocker.id === "goal") ?? null;
  const leadBlocker =
    surfaces.onAttempt.find((blocker) => blocker.id === "lead") ?? null;
  const worktreeBlocker =
    surfaces.onAttempt.find((blocker) => blocker.id === "worktree-name") ??
    null;
  return (
    <section
      aria-labelledby="coding-session-founded-setup-title"
      className="flex flex-col gap-5 rounded-lg border border-border/60 bg-card p-4"
      data-testid="coding-session-founded-setup-card"
    >
      <div className="flex flex-col gap-1">
        <h2
          className="text-sm font-semibold"
          id="coding-session-founded-setup-title"
        >
          Set up this session
        </h2>
        <p className="text-2xs text-muted-foreground">
          {setup.channelReader !== "resolved"
            ? "Founded. Nothing runs until Start."
            : projectRef
              ? "Founded, in its channel's project. Nothing runs until Start."
              : "Founded. It belongs to no project — it lives in this channel. Nothing runs until Start."}
        </p>
      </div>

      <CodingSessionFoundedModeSwitch
        disabled={locked}
        mode={setup.mode}
        onModeChange={setup.setMode}
      />

      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-founded-prompt"
        >
          Initial prompt
        </label>
        <Textarea
          className="min-h-32"
          data-testid="coding-session-founded-prompt"
          disabled={locked}
          id="coding-session-founded-prompt"
          maxLength={MAX_CODING_SESSION_GOAL_BYTES}
          onBlur={() => {
            void text.commitPrompt();
            text.requestSuggestionNow();
          }}
          onChange={(event) => text.setPrompt(event.target.value)}
          onKeyDown={text.onPromptKeyDown}
          placeholder="What is this session for?"
          value={text.prompt}
        />
        <CodingSessionLaunchGoalNotes
          bytes={new TextEncoder().encode(text.prompt.trim()).byteLength}
          goalOutcome={null}
          overflow={text.goalOverflow}
        />
        {text.promptAttempted && promptBlocker ? (
          <FieldError testId="new-coding-session-blocker-goal">
            {promptBlocker.sentence}
          </FieldError>
        ) : null}
        {text.promptError ? (
          <FieldError testId="coding-session-founded-prompt-error">
            {text.promptError}
          </FieldError>
        ) : null}
        {!team && text.autoGoalSentence ? (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="coding-session-founded-goal-auto"
          >
            {text.autoGoalSentence}
          </p>
        ) : null}
        {text.busySentence ? (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="coding-session-founded-text-status"
            role="status"
          >
            {text.busySentence}
          </p>
        ) : null}
        {text.persistence.message ? (
          <p className="text-2xs text-muted-foreground">
            {text.persistence.message}
          </p>
        ) : null}
      </div>

      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-founded-name"
        >
          Name (optional)
        </label>
        <Input
          data-testid="coding-session-founded-name"
          disabled={locked || text.nameReadPending || !!text.nameReadError}
          aria-describedby={
            text.nameReadPending || text.nameReadError
              ? "coding-session-founded-name-read-status"
              : undefined
          }
          id="coding-session-founded-name"
          maxLength={MAX_CODING_SESSION_NAME_BYTES}
          onBlur={() => void text.commitName()}
          onChange={(event) => text.setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void text.commitName();
            }
          }}
          placeholder="Short name for this session"
          value={text.name}
        />
        {text.nameReadPending || text.nameReadError ? (
          <p
            id="coding-session-founded-name-read-status"
            data-testid="coding-session-founded-name-read-status"
            className="text-2xs text-muted-foreground"
            role="status"
          >
            {text.nameReadError
              ? `The existing session name could not be checked: ${text.nameReadError} Your draft is preserved.`
              : "Reading the existing session name… Your draft is preserved; the name is not editable yet."}
            {text.nameReadError && text.refreshNames ? (
              <button
                type="button"
                className="ml-2 text-primary underline"
                onClick={text.refreshNames}
              >
                Retry name read
              </button>
            ) : null}
          </p>
        ) : null}
        {text.nameError ? (
          <FieldError testId="coding-session-founded-name-error">
            {text.nameError}
          </FieldError>
        ) : null}
        {text.suggestion?.message ? (
          <p
            className={cn(
              "text-2xs",
              text.suggestion.state === "failed"
                ? "text-destructive"
                : "text-muted-foreground",
            )}
            data-testid="coding-session-founded-name-suggestion"
          >
            {text.suggestion.message}
          </p>
        ) : null}
        {text.name.trim().length === 0 && text.autoNameSentence ? (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="coding-session-founded-name-auto"
          >
            {text.autoNameSentence}
          </p>
        ) : null}
      </div>

      {team ? (
        <div className="flex flex-col gap-2">
          <NewCodingSessionLeadField
            candidates={setup.candidates}
            disabled={locked}
            groups={setup.leadGroups}
            lead={setup.lead}
            onLeadChange={setup.setLeadActor}
          />
          {setup.attempted && leadBlocker ? (
            <FieldError testId="new-coding-session-blocker-lead">
              {leadBlocker.sentence}
            </FieldError>
          ) : null}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        <NewCodingSessionProviderPicker
          disabled={locked}
          model={setup.effectiveModel}
          onLoginLaunched={({ runtime }) => setup.beginLoginWatch(runtime)}
          onModelChange={setup.selectModel}
          onTargetChange={setup.selectTarget}
          selectedTarget={setup.selectedTarget}
          targets={setup.targets}
        />
        <NewCodingSessionModelDisclosure
          catalog={setup.modelCatalog}
          model={setup.effectiveModel}
          note={setup.seatedModelNote}
        />
        {team && setup.modelOverridden && setup.lead.kind === "agent" ? (
          <div className="flex flex-col gap-1">
            <label
              className="text-2xs text-muted-foreground"
              htmlFor="coding-session-founded-model-override"
            >
              Why this model, and not the one {setup.lead.label} carries?
            </label>
            <Input
              data-testid="coding-session-founded-model-override"
              disabled={locked}
              id="coding-session-founded-model-override"
              onChange={(event) => setup.setOverrideReason(event.target.value)}
              placeholder="Because…"
              value={setup.overrideReason}
            />
          </div>
        ) : null}
      </div>

      {team ? (
        <NewCodingSessionBenchField
          challengerRate={setup.challengerRate}
          disabled={locked}
          identities={setup.benchIdentityOptions}
          onChallengerRateChange={setup.setChallengerRate}
          onToggleIdentity={setup.toggleBenchIdentity}
          onToggleProvider={setup.toggleBenchProvider}
          providers={setup.benchProviderOptions}
          selectedIdentities={setup.benchIdentities}
          selectedProviders={setup.benchProviders}
        />
      ) : null}

      {team ? (
        <NewCodingSessionPolicyField
          disabled={locked}
          draft={setup.policyDraft}
          onDraftChange={setup.setPolicyDraft}
        />
      ) : null}

      {team && projectRef ? (
        <NewCodingSessionRolesField
          disabled={locked}
          launchRoles={setup.launchRoles}
          onUseRolesChange={setup.setUseRoles}
          runtimeTarget={setup.selectedTarget}
          teamReadiness={setup.teamReadiness}
          useRoles={setup.useRoles}
        />
      ) : null}

      <CodingSessionFoundedWhereField
        channelId={channelId}
        disabled={!workdirEditable}
        draftSource={setup.draftSource}
        governed={team}
        projectRef={projectRef}
        sessionName={setup.worktreeSeed}
        setUseWorktree={setup.setUseWorktree}
        setWorkdir={setup.setWorkdir}
        setWorktreeName={setup.setWorktreeName}
        setWorktreeSource={setup.setWorktreeSource}
        useWorktree={setup.useWorktree}
        workdir={setup.workdir}
        workspaceReuse={setup.workspaceReuse}
        worktreeName={setup.worktreeName}
        worktreeSource={setup.worktreeSource}
      />
      {setup.attempted && worktreeBlocker ? (
        <FieldError testId="new-coding-session-blocker-worktree-name">
          {worktreeBlocker.sentence}
        </FieldError>
      ) : null}

      {setup.steps.length > 0 ? (
        <CodingSessionLaunchSteps steps={setup.steps} />
      ) : null}

      {/* Solo signs one create; its blockers are the whole story (Andy,
          2026-09-10). A Team Start signs a policy, a seat, grants and a
          turn, and names what it will not check — it keeps both. */}
      <NewCodingSessionReadiness
        plan={team && setup.attempted ? setup.plan : []}
        readiness={inlineReadiness}
        unknownsDisclosed={team}
      />

      {setup.status ? (
        <p
          className={cn(
            "flex items-start gap-2 text-sm",
            setup.status.tone === "destructive"
              ? "text-destructive"
              : "text-muted-foreground",
          )}
          data-testid="coding-session-founded-status"
          role="status"
        >
          {setup.status.tone === "destructive" ? (
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
          ) : (
            <LoaderCircle className="mt-0.5 size-4 shrink-0 animate-spin motion-reduce:animate-none" />
          )}
          {setup.status.message}
        </p>
      ) : null}

      {isCodingSessionAuthFailure(setup.failureCode) ? (
        <ProviderLoginNeeded
          onLoginLaunched={({ runtime }) => setup.beginLoginWatch(runtime)}
          runtime={
            setup.selectedTarget
              ? {
                  runtime: setup.selectedTarget.provider.runtime,
                  label: setup.selectedTarget.availability?.label,
                }
              : null
          }
        />
      ) : null}

      <div className="flex flex-wrap items-center justify-end gap-2">
        {[setup.setupError, setup.launchError].map((message) =>
          message ? (
            <p
              className="flex max-h-32 basis-full items-start gap-2 overflow-y-auto break-words text-sm text-destructive"
              data-testid="coding-session-founded-error"
              key={message}
              role="alert"
            >
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              {message}
            </p>
          ) : null,
        )}
        {setup.transaction !== null && setup.lifecycleState === "failed" ? (
          // A failed create would otherwise hold the busy blocker forever;
          // discarding it is the one way this card offers out, and it is
          // named as what it is.
          <>
            {setup.startFreshReadiness.reason ? (
              <p
                id="coding-session-founded-discard-attempt-status"
                data-testid="coding-session-founded-discard-attempt-status"
                className="basis-full text-2xs text-muted-foreground"
                role="status"
              >
                {setup.startFreshReadiness.reason}
              </p>
            ) : null}
            <Button
              data-testid="coding-session-founded-discard-attempt"
              disabled={!setup.startFreshReadiness.allowed}
              aria-describedby={
                setup.startFreshReadiness.reason
                  ? "coding-session-founded-discard-attempt-status"
                  : undefined
              }
              onClick={setup.startFresh}
              type="button"
              variant="outline"
            >
              Discard the failed attempt
            </Button>
          </>
        ) : null}
        {onDiscard ? (
          // A click founded this session; an abandoned click is a row on
          // every device until it is closed. This is the existing close
          // flow, with its confirm dialog — nothing is deleted.
          <Button
            data-testid="coding-session-founded-discard"
            onClick={onDiscard}
            type="button"
            variant="outline"
          >
            <Trash2 />
            Discard session
          </Button>
        ) : null}
        <Button
          data-testid="coding-session-founded-start"
          disabled={!surfaces.canPress}
          onClick={onStart}
          type="button"
        >
          {busy ? (
            <LoaderCircle className="animate-spin motion-reduce:animate-none" />
          ) : (
            <Rocket />
          )}
          Start
        </Button>
      </div>
    </section>
  );
}

function FieldError({
  children,
  testId,
}: {
  children: ReactNode;
  testId: string;
}) {
  return (
    <p
      className="flex items-start gap-2 text-sm text-destructive"
      data-testid={testId}
      role="alert"
    >
      <CircleAlert className="mt-0.5 size-4 shrink-0" />
      {children}
    </p>
  );
}

/**
 * The setup card, wired: the state hook, the Start hook and the Discard
 * confirm around the card. Mounted for the founder only — the create hook
 * it holds provisions this computer's provider on mount, and the text hook
 * publishes under the founder's key; neither is anything a non-founder asked
 * for.
 */
export function CodingSessionFoundedSetupHost({
  channelId,
  channelReader,
  founderPubkey,
  genesisRef,
  goal,
  nameResolved,
  nameReadError = null,
  refreshNames,
  onCreated,
  projectRef,
  sessionRef,
  starting = false,
  wireName,
}: {
  channelId: string;
  /** Whether the channel record (and so its project) has been read. */
  channelReader: "loading" | "errored" | "resolved";
  founderPubkey: string;
  genesisRef: string;
  /** The goal as the page could read it. */
  goal: CodingSessionFoundedGoal;
  /** Whether the names read has settled once for this channel. */
  nameResolved: boolean;
  /** Failed or partial history cannot authorize replacing a name. */
  nameReadError?: string | null;
  /** Retry the name read without replacing the draft. */
  refreshNames?: () => void;
  onCreated: (input: { channelId: string; generationId: string }) => void;
  projectRef: string | null;
  sessionRef: string;
  /** A receipt-joined create already claims the umbrella; Start is held. */
  starting?: boolean;
  /** The founder-keyed 44229, or null when none is on the wire. */
  wireName: string | null;
}) {
  const { goCodingSession } = useAppNavigation();
  const setup = useCodingSessionFoundedSetup({
    channelId,
    sessionRef,
    projectRef,
    founderPubkey,
    goal,
    wireName,
    nameResolved,
    nameReadError,
    refreshNames,
    channelReader,
    starting,
    onCreated,
  });
  const onStart = useCodingSessionFoundedStart({
    channelId,
    sessionRef,
    genesisRef,
    founderPubkey,
    projectRef,
    setup,
    goCodingSession,
  });
  const { goChannel } = useAppNavigation();
  // Discard deletes. A founded session that never ran has no transcript and
  // nothing worth a closed record: closing it filed an "Untitled session"
  // under Settled for good (Andy, 2026-09-11). The delete is the existing
  // whole-session kind:5 (genesis, prompt, name) through the existing
  // confirm dialog, in its never-started voice; once the relay accepts it
  // the page leaves for the channel, and every store forgets the session.
  const deletion = useDeleteCodingSessionDialog(() => {
    void goChannel(channelId);
  });
  const onDiscard =
    starting || setup.interactionLocked
      ? null
      : () =>
          deletion.requestDelete({
            channelId,
            genesisRef,
            sessionRef,
            label: setup.text.name.trim() || "Untitled session",
            stops: [],
            neverStarted: true,
          });
  return (
    <>
      <CodingSessionFoundedSetupCard
        channelId={channelId}
        onDiscard={onDiscard}
        onStart={onStart}
        projectRef={projectRef}
        setup={setup}
      />
      {deletion.dialog}
    </>
  );
}

/**
 * What a member who did not found the session sees in the card's place.
 *
 * Nothing about the setup is on the wire before Start — Solo or Team, the
 * lead, the runtime, the bench, the policy and where it runs are decided on
 * the founder's screen and published by their Start — so there is nothing to
 * render read-only except that fact. Saying so beats a greyed-out form full
 * of this computer's defaults, which would read as the founder's choices.
 */
export function CodingSessionFoundedSetupReadOnly({
  founderName,
  starting = false,
}: {
  founderName: ReactNode;
  /** A receipt-joined create already claims the umbrella. */
  starting?: boolean;
}) {
  return (
    <section
      className="flex flex-col gap-2 rounded-lg border border-border/60 bg-card p-4"
      data-testid="coding-session-founded-setup-readonly"
    >
      <h2 className="text-sm font-semibold">Set up this session</h2>
      <p className="text-sm text-muted-foreground">
        {starting ? (
          <>
            A create for this session is on the relay; its first report is still
            to come.
          </>
        ) : (
          <>
            Not set up yet. Solo or team, the runtime and where it runs are
            picked by {founderName} when they start it; nothing about that is on
            the relay before then.
          </>
        )}
      </p>
    </section>
  );
}
