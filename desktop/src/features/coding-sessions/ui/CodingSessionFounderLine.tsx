import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { cn } from "@/shared/lib/cn";

import { useCodingSessionColumnGutter } from "../lib/codingSessionGutterPreference";

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
  const gutter = useCodingSessionColumnGutter();
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
      className={cn(
        "border-b border-border/50 py-2 text-xs text-muted-foreground",
        gutter,
      )}
      data-genesis-ref={genesisRef}
      data-testid="coding-session-founded-by"
    >
      Founded by <span className="font-medium text-foreground">{label}</span>
    </div>
  );
}
