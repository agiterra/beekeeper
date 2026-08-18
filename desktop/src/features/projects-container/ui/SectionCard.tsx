import type * as React from "react";

/** Shared section shell for the project screen's cards (Coding sessions,
 * Members, Channels, …) so they all render with the same chrome. */
export function SectionCard({
  icon,
  title,
  count,
  action,
  children,
  testId,
}: {
  icon: React.ReactNode;
  title: string;
  count: number;
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
        <span className="text-2xs text-muted-foreground">{count}</span>
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
