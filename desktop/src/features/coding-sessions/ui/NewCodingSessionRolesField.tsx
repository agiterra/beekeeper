import { Checkbox } from "@/shared/ui/checkbox";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";
import type { useProjectTeamReadiness } from "../lib/useProjectTeamReadiness";
import { TeamReadinessCard } from "./TeamReadinessCard";

/**
 * Whether a launch uses the project's role packs, and — only when it does —
 * the project-roles readiness card.
 *
 * Roles are opt-in. A session led by the person, with no seats, runs on none
 * of the things readiness checks (a recorded checkout, a supervised provider,
 * installed packs), so off, the card is hidden and Start is not gated by it.
 * On, the card and its gate are exactly what they were.
 */
export function NewCodingSessionRolesField(props: {
  useRoles: boolean;
  onUseRolesChange: (next: boolean) => void;
  /**
   * What unticking costs, on a project that has a role source — `null` on a
   * project that has none, where roles-off is simply the truth
   * (ledger 207(2)).
   */
  useRolesOffWarning?: string | null;
  disabled: boolean;
  teamReadiness: ReturnType<typeof useProjectTeamReadiness>;
  launchRoles: readonly string[];
  runtimeTarget: NewCodingSessionTarget | null;
  /** The repository folder this form already holds, for the one-click record. */
  candidateCheckout?: string | null;
  projectRef?: string | null;
  projectLabel?: string | null;
  onCheckoutRecorded?: () => void;
}) {
  const { teamReadiness } = props;
  return (
    <>
      <label
        className="flex items-center gap-2 text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-use-roles-toggle"
      >
        <Checkbox
          checked={props.useRoles}
          data-testid="coding-session-use-roles-toggle"
          disabled={props.disabled}
          id="coding-session-use-roles-toggle"
          onCheckedChange={(next) => props.onUseRolesChange(next === true)}
        />
        Use roles
        <span className="font-normal">
          — seat agents from this project&rsquo;s role packs; off, the session
          runs without them and no preparation is needed
        </span>
      </label>
      {!props.useRoles && props.useRolesOffWarning ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="coding-session-use-roles-off-warning"
        >
          {props.useRolesOffWarning}
        </p>
      ) : null}
      {props.useRoles ? (
        <TeamReadinessCard
          loading={teamReadiness.isLoading}
          externalBusy={props.disabled}
          names={teamReadiness.names}
          onBeginPrepare={() => void teamReadiness.beginPrepare()}
          onCancelPrepare={teamReadiness.cancelPrepare}
          onConfirmPrepare={() => void teamReadiness.confirmPrepare()}
          onNameChange={teamReadiness.setName}
          prepareError={teamReadiness.prepareError}
          prepareWarning={teamReadiness.prepareWarning}
          prepareSteps={teamReadiness.prepareSteps}
          preparing={teamReadiness.isPreparing}
          readError={teamReadiness.readError}
          readiness={teamReadiness.readiness}
          scan={teamReadiness.scan}
          scanning={teamReadiness.isScanning}
          selectedRoles={props.launchRoles}
          runtimeTarget={props.runtimeTarget}
          candidateCheckout={props.candidateCheckout}
          projectRef={props.projectRef}
          projectLabel={props.projectLabel}
          onCheckoutRecorded={props.onCheckoutRecorded}
        />
      ) : null}
    </>
  );
}
