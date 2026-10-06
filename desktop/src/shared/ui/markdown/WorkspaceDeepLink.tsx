import type * as React from "react";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import {
  type CodingSessionLink,
  codingSessionLinkGenerationId,
  parseCodingSessionLink,
} from "@/features/coding-sessions/lib/codingSessionLink";
import { parseChannelLink } from "@/features/messages/lib/channelLink";
import { BeekeeperInlineLink } from "./BeekeeperLinkChip";
import { ChannelDeepLinkAnchor } from "./ChannelDeepLink";

/** Render workspace navigation links before falling back to external anchors. */
export function renderWorkspaceDeepLink(
  props: React.ComponentPropsWithoutRef<"a"> & { interactive: boolean },
) {
  const { href, children, interactive } = props;
  if (!href) return null;
  const link = parseCodingSessionLink(href);
  if (link)
    return (
      <CodingSessionDeepLinkAnchor
        href={href}
        link={link}
        interactive={interactive}
      >
        {children}
      </CodingSessionDeepLinkAnchor>
    );
  return parseChannelLink(href).ok ? (
    <ChannelDeepLinkAnchor {...props} />
  ) : null;
}

/** Open a confirmed execution target in the existing session workspace. */
export function CodingSessionDeepLinkAnchor({
  children,
  href,
  link,
  interactive,
}: {
  children: React.ReactNode;
  href: string;
  link: CodingSessionLink;
  interactive: boolean;
}) {
  const { goCodingSession } = useAppNavigation();
  return (
    <BeekeeperInlineLink
      href={href}
      title={href}
      aria-label="Open coding session"
      interactive={interactive}
      onOpenLink={() => {
        void goCodingSession(
          link.channelId,
          codingSessionLinkGenerationId(link),
        );
      }}
    >
      {children}
    </BeekeeperInlineLink>
  );
}
