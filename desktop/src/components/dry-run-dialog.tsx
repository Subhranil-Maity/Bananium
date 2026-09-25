import { useQuery } from "@tanstack/react-query";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage, run } from "@/lib/api";

/** Shows the exact command line a launch would run, without running it. */
export function DryRunDialog({
  slug,
  open,
  onOpenChange,
}: {
  slug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { data, isLoading, error } = useQuery({
    queryKey: ["dry-run", slug],
    queryFn: () =>
      run({ command: "launch", instance: slug, profile: null, dry_run: true }, "launch_planned"),
    enabled: open,
    staleTime: 0,
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Launch command</DialogTitle>
          <DialogDescription>Dry run — nothing is executed.</DialogDescription>
        </DialogHeader>
        {isLoading && <Skeleton className="h-40" />}
        {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
        {data && (
          <ScrollArea className="h-80 rounded-md border bg-muted p-3">
            <pre className="font-mono text-xs break-all whitespace-pre-wrap select-text">{data.command_line}</pre>
          </ScrollArea>
        )}
      </DialogContent>
    </Dialog>
  );
}
