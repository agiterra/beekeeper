import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { cn } from "@/shared/lib/cn";

import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";

/**
 * The session's founder, named.
 *
 * `variant="line"` is the standalone bar under the header and still requires
 * a linked genesis. `variant="inline"` makes the same claim, under the same
 * condition, as a quiet span that rides at the end of another row (the
 * single-session workspace's goal row). `variant="label"` is the provenance
 * popover's value cell and needs only the founder key, however it resolved.
 */
export function CodingSessionFounderLine({
  founderPubkey,
  genesisRef,
  variant = "line",
}: {
  founderPubkey: string | null;
  genesisRef: string | null;
  variant?: "inline" | "label" | "line";
}) {
  const gutter = useCodingSessionColumnGutter();
  // The `label` variant only needs the founder: a legacy session (creates
  // observed, none naming a genesis) still has one, and dropping the name for
  // want of a genesis ref is what left the provenance popover founderless
  // (walk finding 5). The standalone `line` still waits for the linked
  // genesis, because that line's whole claim is the resolved authority chain.
  const wantsProfile = Boolean(
    founderPubkey && (variant === "label" || genesisRef),
  );
  const pubkeys = React.useMemo(
    () => (wantsProfile && founderPubkey ? [founderPubkey] : []),
    [founderPubkey, wantsProfile],
  );
  const profiles = useUsersBatchQuery(pubkeys).data?.profiles;
  if (!founderPubkey) return null;
  const label = resolveUserLabel({ pubkey: founderPubkey, profiles });
  if (variant === "label") {
    return <span className="font-medium text-foreground">{label}</span>;
  }
  if (!genesisRef) return null;
  if (variant === "inline") {
    return (
      <span
        className="ms-auto shrink-0 whitespace-nowrap text-xs text-muted-foreground"
        data-genesis-ref={genesisRef}
        data-testid="coding-session-founded-by"
      >
        Founded by <span className="font-medium text-foreground">{label}</span>
      </span>
    );
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
