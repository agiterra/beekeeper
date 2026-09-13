import type {
  ProjectTeamSetupDraft,
  ProjectTeamSetupSnapshot,
} from "../lib/projectTeamSetup";
import type { ProjectTeamSetupPublicationProgress } from "./ProjectTeamSetupPublication";

function Row({
  label,
  value,
  testId,
}: {
  label: string;
  value: string | null | undefined;
  testId?: string;
}) {
  if (!value) return null;
  return (
    <div data-testid={testId}>
      <dt className="font-medium">{label}</dt>
      <dd className="break-all">{value}</dd>
    </div>
  );
}

/**
 * Folder paths, identifiers, refs, commits and the host's raw journal
 * messages: true and sometimes needed, but not what a person reads to decide
 * the next step. Collapsed by default.
 */
export function ProjectTeamSetupTechnicalDetails({
  draft,
  snapshot,
  progress,
}: {
  draft: ProjectTeamSetupDraft;
  snapshot: ProjectTeamSetupSnapshot | null;
  progress: ProjectTeamSetupPublicationProgress | null;
}) {
  const destination =
    progress?.publication?.destination ??
    progress?.options?.suggestedDestination ??
    null;
  const publication = progress?.publication ?? null;
  const activation = progress?.activation ?? null;
  return (
    <details data-testid="project-team-setup-technical-details">
      <summary className="cursor-pointer text-sm font-medium">
        Technical details
      </summary>
      <dl className="mt-2 space-y-2 text-sm">
        <Row
          label="Repository folder (checked when prepared)"
          value={draft.projectDirectory}
        />
        <Row label="Draft role packs" value={draft.rolesDirectory} />
        <Row label="Setup" value={draft.setupId} />
        {snapshot ? (
          <div data-testid="project-team-setup-saved-version">
            <dt className="font-medium">Saved version</dt>
            <dd className="break-all">{snapshot.snapshotId}</dd>
            <dd>Roles: {snapshot.roles.join(", ")}</dd>
          </div>
        ) : null}
        {destination ? (
          <Row
            label="Shared destination"
            testId="project-team-setup-publication-target"
            value={`${destination.repoRef} at ${destination.packPath}`}
          />
        ) : null}
        <Row label="Publication" value={publication?.publicationId} />
        <Row label="Publication status" value={publication?.status} />
        <Row label="Candidate ref" value={publication?.candidateRef} />
        <Row label="Candidate commit" value={publication?.candidateCommit} />
        <Row label="Source event" value={publication?.sourceEventId} />
        <Row label="Publication message" value={publication?.message} />
        {activation?.source ? (
          <Row
            label="Adopted source"
            testId="project-team-setup-adopted-source"
            value={`${activation.source.repoRef} @ ${activation.source.commit} / ${activation.source.packPath}`}
          />
        ) : null}
        <Row
          label="Installation status"
          value={activation?.installation.status}
        />
        <Row
          label="Installation message"
          value={activation?.installation.message}
        />
        {activation?.installation.installedRoles.map((role) => (
          <Row
            key={role.agentPubkey}
            label={`Installed ${role.role}`}
            value={`${role.agentPubkey} from ${role.packRef.repo} @ ${role.packRef.sha}`}
          />
        ))}
        <Row label="Lead status" value={activation?.lead.status} />
        <Row label="Lead identity" value={activation?.lead.leadPubkey} />
        <Row label="Lead channel" value={activation?.lead.channelId} />
        <Row label="Lead session" value={activation?.lead.sessionRef} />
        <Row label="Lead message" value={activation?.lead.message} />
      </dl>
    </details>
  );
}
