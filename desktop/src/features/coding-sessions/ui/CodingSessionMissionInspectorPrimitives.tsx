import type * as React from "react";

/**
 * The two leaves every Inspector section renders: an empty-state sentence and
 * a signed-source disclosure.
 *
 * Lifted out of `CodingSessionMissionInspector.tsx` when that file passed the
 * repository's 1,000-line ceiling and its plan renderers moved to a sibling —
 * the sibling needs both of these, and importing them back from the Inspector
 * would make the two files a cycle. Neither leaf changed.
 */
export function EmptyCopy({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}

/**
 * The signed event behind a claim, behind a disclosure.
 *
 * Every fact this surface states names the event it came from. Collapsed by
 * default because a panel of 64-hex ids is unreadable, and present on every
 * claim because a fact with no source is a fact this app is asserting on its
 * own authority.
 */
export function SignedSource({
  authorLabel,
  eventId,
}: {
  authorLabel?: string;
  eventId: string;
}) {
  return (
    <details className="mt-1 text-2xs text-muted-foreground">
      <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        Signed source
      </summary>
      <dl className="mt-1 space-y-1">
        {authorLabel ? (
          <div>
            <dt className="font-medium">Author</dt>
            <dd>
              <code className="block break-all">{authorLabel}</code>
            </dd>
          </div>
        ) : null}
        <div>
          <dt className="font-medium">Event</dt>
          <dd>
            <code className="block break-all">{eventId}</code>
          </dd>
        </div>
      </dl>
    </details>
  );
}
