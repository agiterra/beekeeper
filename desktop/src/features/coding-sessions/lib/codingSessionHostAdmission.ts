import type { HostAdmission } from "@/shared/api/tauriSessionProvider";

/**
 * The one honest line a session shows about this computer's admission to its
 * project (ledger 266), or `null` when there is nothing to disclose: the
 * project is public, the host was admitted and read the project as itself,
 * or no admission was asked for (no project, or another computer's host).
 */
export function hostAdmissionLine(
  admission: HostAdmission | null | undefined,
): string | null {
  if (!admission) return null;
  switch (admission.state) {
    case "public":
    case "admitted":
      return null;
    case "unauthorized":
    case "unreadable":
      return `This computer cannot read this project: ${admission.reason}`;
  }
}
