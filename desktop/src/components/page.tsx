import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/** Standard page frame: consistent gutters and max width. */
export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("mx-auto w-full max-w-[1480px] px-5 pt-4 pb-8", className)}>{children}</div>;
}

/** A page's title row: title (+ optional count/subtitle) on the left, actions on the right. */
export function PageHeader({
  title,
  meta,
  children,
}: {
  title: ReactNode;
  meta?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="mb-4 flex min-h-9 flex-wrap items-center gap-x-3 gap-y-2">
      <h1 className="text-base font-semibold tracking-tight">{title}</h1>
      {meta && <span className="text-xs text-muted-foreground tabular-nums">{meta}</span>}
      <div className="ml-auto flex flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

/** Dashed empty-state box used inside lists. */
export function EmptyState({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn("rounded-lg border border-dashed px-6 py-10 text-center text-sm text-muted-foreground", className)}>
      {children}
    </div>
  );
}

/** A titled surface for grouping form sections (settings pages). */
export function Section({
  title,
  description,
  children,
  className,
  actions,
}: {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  className?: string;
  actions?: ReactNode;
}) {
  return (
    <section className={cn("rounded-lg border bg-card", className)}>
      <header className="flex items-start gap-3 border-b px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">{title}</h2>
          {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
        </div>
        {actions}
      </header>
      <div className="space-y-4 p-4">{children}</div>
    </section>
  );
}
