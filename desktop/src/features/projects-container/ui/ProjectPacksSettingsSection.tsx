import { getVersion } from "@tauri-apps/api/app";
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { managedAgentsQueryKey } from "@/features/agents/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

import type { ProjectContainer } from "../hooks";
import {
  canSetProjectPackSource,
  fetchProjectPackSource,
  projectPackSourceQueryKey,
  publishProjectPackSource,
  type ProjectPackSource,
} from "../lib/projectPackSource";
import {
  describeAgentsSetup,
  projectAgentsInit,
  type ProjectAgentsInitResult,
} from "../lib/projectAgentsInit";
import {
  defaultPacksRepoId,
  describeSeedOutcome,
  packsRepoIdError,
  projectPacksInit,
  type ProjectPacksInitResult,
} from "../lib/projectPacksInit";
import { useProjectCapabilities } from "../lib/projectPermissions";
import { useProjectRosterQuery } from "../lib/projectMembers";

export { projectPackSourceQueryKey };

/**
 * This app's own version, the way the "shipped defaults" disclosure names
 * it — `getVersion()` is the same Tauri API `SettingsView.tsx` already reads
 * it from, so this surface and the About screen can never disagree about
 * which build a viewer is running. `null` until the async read settles, or
 * on a webview where the Tauri API is unavailable (never invented).
 */
function useAppVersion(): string | null {
  const [version, setVersion] = React.useState<string | null>(null);
  React.useEffect(() => {
    let cancelled = false;
    void getVersion()
      .then((value) => {
        if (!cancelled) setVersion(value);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return version;
}

/**
 * "Packs" — the project settings row showing which git repository (and
 * pinned commit) this project's coding-session seats stage their persona
 * packs from (LANE-L23), and, for a founder or the project owner, the two
 * actions the "setup lives inside the app" addendum (2026-09-03) asks for:
 * "Create packs repository" (announce, seed from the shipped packs, and
 * publish the source — one host call) and "Use an existing repository"
 * (pick or paste a coordinate and publish the source directly).
 *
 * A project with no `30624` source is no longer "no pack": the app bundles
 * the seven role packs at build time and stages them as the fallback, so the
 * empty state names that fallback — "shipped defaults vX (app <version>)" —
 * rather than reading as if nothing were staged at all.
 *
 * The relay is the actual gate (author must be a founder of one of the
 * project's repositories, or the project owner); this surface's own check
 * ({@link canSetProjectPackSource}) is advisory and deliberately the *safe*
 * subset — a project owner is always one of the two allowed arms, so gating
 * the actions on that arm alone never shows a control the relay would
 * refuse. It can, however, hide the actions from a non-owner repository
 * founder this client has no way to identify without a native command (no
 * different from `codingSessionMissionLand.ts`'s own `viewerIsFounder`,
 * which needed exactly that) — disclosed here, not silently narrowed.
 */
export function ProjectPacksSettingsSection({
  project,
}: {
  project: ProjectContainer;
}) {
  const queryClient = useQueryClient();
  const identityQuery = useIdentityQuery();
  const rosterQuery = useProjectRosterQuery(project);
  const capabilities = useProjectCapabilities(project);
  const appVersion = useAppVersion();
  const sourceQuery = useQuery({
    queryKey: projectPackSourceQueryKey(project.address),
    queryFn: () => fetchProjectPackSource(project.address),
  });

  const self = identityQuery.data?.pubkey ?? null;
  const roster = rosterQuery.data ?? project.members;
  // The safe subset of the wire rule — see the module doc above.
  const canSet =
    capabilities.isOwner ||
    canSetProjectPackSource({ self, project, roster, repo: null });

  const [activeAction, setActiveAction] = React.useState<
    "none" | "agents" | "create" | "use-existing"
  >("none");
  const invalidateSource = () =>
    void queryClient.invalidateQueries({
      queryKey: projectPackSourceQueryKey(project.address),
    });

  return (
    <div className="flex flex-col gap-4" data-testid="project-packs-section">
      <p className="text-xs text-muted-foreground">
        Which git repository this project's coding-session seats stage their
        persona packs from. Without a source, seats stage the app&apos;s own
        shipped packs.
      </p>

      <ProjectPackSourceRow
        appVersion={appVersion}
        loading={sourceQuery.isLoading}
        source={sourceQuery.data ?? null}
      />

      {canSet ? (
        <div className="flex flex-col gap-3">
          {activeAction === "none" ? (
            <div className="flex flex-wrap gap-2">
              <Button
                className="self-start"
                data-testid="project-agents-init-open"
                onClick={() => setActiveAction("agents")}
                size="sm"
                variant="outline"
              >
                {sourceQuery.data
                  ? "Finish repository setup"
                  : "Create the project's repositories"}
              </Button>
              <Button
                className="self-start"
                data-testid="project-packs-create-repo-open"
                onClick={() => setActiveAction("create")}
                size="sm"
                variant="outline"
              >
                Create packs repository
              </Button>
              <Button
                className="self-start"
                data-testid="project-packs-use-existing-open"
                onClick={() => setActiveAction("use-existing")}
                size="sm"
                variant="outline"
              >
                Use an existing repository
              </Button>
            </div>
          ) : null}
          {activeAction === "agents" ? (
            <ProjectAgentsInitAction
              onCancel={() => setActiveAction("none")}
              onRan={() => {
                invalidateSource();
                // Finish setup may have installed the project's agents.
                void queryClient.invalidateQueries({
                  queryKey: managedAgentsQueryKey,
                });
              }}
              projectRef={project.address}
            />
          ) : null}
          {activeAction === "create" ? (
            <ProjectPacksCreateRepoAction
              onCancel={() => setActiveAction("none")}
              onCreated={() => {
                invalidateSource();
              }}
              projectRef={project.address}
              projectSlug={project.dtag}
            />
          ) : null}
          {activeAction === "use-existing" ? (
            <ProjectPackSourceForm
              existingRepoAddrs={project.repoAddrs}
              onCancel={() => setActiveAction("none")}
              onSaved={() => {
                setActiveAction("none");
                invalidateSource();
              }}
              projectCoord={project.address}
            />
          ) : null}
        </div>
      ) : (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-packs-readonly-note"
        >
          Only this project&apos;s repository founders or its owner can set the
          pack source.
        </p>
      )}
    </div>
  );
}

function ProjectPackSourceRow({
  appVersion,
  loading,
  source,
}: {
  appVersion: string | null;
  loading: boolean;
  source: ProjectPackSource | null;
}) {
  if (loading) {
    return <p className="text-xs text-muted-foreground">Reading the source…</p>;
  }
  if (source === null) {
    return (
      <p className="text-sm" data-testid="project-packs-source-shipped">
        {appVersion
          ? `shipped defaults v${appVersion} (app ${appVersion})`
          : "shipped defaults"}
        {
          " — no project source is set, so seats stage the packs built into this app."
        }
      </p>
    );
  }
  const pin = source.sha ? `sha ${source.sha.slice(0, 8)}` : source.ref;
  return (
    <dl
      className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-1 text-sm"
      data-testid="project-packs-source-row"
    >
      <dt className="text-muted-foreground">Repository</dt>
      <dd className="truncate font-mono text-xs">{source.repo}</dd>
      <dt className="text-muted-foreground">Pinned to</dt>
      <dd className="truncate font-mono text-xs">{pin}</dd>
      <dt className="text-muted-foreground">Path</dt>
      <dd className="truncate font-mono text-xs">{source.path}</dd>
      <dt className="text-muted-foreground">Set by</dt>
      <dd className="truncate text-xs">
        {truncatePubkey(source.author)} on{" "}
        {new Date(source.createdAt * 1000).toLocaleString()}
      </dd>
      {source.note ? (
        <>
          <dt className="text-muted-foreground">Note</dt>
          <dd className="text-xs">{source.note}</dd>
        </>
      ) : null}
    </dl>
  );
}

/**
 * "Create the project's repositories" / "Finish repository setup" — runs
 * `project_agents_init` (spec § 4.11): announce `<slug>` and
 * `<slug>-beekeeper-agents`, seed the latter from this build's shipped role
 * templates by reference, push `main`, set the source. Idempotent, so the
 * same button finishes a create that stopped part-way. The host's own
 * `complete`/`gap` verdict is what this panel prints.
 */
function ProjectAgentsInitAction({
  onCancel,
  onRan,
  projectRef,
}: {
  onCancel: () => void;
  onRan: () => void;
  projectRef: string;
}) {
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [result, setResult] = React.useState<ProjectAgentsInitResult | null>(
    null,
  );

  async function handleRun() {
    setPending(true);
    setError(null);
    try {
      const ran = await projectAgentsInit({ projectRef });
      setResult(ran);
      onRan();
    } catch (thrown) {
      setError(
        thrown instanceof Error
          ? thrown.message
          : "Failed to create the project's repositories.",
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-border/60 p-3"
      data-testid="project-agents-init-panel"
    >
      <p className="text-xs text-muted-foreground">
        Announces the code repository and the agents repository under your key,
        seeds the agents repository from this app&apos;s shipped role templates
        by reference (roles/, plans/, each with an archive/), pushes main, and
        sets it as this project&apos;s role source. What already exists is
        reused; what is missing is created.
      </p>
      {result ? (
        <div
          className="flex flex-col gap-1 text-xs"
          data-testid="project-agents-init-result"
        >
          <p className={result.complete ? "" : "text-destructive"}>
            {describeAgentsSetup(result)}
          </p>
          <dl className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-0.5 font-mono text-2xs">
            <dt className="text-muted-foreground">code</dt>
            <dd className="truncate">{result.codeRepoRef}</dd>
            <dt className="text-muted-foreground">agents</dt>
            <dd className="truncate">{result.agentsRepoRef}</dd>
            <dt className="text-muted-foreground">seed</dt>
            <dd className="truncate">
              {result.seedCommitSha
                ? `${result.seedCommitSha.slice(0, 8)} (${result.roles.join(", ")})`
                : result.seedSkipped
                  ? "already on the relay"
                  : (result.seedError ?? result.pushError ?? "not reached")}
            </dd>
            <dt className="text-muted-foreground">source</dt>
            <dd className="truncate">
              {result.sourceEventId
                ? result.sourceEventId.slice(0, 8)
                : result.sourceExisted
                  ? "already set"
                  : (result.publicationError ?? "not set")}
            </dd>
            <dt className="text-muted-foreground">agents</dt>
            <dd className="truncate" data-testid="project-agents-init-agents">
              {result.agentsInstalled.length > 0
                ? result.agentsInstalled
                    .map((agent) => `${agent.name} (${agent.role})`)
                    .join(", ")
                : (result.agentsError ?? "none installed")}
            </dd>
          </dl>
        </div>
      ) : null}
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-agents-init-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      <div className="flex gap-2">
        <Button
          data-testid="project-agents-init-run"
          disabled={pending}
          onClick={() => void handleRun()}
          size="sm"
        >
          {pending ? "Running…" : result ? "Run again" : "Run"}
        </Button>
        <Button disabled={pending} onClick={onCancel} size="sm" variant="ghost">
          {result ? "Close" : "Cancel"}
        </Button>
      </div>
    </div>
  );
}

/**
 * "Create packs repository" — calls Lane B's `project_packs_init` host
 * command (announce, seed, publish, all one step) and prints every wire
 * fact it produced, per the addendum's own words: "every step reports the
 * wire fact it produced (event ids, the push record)".
 *
 * LANE-L30 (2026-09-03): the repository id and name are no longer implied —
 * "one packs repository for all of agiterra; every project points at it"
 * means the person creating it must be able to name the *shared* repository
 * rather than get a fresh `<project-slug>-packs` every time. Both fields
 * start from sensible defaults ({@link defaultPacksRepoId}, and the name
 * defaulting to whatever the id is) and stay editable; the id is validated
 * with {@link packsRepoIdError} before the button will submit, the same rule
 * the CLI's `bee repos create --id` applies.
 */
function ProjectPacksCreateRepoAction({
  onCancel,
  onCreated,
  projectRef,
  projectSlug,
}: {
  onCancel: () => void;
  onCreated: () => void;
  projectRef: string;
  projectSlug: string;
}) {
  const [repoId, setRepoId] = React.useState(() =>
    defaultPacksRepoId(projectSlug),
  );
  // The name field shows the id until the viewer types into it directly —
  // after that, it is theirs, and a later edit to the id must not clobber it.
  const [nameTouched, setNameTouched] = React.useState(false);
  const [manualName, setManualName] = React.useState("");
  const name = nameTouched ? manualName : repoId;
  const repoIdError = packsRepoIdError(repoId);

  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [result, setResult] = React.useState<ProjectPacksInitResult | null>(
    null,
  );

  async function handleCreate() {
    if (repoIdError !== null) return;
    setPending(true);
    setError(null);
    try {
      const created = await projectPacksInit({
        name: name.trim() || repoId,
        projectRef,
        repoId,
      });
      setResult(created);
      onCreated();
    } catch (thrown) {
      setError(
        thrown instanceof Error
          ? thrown.message
          : "Failed to create the packs repository.",
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-border/60 p-3"
      data-testid="project-packs-create-repo-panel"
    >
      <p className="text-xs text-muted-foreground">
        Announces a new repository under your key, seeds it from this app&apos;s
        shipped packs with one signed commit, and sets it as this project&apos;s
        pack source.
      </p>
      {result ? null : (
        <div className="flex flex-col gap-2">
          <div className="flex flex-col gap-1">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor="project-packs-create-repo-id"
            >
              Repository id
            </label>
            <Input
              data-testid="project-packs-create-repo-id"
              disabled={pending}
              id="project-packs-create-repo-id"
              onChange={(event) => setRepoId(event.target.value)}
              value={repoId}
            />
            {repoIdError ? (
              <p
                className="text-2xs text-destructive"
                data-testid="project-packs-create-repo-id-error"
                role="alert"
              >
                {repoIdError}
              </p>
            ) : null}
          </div>
          <div className="flex flex-col gap-1">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor="project-packs-create-repo-name"
            >
              Name
            </label>
            <Input
              data-testid="project-packs-create-repo-name"
              disabled={pending}
              id="project-packs-create-repo-name"
              onChange={(event) => {
                setNameTouched(true);
                setManualName(event.target.value);
              }}
              value={name}
            />
          </div>
        </div>
      )}
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-packs-create-repo-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      {result ? (
        <div data-testid="project-packs-create-repo-result">
          <p
            className="text-xs"
            data-testid="project-packs-create-repo-coordinate"
          >
            Created <span className="font-mono">{result.repoRef}</span>.
          </p>
          <p
            className={
              result.seedCommitSha === null
                ? "text-xs text-destructive"
                : "text-xs"
            }
            data-testid="project-packs-create-repo-seed-outcome"
            role={result.seedCommitSha === null ? "alert" : undefined}
          >
            {describeSeedOutcome(result)}
          </p>
          {/* The raw text this sentence replaces — never git's stderr on
              its own, per LANE-L31 (Finding 66) — stays reachable behind a
              disclosure rather than thrown away. */}
          {result.seedError !== null ? (
            <details
              className="text-2xs text-muted-foreground"
              data-testid="project-packs-create-repo-seed-error-details"
            >
              <summary className="cursor-pointer">Details</summary>
              <pre className="whitespace-pre-wrap font-mono">
                {result.seedError}
              </pre>
            </details>
          ) : null}
          <dl className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-1 text-xs">
            <dt className="text-muted-foreground">Repository</dt>
            <dd className="truncate font-mono">{result.repoRef}</dd>
            <dt className="text-muted-foreground">Source event</dt>
            {result.sourceEventId === null ? (
              <dd className="text-muted-foreground">
                not published — the seed or push did not reach the relay
              </dd>
            ) : (
              <dd className="truncate font-mono">
                {truncatePubkey(result.sourceEventId)}
              </dd>
            )}
            <dt className="text-muted-foreground">Seed commit</dt>
            {result.seedCommitSha === null ? (
              <dd className="text-muted-foreground">none — seeding failed</dd>
            ) : (
              <dd className="truncate font-mono">
                {result.seedCommitSha.slice(0, 8)}
              </dd>
            )}
            <dt className="text-muted-foreground">Push record</dt>
            {result.pushRecordEventId === null ? (
              <dd className="text-muted-foreground">
                none yet — the relay had published no 30618 when this looked
              </dd>
            ) : (
              <dd className="truncate font-mono">
                {truncatePubkey(result.pushRecordEventId)}
              </dd>
            )}
          </dl>
        </div>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button
          disabled={pending}
          onClick={onCancel}
          size="sm"
          type="button"
          variant="ghost"
        >
          {result ? "Close" : "Cancel"}
        </Button>
        {result ? null : (
          <Button
            data-testid="project-packs-create-repo-submit"
            disabled={pending || repoIdError !== null}
            onClick={handleCreate}
            size="sm"
            type="button"
          >
            {pending ? "Creating…" : "Create"}
          </Button>
        )}
      </div>
    </div>
  );
}

function ProjectPackSourceForm({
  existingRepoAddrs,
  onCancel,
  onSaved,
  projectCoord,
}: {
  existingRepoAddrs: readonly string[];
  onCancel: () => void;
  onSaved: () => void;
  projectCoord: string;
}) {
  const [repo, setRepo] = React.useState(existingRepoAddrs[0] ?? "");
  const [pinKind, setPinKind] = React.useState<"ref" | "sha">("ref");
  const [pin, setPin] = React.useState("refs/heads/main");
  const [path, setPath] = React.useState("");
  const [note, setNote] = React.useState("");
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [publishedEventId, setPublishedEventId] = React.useState<string | null>(
    null,
  );

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSaving(true);
    try {
      const published = await publishProjectPackSource({
        note: note.trim() || null,
        path: path.trim() || null,
        projectCoord,
        repoCoord: repo.trim(),
        ref: pinKind === "ref" ? pin.trim() : null,
        sha: pinKind === "sha" ? pin.trim() : null,
      });
      setPublishedEventId(published.eventId);
      onSaved();
    } catch (thrown) {
      setError(
        thrown instanceof Error
          ? thrown.message
          : "Failed to set the pack source.",
      );
    } finally {
      setSaving(false);
    }
  }

  return (
    <form
      className="flex flex-col gap-3 rounded-md border border-border/60 p-3"
      data-testid="project-packs-set-source-form"
      onSubmit={handleSubmit}
    >
      <div className="flex flex-col gap-1">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="project-packs-repo"
        >
          Repository coordinate
        </label>
        <Input
          data-testid="project-packs-repo-input"
          id="project-packs-repo"
          list="project-packs-repo-suggestions"
          onChange={(event) => setRepo(event.target.value)}
          placeholder="30617:<owner-hex>:<id>"
          value={repo}
        />
        <datalist id="project-packs-repo-suggestions">
          {existingRepoAddrs.map((addr) => (
            <option key={addr} value={addr} />
          ))}
        </datalist>
      </div>

      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-muted-foreground">
          Pin to
        </span>
        <div className="flex gap-2">
          <label className="flex items-center gap-1 text-xs">
            <input
              checked={pinKind === "ref"}
              onChange={() => {
                setPinKind("ref");
                setPin("refs/heads/main");
              }}
              type="radio"
              value="ref"
            />
            A branch (ref)
          </label>
          <label className="flex items-center gap-1 text-xs">
            <input
              checked={pinKind === "sha"}
              onChange={() => {
                setPinKind("sha");
                setPin("");
              }}
              type="radio"
              value="sha"
            />
            An exact commit (sha)
          </label>
        </div>
        <Input
          data-testid="project-packs-pin-input"
          onChange={(event) => setPin(event.target.value)}
          placeholder={
            pinKind === "ref" ? "refs/heads/main" : "40 hex characters"
          }
          value={pin}
        />
      </div>

      <div className="flex flex-col gap-1">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="project-packs-path"
        >
          Path (optional — defaults to <code>personas/roles</code>)
        </label>
        <Input
          id="project-packs-path"
          onChange={(event) => setPath(event.target.value)}
          placeholder="personas/roles"
          value={path}
        />
      </div>

      <div className="flex flex-col gap-1">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="project-packs-note"
        >
          Note (optional)
        </label>
        <Input
          id="project-packs-note"
          maxLength={512}
          onChange={(event) => setNote(event.target.value)}
          value={note}
        />
      </div>

      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-packs-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      {publishedEventId ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-packs-published"
        >
          Published {truncatePubkey(publishedEventId)}.
        </p>
      ) : null}

      <div className="flex justify-end gap-2">
        <Button
          disabled={saving}
          onClick={onCancel}
          size="sm"
          type="button"
          variant="ghost"
        >
          Cancel
        </Button>
        <Button
          data-testid="project-packs-set-source-submit"
          disabled={saving || repo.trim() === "" || pin.trim() === ""}
          size="sm"
          type="submit"
        >
          {saving ? "Setting…" : "Use this repository"}
        </Button>
      </div>
    </form>
  );
}
