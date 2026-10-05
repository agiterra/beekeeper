import { Users } from "lucide-react";
import * as React from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

import {
  CodingSessionPeopleBody,
  codingSessionPeopleIntro,
  useCodingSessionPeopleIsOwner,
} from "./CodingSessionPeopleBody";

/**
 * The People dialog: a thin wrapper around {@link CodingSessionPeopleBody},
 * which the People surface (SV-24) mounts too. The roster, the invite and
 * the revoke all live in the body, so the dialog and the surface cannot
 * disagree about who has access.
 */
export function CodingSessionPeoplePopover({
  channelId,
  founderPubkey,
  genesisRef,
  onOpenChange,
  open,
  providerAuthorityLabels,
}: {
  channelId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  /** Handed straight to the body; see its doc comment. */
  providerAuthorityLabels?: ReadonlyMap<string, string | null>;
}) {
  const isOwner = useCodingSessionPeopleIsOwner(founderPubkey);
  const rosterRef = React.useRef<HTMLElement>(null);

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-md"
        data-testid="coding-session-people"
        onOpenAutoFocus={(event) => {
          // Radix otherwise focuses the first tabbable descendant, which is
          // the invite search field — and that field opens its recipient
          // popover `onFocus`, so the dialog's opening frame covered the
          // roster with the agent directory. This surface's first job is
          // saying who has access; inviting is its second. Focus lands on the
          // roster region (programmatic only, `tabIndex={-1}`), so Escape and
          // the focus trap still work and one Tab reaches the invite.
          event.preventDefault();
          rosterRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Users aria-hidden className="size-4" />
            People
          </DialogTitle>
          <DialogDescription>
            {codingSessionPeopleIntro(isOwner)}
          </DialogDescription>
        </DialogHeader>
        <CodingSessionPeopleBody
          active={open}
          channelId={channelId}
          founderPubkey={founderPubkey}
          genesisRef={genesisRef}
          providerAuthorityLabels={providerAuthorityLabels}
          rosterRef={rosterRef}
        />
      </DialogContent>
    </Dialog>
  );
}
