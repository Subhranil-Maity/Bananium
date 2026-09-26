import { useState } from "react";
import { useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, ScrollText, TriangleAlert } from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { run } from "@/lib/api";

/**
 * Tells the user when the previous run crashed or didn't close properly,
 * with the one-line reason and a way to the log. The check runs once in the
 * background after the app has rendered; nothing waits on it.
 */
export function LastSessionNotice() {
  const navigate = useNavigate();
  const [dismissed, setDismissed] = useState(false);
  const { data: log } = useQuery({
    queryKey: ["launcher-last-session"],
    queryFn: async () => (await run({ command: "launcher_last_session" }, "launcher_last_session")).log,
    staleTime: Infinity,
  });
  const bad = log && (log.status === "crashed" || log.status === "unclean");
  if (!bad || dismissed) return null;

  return (
    <Dialog open onOpenChange={(open) => !open && setDismissed(true)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <TriangleAlert className="size-4 text-amber-500" />
            {log.status === "crashed" ? "Bananium crashed last time" : "Bananium didn't close properly last time"}
          </DialogTitle>
          <DialogDescription>
            If this keeps happening or looks like a bug, please share this log file with the developer.
          </DialogDescription>
        </DialogHeader>
        <p className="rounded-md border bg-muted/50 px-3 py-2 font-mono text-xs break-words select-text">
          {log.reason}
        </p>
        <p className="truncate font-mono text-[11px] text-muted-foreground" title={log.path}>
          {log.path}
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={() => setDismissed(true)}>
            Dismiss
          </Button>
          <Button variant="outline" onClick={() => void revealItemInDir(log.path)}>
            <FolderOpen /> Show in folder
          </Button>
          <Button
            onClick={() => {
              setDismissed(true);
              void navigate(`/console?file=${encodeURIComponent(log.name)}`);
            }}
          >
            <ScrollText /> View log
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
