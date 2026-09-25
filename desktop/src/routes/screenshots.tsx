import { useEffect, useState } from "react";
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
import { EmptyState, Page, PageHeader } from "@/components/page";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { formatBytes } from "@/lib/utils";

const ALL = "__all__";

function Gallery({ instance, showInstance }: { instance: string | null; showInstance: boolean }) {
  const queryClient = useQueryClient();
  const [viewing, setViewing] = useState<number | null>(null);

  const { data: shots, isLoading, error } = useQuery({
    queryKey: ["screenshots", instance],
    queryFn: async () => (await run({ command: "screenshot_list", instance }, "screenshot_listed")).screenshots,
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

  // Arrow keys page through the lightbox.
  useEffect(() => {
    if (viewing === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") step(-1);
      else if (e.key === "ArrowRight") step(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <>
      {error && <p className="mb-3 text-sm text-destructive">{errorMessage(error)}</p>}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2">
        {isLoading && [0, 1, 2, 3].map((i) => <Skeleton key={i} className="aspect-video" />)}
        {shots?.map((s, i) => (
          <button
            key={s.path}
            className="group relative overflow-hidden rounded-md bg-muted ring-1 ring-border outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => setViewing(i)}
          >
            <img
              src={convertFileSrc(s.path)}
              alt={s.file_name}
              loading="lazy"
              className="aspect-video w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
            />
            <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 bg-gradient-to-t from-black/75 to-transparent px-2 pt-6 pb-1.5 text-[11px] text-white/90 opacity-0 transition-opacity group-hover:opacity-100">
              <span className="truncate font-medium">{showInstance ? s.instance_name : s.file_name}</span>
              <span className="shrink-0 tabular-nums">{new Date(s.taken_unix * 1000).toLocaleDateString()}</span>
            </div>
          </button>
        ))}
      </div>
      {shots?.length === 0 && <EmptyState>No screenshots yet. Press F2 in game to take one.</EmptyState>}

      <Dialog open={current !== undefined} onOpenChange={(o) => !o && setViewing(null)}>
        <DialogContent className="gap-3 p-3 sm:max-w-6xl">
          {current && (
            <>
              <div className="flex items-baseline gap-3 px-1 pr-8">
                <DialogTitle className="text-sm">{current.instance_name}</DialogTitle>
                <DialogDescription className="truncate text-xs tabular-nums">
                  {new Date(current.taken_unix * 1000).toLocaleString()} · {formatBytes(current.size)} · {current.file_name}
                </DialogDescription>
              </div>
              <div className="relative overflow-hidden rounded-md bg-black">
                <img src={convertFileSrc(current.path)} alt={current.file_name} className="max-h-[75vh] w-full object-contain" />
                {shots && shots.length > 1 && (
                  <>
                    <Button
                      variant="secondary"
                      size="icon"
                      className="absolute top-1/2 left-2 -translate-y-1/2 opacity-80 hover:opacity-100"
                      onClick={() => step(-1)}
                      aria-label="Previous"
                    >
                      <ChevronLeft />
                    </Button>
                    <Button
                      variant="secondary"
                      size="icon"
                      className="absolute top-1/2 right-2 -translate-y-1/2 opacity-80 hover:opacity-100"
                      onClick={() => step(1)}
                      aria-label="Next"
                    >
                      <ChevronRight />
                    </Button>
                  </>
                )}
              </div>
              <div className="flex items-center justify-between gap-2 px-1">
                <span className="text-xs text-muted-foreground tabular-nums">
                  {viewing! + 1} / {shots?.length}
                </span>
                <div className="flex gap-2">
                  <Button variant="outline" onClick={() => void revealItemInDir(current.path)}>
                    <FolderOpen /> Show in folder
                  </Button>
                  <Button variant="destructive" disabled={remove.isPending} onClick={() => remove.mutate(current)}>
                    <Trash2 /> Delete
                  </Button>
                </div>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}

/**
 * Screenshots from every instance in one gallery (or one instance's, when
 * embedded in its page). Images load through Tauri's asset protocol, which
 * the backend scopes to the instances folder.
 */
export function ScreenshotsPage({ instance: fixedInstance }: { instance?: string }) {
  const { data: instances } = useInstances();
  const [filter, setFilter] = useState(ALL);

  if (fixedInstance) return <Gallery instance={fixedInstance} showInstance={false} />;

  return (
    <Page>
      <PageHeader title="Screenshots">
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
      </PageHeader>
      <Gallery instance={filter === ALL ? null : filter} showInstance={filter === ALL} />
    </Page>
  );
}
