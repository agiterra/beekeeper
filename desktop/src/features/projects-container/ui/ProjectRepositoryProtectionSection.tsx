import * as React from "react";

import type { Repository } from "@/features/projects/projectModels";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Switch } from "@/shared/ui/switch";

import type { ProjectContainer } from "../hooks";
import {
  parseProtectionTags,
  PROTECTED_MAIN_REF,
  requireVerdictOnMain,
  setRequireVerdictOnMain,
  useProjectRepositories,
  type ProtectionRule,
} from "../lib/projectRepositoryProtection";

/**
 * "Repository → Protection" — the LANE-L23 addendum's small panel: which
 * `buzz-protect` rules a project's linked repositories carry, who may set
 * them (the announcement's own signer — never a maintainer or a project
 * owner), and one interactive control for the rule that matters most,
 * "Require verdict on main".
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
  const [rules, setRules] = React.useState<ProtectionRule[]>(() =>
    parseProtectionTags(repository.eventTags ?? []),
  );
  React.useEffect(() => {
    setRules(parseProtectionTags(repository.eventTags ?? []));
  }, [repository.eventTags]);

  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [publishedEventId, setPublishedEventId] = React.useState<string | null>(
    null,
  );

  const owner = repository.owner.toLowerCase();
  const isSigner = self !== null && self === owner;
  const enabled = requireVerdictOnMain(rules);

  async function handleToggle(next: boolean) {
    setPending(true);
    setError(null);
    try {
      const result = await setRequireVerdictOnMain({
        owner: repository.owner,
        dtag: repository.dtag,
        enabled: next,
      });
      setRules(result.rules);
      setPublishedEventId(result.eventId);
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

      {rules.length > 0 ? (
        <ul
          className="flex flex-col gap-0.5 text-xs text-muted-foreground"
          data-testid="project-repository-protection-rules"
        >
          {rules.map((rule) => (
            <li key={rule.refPattern}>
              <code>{rule.refPattern}</code>:{" "}
              {rule.rules.join(", ") || "no rules"}
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
        Only {truncatePubkey(repository.owner)} — this repository's own
        announcement signer — may change these rules.
      </p>

      <div className="flex items-center gap-2">
        <Switch
          checked={enabled}
          data-testid="project-repository-require-verdict-switch"
          disabled={!isSigner || pending}
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
      {!isSigner ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-repository-protection-readonly-note"
        >
          Only {truncatePubkey(repository.owner)} can set this.
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
      {publishedEventId ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-repository-protection-published"
        >
          Published {truncatePubkey(publishedEventId)}.
        </p>
      ) : null}
    </div>
  );
}
