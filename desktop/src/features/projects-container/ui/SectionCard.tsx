import type * as React from "react";

/** Shared section shell for the project screen's cards (Coding sessions,
 * Members, Channels, …) so they all render with the same chrome. */
export function SectionCard({
  icon,
  title,
  count,
  countLabel,
  action,
  children,
  testId,
}: {
  icon: React.ReactNode;
  title: string;
  count: number;
  /**
   * What the count is a count OF, when it is scoped to something narrower
   * than "in this project" — e.g. "8 on this computer". A bare number over a
   * scoped set is a claim the card cannot keep (ledger 207(3)).
   */
  countLabel?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
  testId?: string;
}) {
  return (
    <section
      className="rounded-lg border border-border bg-card p-4"
      data-testid={testId}
    >
      <div className="mb-3 flex items-center gap-2 text-sm font-medium text-foreground">
        {icon}
        <span>{title}</span>
        <span className="text-2xs text-muted-foreground">
          {countLabel ?? count}
        </span>
        <span className="flex-1" />
        {action}
      </div>
      {children}
    </section>
  );
}

export function EmptyHint({ children }: { children: React.ReactNode }) {
  return <p className="text-sm text-muted-foreground">{children}</p>;
}
