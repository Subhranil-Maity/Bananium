import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, FolderOpen, Trash2 } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { Screenshot } from "@/bindings/Screenshot";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { formatBytes } from "@/lib/utils";

const ALL = "__all__";

/**
 * Screenshots from every instance in one gallery. Images load through
 * Tauri's asset protocol, which the backend scopes to the instances folder.
 */
export function ScreenshotsPage({ instance: fixedInstance }: { instance?: string }) {
  const queryClient = useQueryClient();
  const { data: instances } = useInstances();
  const [filter, setFilter] = useState(ALL);
  const [viewing, setViewing] = useState<number | null>(null);
  const instance = fixedInstance ?? (filter === ALL ? null : filter);

  const { data: shots, isLoading, error } = useQuery({
    queryKey: ["screenshots", instance],
    queryFn: async () =>
      (await run({ command: "screenshot_list", instance }, "screenshot_listed")).screenshots,
  });

  const remove = useMutation({
    mutationFn: (s: Screenshot) => run({ command: "screenshot_delete", path: s.path }, "screenshot_deleted"),
    onSuccess: () => {
      toast.success("Screenshot deleted");
      setViewing(null);
      void queryClient.invalidateQueries({ queryKey: ["screenshots"] });
    },
    onError: (err) => toast.error("Couldn't delete", { description: errorMessage(err) }),
  });

  const current = viewing !== null ? shots?.[viewing] : undefined;
  const step = (delta: number) =>
    setViewing((v) => (v === null || !shots ? v : (v + delta + shots.length) % shots.length));

  return (
    <div className="space-y-6">
      {!fixedInstance && (
        <div className="flex items-center justify-between gap-2">
          <h1 className="text-2xl font-semibold">Screenshots</h1>
          <Select value={filter} onValueChange={setFilter}>
            <SelectTrigger className="w-56">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>All instances</SelectItem>
              {instances?.map((i) => (
                <SelectItem key={i.slug} value={i.slug}>
                  {i.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}
      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
        {isLoading && [0, 1, 2, 3].map((i) => <Skeleton key={i} className="aspect-video" />)}
        {shots?.map((s, i) => (
          <button
            key={s.path}
            className="group overflow-hidden rounded-lg border bg-muted text-left focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => setViewing(i)}
          >
            <img
              src={convertFileSrc(s.path)}
              alt={s.file_name}
              loading="lazy"
              className="aspect-video w-full object-cover transition-transform group-hover:scale-105"
            />
            <div className="truncate px-2 py-1 text-xs text-muted-foreground">
              {!fixedInstance && `${s.instance_name} · `}
              {new Date(s.taken_unix * 1000).toLocaleDateString()}
            </div>
          </button>
        ))}
      </div>
      {shots?.length === 0 && (
        <div className="rounded-lg border border-dashed p-12 text-center text-muted-foreground">
          No screenshots yet. Press F2 in game to take one.
        </div>
      )}

      <Dialog open={current !== undefined} onOpenChange={(o) => !o && setViewing(null)}>
        <DialogContent className="sm:max-w-5xl">
          {current && (
            <>
              <DialogTitle>{current.instance_name}</DialogTitle>
              <DialogDescription>
                {new Date(current.taken_unix * 1000).toLocaleString()} · {formatBytes(current.size)} · {current.file_name}
              </DialogDescription>
              <div className="relative">
                <img src={convertFileSrc(current.path)} alt={current.file_name} className="w-full rounded-md" />
                {shots && shots.length > 1 && (
                  <>
                    <Button variant="secondary" size="icon" className="absolute top-1/2 left-2 -translate-y-1/2" onClick={() => step(-1)}>
                      <ChevronLeft />
                    </Button>
                    <Button variant="secondary" size="icon" className="absolute top-1/2 right-2 -translate-y-1/2" onClick={() => step(1)}>
                      <ChevronRight />
                    </Button>
                  </>
                )}
              </div>
              <div className="flex justify-end gap-2">
                <Button variant="outline" onClick={() => void revealItemInDir(current.path)}>
                  <FolderOpen /> Show in folder
                </Button>
                <Button variant="destructive" disabled={remove.isPending} onClick={() => remove.mutate(current)}>
                  <Trash2 /> Delete
                </Button>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
