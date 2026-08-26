import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";

/** Minimal provenance line for sessions whose linked genesis resolved. */
export function CodingSessionFounderLine({
  founderPubkey,
  genesisRef,
  variant = "line",
}: {
  founderPubkey: string | null;
  genesisRef: string | null;
  variant?: "label" | "line";
}) {
  const pubkeys = React.useMemo(
    () => (founderPubkey && genesisRef ? [founderPubkey] : []),
    [founderPubkey, genesisRef],
  );
  const profiles = useUsersBatchQuery(pubkeys).data?.profiles;
  if (!founderPubkey || !genesisRef) return null;
  const label = resolveUserLabel({ pubkey: founderPubkey, profiles });
  if (variant === "label") {
    return <span className="font-medium text-foreground">{label}</span>;
  }
  return (
    <div
      className="border-b border-border/50 px-5 py-2 text-xs text-muted-foreground sm:px-8"
      data-genesis-ref={genesisRef}
      data-testid="coding-session-founded-by"
    >
      Founded by <span className="font-medium text-foreground">{label}</span>
    </div>
  );
}
