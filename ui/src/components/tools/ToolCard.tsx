import { cn } from "cn";
import type { JSX, ReactNode } from "react";

interface ToolCardProps {
  title: string;
  /** Right side of the header (a status or a small action). */
  aside?: ReactNode;
  className?: string;
  children: ReactNode;
}

/** A titled card in the tool panel. */
export function ToolCard({
  title,
  aside,
  className,
  children,
}: ToolCardProps): JSX.Element {
  return (
    <section className={cn("rounded-lg border border-border bg-card p-3", className)}>
      <header className="mb-2.5 flex items-center gap-2">
        <h3 className="text-[11px] font-semibold tracking-wider text-muted-foreground uppercase">
          {title}
        </h3>
        {aside && <div className="ml-auto flex items-center gap-1.5">{aside}</div>}
      </header>
      {children}
    </section>
  );
}

/** Centered message for a tab with nothing to show yet. */
export function ToolEmpty({ children }: { children: ReactNode }): JSX.Element {
  return (
    <div className="grid h-full place-items-center p-6 text-center text-muted-foreground">
      <div className="flex max-w-64 flex-col items-center gap-3">{children}</div>
    </div>
  );
}

/** One label and value pair in a stats list. */
export function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: ReactNode;
  hint?: string;
}): JSX.Element {
  return (
    <div className="flex items-baseline justify-between gap-2 py-0.5">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="font-mono tabular-nums">
        {value}
        {hint && <span className="ml-1.5 text-[11px] text-muted-foreground">{hint}</span>}
      </dd>
    </div>
  );
}
