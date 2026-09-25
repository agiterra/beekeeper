import { CircleAlert } from "lucide-react";

import type { HostAdmission } from "@/shared/api/tauriSessionProvider";

import { hostAdmissionLine } from "../lib/codingSessionHostAdmission";

/**
 * One line, only when this computer is not admitted to the session's project
 * (ledger 266): "This computer cannot read this project: <reason>". Public
 * and admitted projects render nothing.
 */
export function CodingSessionHostAdmissionLine({
  admission,
}: {
  admission: HostAdmission | null | undefined;
}) {
  const line = hostAdmissionLine(admission);
  if (line === null) return null;
  return (
    <p
      className="flex items-start gap-2 text-sm text-destructive"
      data-testid="coding-session-host-admission"
      role="status"
    >
      <CircleAlert className="mt-0.5 size-4 shrink-0" />
      {line}
    </p>
  );
}
