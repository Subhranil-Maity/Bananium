import { useState } from "react";
import { CloudOff, Loader2, RotateCw, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { errorMessage, isServiceUnavailable, logAction } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useService } from "@/stores/service";

/**
 * Shown while something is loading from Modrinth and the backend reports
 * it's retrying: "Modrinth is slow to respond — retrying (2/3)…", instead of
 * a skeleton that just sits there. Renders nothing otherwise.
 */
export function RetryingNotice({ loading, className }: { loading: boolean; className?: string }) {
  const notice = useService((s) => s.notice);
  if (!loading || !notice) return null;
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning",
        className,
      )}
    >
      <Loader2 className="size-3.5 shrink-0 animate-spin" />
      <span>
        {notice.reason} — retrying ({notice.attempt}/{notice.maxAttempts})…
      </span>
    </div>
  );
}

/**
 * A failed Modrinth request, with Retry and Close. A timeout or an
 * unreachable server reads "Modrinth didn't respond" with the raw error
 * tucked under Details; anything else shows its own message.
 */
export function ServiceError({
  error,
  where,
  onRetry,
  retrying,
  className,
}: {
  error: unknown;
  /** Named in the action log: which view the error was in. */
  where: string;
  onRetry: () => void;
  retrying?: boolean;
  className?: string;
}) {
  const [closed, setClosed] = useState<unknown>(null);
  if (!error || closed === error) return null;
  const unavailable = isServiceUnavailable(error);
  return (
    <div className={cn("rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2.5 text-xs", className)}>
      <div className="flex items-start gap-2">
        <CloudOff className="mt-px size-4 shrink-0 text-destructive" />
        <div className="min-w-0 flex-1 space-y-1">
          <p className="font-medium text-destructive">
            {unavailable ? "Modrinth didn't respond" : "Couldn't load this from Modrinth"}
          </p>
          <p className="text-muted-foreground">
            {unavailable
              ? "It may be down or your connection may be offline. Check your connection and try again."
              : errorMessage(error)}
          </p>
          {unavailable && (
            <details className="text-muted-foreground/80">
              <summary className="cursor-pointer select-none">Details</summary>
              <p className="mt-1 font-mono text-[10px] break-all select-text">{errorMessage(error)}</p>
            </details>
          )}
        </div>
        <div className="flex shrink-0 gap-1">
          <Button
            size="xs"
            variant="outline"
            disabled={retrying}
            onClick={() => {
              logAction("service_error_retry", { where });
              onRetry();
            }}
          >
            {retrying ? <Loader2 className="animate-spin" /> : <RotateCw />} Retry
          </Button>
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Close"
            onClick={() => {
              logAction("service_error_closed", { where });
              setClosed(error);
            }}
          >
            <X />
          </Button>
        </div>
      </div>
    </div>
  );
}
