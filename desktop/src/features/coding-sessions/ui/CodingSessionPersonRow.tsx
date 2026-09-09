import { ChevronDown, Server } from "lucide-react";

import type { CodingSessionRosterEntry } from "@/features/coding-sessions/lib/codingSessionRoster";
import {
  codingSessionRowKindBadge,
  codingSessionRowSentence,
  type CodingSessionRowKind,
} from "@/features/coding-sessions/lib/codingSessionRowKind";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import { ENTITY_ROLE_LABELS } from "@/shared/lib/entityRoles";
import { Badge } from "@/shared/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { PubKey } from "@/shared/ui/PubKey";

/**
 * One session-roster row.
 *
 * Presentational and hook-free by design: every fact it renders is resolved by
 * the popover and handed down, so the whole row-kind vocabulary is exercisable
 * without a relay, a query client, or a profile batch.
 *
 * The row says three things and nothing more: who the key is *as far as
 * evidence reaches* (`kind`), what it may do here in ordinary words, and the
 * key itself through the shared {@link PubKey}. A name is displayed but is
 * never treated as evidence of ownership or species.
 */
export function CodingSessionPersonRow({
  avatarUrl,
  entry,
  hasProfile,
  isFounderViewer,
  kind,
  name,
  onRevoke,
  providerLabel,
}: {
  avatarUrl: string | null;
  entry: CodingSessionRosterEntry;
  /** A kind:0 resolved for this key. Not evidence of personhood — see below. */
  hasProfile: boolean;
  /** The viewer founded this session, so management controls are theirs. */
  isFounderViewer: boolean;
  /** `null` while the kind could still change — the no-flicker gate. */
  kind: CodingSessionRowKind | null;
  name: string;
  onRevoke: () => void;
  providerLabel: string | null;
}) {
  const seatRole = entry.seatRole ?? null;
  const isProvider = kind === "provider";
  const { capability, evidence } = codingSessionRowSentence({
    kind,
    role: entry.role,
    seatRole,
    providerLabel,
    hasProfile,
  });
  const sentence =
    evidence === null ? capability : `${capability} · ${evidence}`;
  return (
    <li
      // `shrink-0` is load-bearing, not decoration. The roster is a flex
      // column with `max-h-72 overflow-y-auto`, so a row left at the default
      // `flex-shrink: 1` is compressed to fit the column instead of making
      // the column scroll: the browser lane measured a 74px row rendered 47px
      // tall, with the owner's wrapped sentence printing on top of the
      // provider row beneath it (94px of overflow at maximum text scale). The
      // row grows; the list scrolls.
      className="flex min-h-8 shrink-0 items-start gap-2 px-1 py-1"
      data-testid={`coding-session-people-row-${entry.pubkey}`}
    >
      {isProvider ? (
        // Not a `ProfileAvatar`: a provider authority key is a computer, and a
        // person-shaped disc with a hex initial in it is the first half of the
        // exact mistake this row exists to end.
        <span
          aria-hidden
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border/70 bg-muted/60 text-muted-foreground"
        >
          <Server className="h-3 w-3" />
        </span>
      ) : (
        <ProfileAvatar
          avatarUrl={avatarUrl}
          className="h-6 w-6 shrink-0 text-2xs shadow-none"
          iconClassName="h-3 w-3"
          label={name}
        />
      )}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* The name may be clipped — it is a label, and the key beside it
            disambiguates. The sentence below never is: it is the payload of
            this whole surface, and a one-line clamp reduced the seat row to
            25 of 308 measured pixels (and to nothing at all at the app's
            maximum text scale), leaving the fact only in a `title` tooltip
            that a person reading the roster never opens. It wraps, and the
            row grows. */}
        <span className="shrink-0 truncate text-sm">{name}</span>
        {/* Same hazard, inner column: this wrapper is the flex child that
            holds the wrapped sentence, so it must not be shrinkable either. */}
        <span
          className="flex min-w-0 shrink-0 flex-wrap items-baseline gap-x-1 text-2xs text-muted-foreground"
          data-testid={`coding-session-people-detail-${entry.pubkey}`}
        >
          <span className="min-w-0 break-words">{sentence}</span>
          {hasProfile ? (
            // Identical display names on different keys stay distinguishable.
            <PubKey
              className="shrink-0 text-2xs"
              interactive={false}
              pubkey={entry.pubkey}
            />
          ) : null}
        </span>
      </div>
      {kind === null || kind === "owner" ? null : (
        <Badge
          className="mt-0.5 shrink-0"
          data-testid={`coding-session-people-kind-${entry.pubkey}`}
          variant={kind === "unidentified" ? "outline" : "secondary"}
        >
          {codingSessionRowKindBadge({ kind, seatRole })}
        </Badge>
      )}
      {entry.pending ? (
        <span
          className="mt-0.5 shrink-0 text-xs text-muted-foreground italic"
          title="Waiting for the relay's acceptance receipt"
        >
          Inviting…
        </span>
      ) : entry.role === "owner" ? (
        <Badge className="mt-0.5 shrink-0" variant="secondary">
          {ENTITY_ROLE_LABELS.owner}
        </Badge>
      ) : isFounderViewer ? (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              aria-label={`Manage access for ${name}`}
              className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              data-testid={`coding-session-people-menu-${entry.pubkey}`}
              type="button"
            >
              {ENTITY_ROLE_LABELS[entry.role]}
              <ChevronDown className="size-3" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem
              className="text-destructive focus:text-destructive"
              data-testid={`coding-session-people-revoke-${entry.pubkey}`}
              onSelect={onRevoke}
            >
              Revoke access
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : (
        <span className="mt-0.5 shrink-0 text-xs text-muted-foreground">
          {ENTITY_ROLE_LABELS[entry.role]}
        </span>
      )}
    </li>
  );
}
