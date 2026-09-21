/**
 * What a project roster row is called, and whether it is an agent.
 *
 * # The finding this exists for
 *
 * Ledger 207(4). On 2026-09-20 the Members panel of "Kettle Smoke" listed
 * that project's eight agents as `5f6c1173…4913 · Collaborator` — bare hex
 * keys — while the host holding the panel knew every one of them: they are
 * this computer's managed agents, with names and primary roles ("Verifier 2
 * · verifier"). The roster read resolves names from published profiles only
 * (`useUsersBatchQuery`), and a private project's agents publish none.
 *
 * The rule this file keeps, from an earlier slice's memory: **never call a
 * missing profile a human.** A key with no profile and no local record is
 * rendered as a key, unlabelled — not as a person.
 */

/** The local evidence: one managed agent record on this computer. */
export type ProjectMemberManagedAgent = {
  pubkey: string;
  name: string;
  homeRole?: string | null;
};

/** What a row should show for one pubkey. */
export type ProjectMemberIdentity = {
  /** The primary line. Falls back to the truncated key, never to a guess. */
  name: string;
  /** The secondary line: the role, when known. */
  role: string | null;
  /**
   * `"agent"` — a managed agent on this computer, or a profile that declares
   * itself one. `"profile"` — a published profile that does not. `"unknown"`
   * — nothing on this computer knows this key; it is NOT a human.
   */
  kind: "agent" | "profile" | "unknown";
  /** Whether the hex key should be shown underneath as secondary text. */
  showKey: boolean;
};

/**
 * Resolve one roster row. The local managed-agent record wins over an absent
 * profile and loses to nothing: a published display name and a local name for
 * the same key are the same identity, and the local record additionally knows
 * the role.
 */
export function projectMemberIdentity(input: {
  pubkey: string;
  /** The published profile's display name, if any. */
  profileName?: string | null;
  /** Whether the published profile declares itself an agent. */
  profileIsAgent?: boolean | null;
  /** This computer's managed agent for the key, if any. */
  managedAgent?: ProjectMemberManagedAgent | null;
  /** How this app truncates a key for display. */
  truncate: (pubkey: string) => string;
}): ProjectMemberIdentity {
  const profileName = input.profileName?.trim() || null;
  const managed = input.managedAgent ?? null;
  const managedName = managed?.name?.trim() || null;
  const role = managed?.homeRole?.trim() || null;
  if (managed) {
    return {
      name: profileName ?? managedName ?? input.truncate(input.pubkey),
      role,
      kind: "agent",
      showKey: true,
    };
  }
  if (profileName) {
    return {
      name: profileName,
      role: null,
      kind: input.profileIsAgent ? "agent" : "profile",
      showKey: false,
    };
  }
  return {
    name: input.truncate(input.pubkey),
    role: null,
    kind: "unknown",
    showKey: false,
  };
}
