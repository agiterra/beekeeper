import * as React from "react";

import type { Repository } from "@/features/projects/projectModels";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Switch } from "@/shared/ui/switch";

import type { ProjectContainer } from "../hooks";
import {
  fetchRuleRecords,
  PROTECTED_MAIN_REF,
  repositoryFounders,
  requireVerdictFromDecisions,
  resolveProtection,
  ruleRecordLayer,
  setRequireVerdictOnMain,
  useProjectRepositories,
  type ProtectionDecision,
  type ProtectionLayer,
} from "../lib/projectRepositoryProtection";

/**
 * "Repository → Protection" — which `buzz-protect` rules a project's linked
 * repositories carry, **which record carries each one and who signed it**,
 * who may change them, and one interactive control for the rule that matters
 * most, "Require verdict on main".
 *
 * Lane L26: the switch is live for any **founder**, not only the
 * announcement's signer. A co-founder's toggle signs a rule record (kind
 * 30625) rather than republishing an announcement they cannot address — which
 * is what the old signer-only control disclosed but could not do.
 */
export function ProjectRepositoryProtectionSection({
  project,
}: {
  project: ProjectContainer;
}) {
  const { repositories, isLoading } = useProjectRepositories(project);
  const identityQuery = useIdentityQuery();
  const self = identityQuery.data?.pubkey?.toLowerCase() ?? null;

  if (isLoading) {
    return (
      <p className="text-xs text-muted-foreground">Reading repositories…</p>
    );
  }
  if (repositories.length === 0) {
    return (
      <p className="text-sm" data-testid="project-repository-protection-empty">
        This project has no linked repositories.
      </p>
    );
  }

  return (
    <div
      className="flex flex-col gap-4"
      data-testid="project-repository-protection-section"
    >
      {repositories.map((repository) => (
        <RepositoryProtectionCard
          key={repository.repoAddress}
          repository={repository}
          self={self}
        />
      ))}
    </div>
  );
}

function RepositoryProtectionCard({
  repository,
  self,
}: {
  repository: Repository;
  self: string | null;
}) {
  const [records, setRecords] = React.useState<ProtectionLayer[]>([]);
  const [recordsRead, setRecordsRead] = React.useState(false);

  const founders = React.useMemo(
    () => repositoryFounders(repository),
    [repository],
  );
  // The announcement this panel resolves against: the one the catalog handed
  // us, until this viewer republishes it. The catalog's copy is a snapshot and
  // does not refresh on our publish, so without the override the signer's own
  // toggle would flip back to what it was — the control lying about the change
  // it just made.
  const [publishedAnnouncement, setPublishedAnnouncement] = React.useState<{
    id: string;
    pubkey: string;
    created_at: number;
    tags: string[][];
  } | null>(null);
  const announcement = React.useMemo(
    () =>
      publishedAnnouncement ?? {
        created_at: repository.createdAt,
        id: repository.eventId ?? "",
        pubkey: repository.owner,
        tags: repository.eventTags ?? [],
      },
    [publishedAnnouncement, repository],
  );
  // Resolved during render, not in the effect: the announcement's own rows are
  // in hand already and must paint immediately — a panel that shows nothing
  // until a network read lands would read as "no rules are set", which is the
  // most dangerous thing this screen could say wrongly.
  const decisions: ProtectionDecision[] = React.useMemo(
    () => resolveProtection({ announcement, records }),
    [announcement, records],
  );

  // Read-optional: a rule record supersedes the announcement's rows once it
  // arrives, and a failed read leaves the pre-L26 answer standing with
  // `recordsRead` false so the panel says so rather than implying there are
  // none.
  const refreshRecords = React.useCallback(async () => {
    const events = await fetchRuleRecords({
      dtag: repository.dtag,
      owner: repository.owner,
    });
    setRecords(
      events
        .map((event) =>
          ruleRecordLayer(event, repository.owner, repository.dtag, founders),
        )
        .filter((layer) => layer !== null),
    );
    setRecordsRead(true);
  }, [repository, founders]);

  React.useEffect(() => {
    let cancelled = false;
    setRecords([]);
    setRecordsRead(false);
    setPublishedAnnouncement(null);
    void refreshRecords().catch(() => {
      if (cancelled) return;
    });
    return () => {
      cancelled = true;
    };
  }, [refreshRecords]);

  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [published, setPublished] = React.useState<{
    eventId: string;
    record: string;
  } | null>(null);

  const isFounder = self !== null && founders.includes(self);
  const enabled = requireVerdictFromDecisions(decisions);

  async function handleToggle(next: boolean) {
    setPending(true);
    setError(null);
    try {
      const result = await setRequireVerdictOnMain({
        dtag: repository.dtag,
        enabled: next,
        owner: repository.owner,
        viewerPubkey: self,
      });
      setPublished({ eventId: result.eventId, record: result.record });
      if (result.record === "announcement") {
        setPublishedAnnouncement({
          created_at: Math.floor(Date.now() / 1000),
          id: result.eventId,
          pubkey: repository.owner,
          tags: [
            ...(repository.eventTags ?? []).filter(
              (tag) => tag[0] !== "buzz-protect",
            ),
            ...result.rules.map((rule) => [
              "buzz-protect",
              rule.refPattern,
              ...rule.rules,
            ]),
          ],
        });
      }
      await refreshRecords();
    } catch (thrown) {
      setError(
        thrown instanceof Error
          ? thrown.message
          : "Failed to set the protection rule.",
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-border/60 p-3"
      data-testid="project-repository-protection-card"
    >
      <p className="truncate text-sm font-medium">{repository.name}</p>

      {decisions.length > 0 ? (
        <ul
          className="flex flex-col gap-0.5 text-xs text-muted-foreground"
          data-testid="project-repository-protection-rules"
        >
          {decisions.map((decision) => (
            <li key={decision.refPattern}>
              <code>{decision.refPattern}</code>:{" "}
              {decision.cleared
                ? "cleared — no rules"
                : decision.rules.join(", ") || "no rules"}{" "}
              <span
                className="text-2xs"
                data-testid={`project-repository-protection-source-${decision.refPattern}`}
              >
                (
                {decision.source.record === "announcement"
                  ? "on the announcement"
                  : "rule record"}
                , signed by {truncatePubkey(decision.source.signedBy)})
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-repository-protection-none"
        >
          No protection rules are set on this repository.
        </p>
      )}

      <p className="text-2xs text-muted-foreground">
        {founders.length === 1
          ? `Only ${truncatePubkey(repository.owner)} founds this repository, so only that key can change these rules.`
          : `Any of this repository's ${founders.length} founders can change these rules; each change is signed and says who made it.`}
        {recordsRead
          ? ""
          : " Rule records were not read here, so a founder's own record may not be listed."}
      </p>

      <div className="flex items-center gap-2">
        <Switch
          checked={enabled}
          data-testid="project-repository-require-verdict-switch"
          disabled={!isFounder || pending}
          id={`project-repository-require-verdict-${repository.repoAddress}`}
          onCheckedChange={handleToggle}
        />
        <label
          className="text-xs"
          htmlFor={`project-repository-require-verdict-${repository.repoAddress}`}
        >
          Require verdict on {PROTECTED_MAIN_REF}
        </label>
      </div>
      {!isFounder ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-repository-protection-readonly-note"
        >
          Only a founder of this repository can set this.
        </p>
      ) : null}

      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-repository-protection-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      {published ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-repository-protection-published"
        >
          Published {truncatePubkey(published.eventId)} as{" "}
          {published.record === "announcement"
            ? "a change to this repository's announcement"
            : "your own rule record for this repository"}
          .
        </p>
      ) : null}
    </div>
  );
}
