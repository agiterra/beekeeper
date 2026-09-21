/**
 * Whether the founding form opens with "Use roles" on, and what a person
 * loses by turning it off.
 *
 * # The finding this exists for
 *
 * Ledger 207(2). On 2026-09-20 the founder's form for the brand-new project
 * "Kettle Smoke" opened with **Use roles unticked**, on a project whose
 * agents repository had just been seeded with eight roles. Off, the session
 * runs with no role instructions, the readiness panel is not shown at all,
 * and the founder signs no project-action delegation (ledger 186) — a silent
 * downgrade of the whole team, chosen by a `useState(false)` rather than by
 * anything about the project.
 *
 * The default now follows one fact: **does this project have a role source**
 * — a kind:30624 pack source / agents repository, staged or not. It does not
 * ask whether a session has ever run here, and it does not ask whether this
 * computer could stage the packs; a staging failure is a disclosed readiness
 * fact, not a reason to seat agents with no role at all.
 *
 * A person may still untick it. When they do, the form says in one sentence
 * what that costs.
 */

/** What the readiness read says about the project's role source. */
export type CodingSessionRoleSourceFact = boolean | null | undefined;

/**
 * The checkbox's value: the person's own choice when they have made one,
 * otherwise the project's fact.
 *
 * `null`/`undefined` (nothing asked yet) reads as off, because seating an
 * agent on a role the form has not confirmed exists would be a guess.
 */
export function resolveCodingSessionUseRoles(input: {
  /** `null` until the person touches the checkbox. */
  choice: boolean | null;
  packSourcePresent: CodingSessionRoleSourceFact;
}): boolean {
  if (input.choice !== null) return input.choice;
  return input.packSourcePresent === true;
}

/**
 * The one sentence shown beside an unticked box on a project that has a role
 * source — never on a project that has none, where roles-off is simply the
 * truth and a warning would be noise.
 */
export function codingSessionUseRolesOffWarning(input: {
  useRoles: boolean;
  packSourcePresent: CodingSessionRoleSourceFact;
}): string | null {
  if (input.useRoles || input.packSourcePresent !== true) return null;
  return "This project has roles. Off, its agents are seated with no role instructions, readiness is not checked, and this session's lead gets no delegation to publish or run the project's actions.";
}
