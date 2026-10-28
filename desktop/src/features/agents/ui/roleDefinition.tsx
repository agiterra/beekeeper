import {
  ROLE_DEFINITION_EXECUTION_NOTE,
  ROLE_DEFINITION_EXECUTION_NOTE_TESTID,
  ROLE_DEFINITION_ID_PREFIX,
} from "./agentDirectoryCopy";

type Values = {
  id?: string;
  runtime?: string | null;
  model?: string | null;
  provider?: string | null;
} | null;

/**
 * A role-pack definition (`crew-role:` id). It holds instructions only; its
 * agent's runtime, model and behaviour are per agent, and the backend refuses
 * a definition save that changes them (ledger 401).
 */
export function isRole(values: Values): boolean {
  return Boolean(values?.id?.startsWith(ROLE_DEFINITION_ID_PREFIX));
}

/** Shown on a role in place of its execution settings. */
export function Note({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <p
      className="text-sm text-muted-foreground"
      data-testid={ROLE_DEFINITION_EXECUTION_NOTE_TESTID}
    >
      {ROLE_DEFINITION_EXECUTION_NOTE}
    </p>
  );
}
