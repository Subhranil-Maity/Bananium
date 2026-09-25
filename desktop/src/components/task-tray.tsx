import { CheckCircle2, Download, Loader2, XCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { formatBytes } from "@/lib/utils";
import { useTasks, type Task } from "@/stores/tasks";

/** A phase with no byte counts (e.g. preparing assets) is measured in steps. */
function isStepPhase(task: Task): boolean {
  return task.bytesTotal === null && task.bytesDone === 0;
}

function percent(task: Task): number {
  if (task.status === "completed") return 100;
  if (isStepPhase(task)) return task.filesTotal ? (task.filesDone / task.filesTotal) * 100 : 0;
  if (!task.bytesTotal) return 0;
  return Math.min(100, (task.bytesDone / task.bytesTotal) * 100);
}

function detail(task: Task): string {
  if (task.status === "failed") return task.error ?? "Failed";
  if (task.status === "completed") return `Done · ${task.filesTotal} files`;
  if (isStepPhase(task)) return `${task.filesDone}/${task.filesTotal}`;
  return `${task.filesDone}/${task.filesTotal} files · ${formatBytes(task.bytesDone)}${
    task.bytesTotal ? ` / ${formatBytes(task.bytesTotal)}` : ""
  } · ${formatBytes(Math.round(task.bytesPerSec))}/s`;
}

function TaskRow({ task }: { task: Task }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-2 text-sm">
        {task.status === "running" && <Loader2 className="size-4 animate-spin" />}
        {task.status === "completed" && <CheckCircle2 className="size-4 text-green-600" />}
        {task.status === "failed" && <XCircle className="size-4 text-destructive" />}
        <span className="truncate font-medium">{task.label}</span>
      </div>
      <Progress value={percent(task)} />
      <div className="truncate text-xs text-muted-foreground">{detail(task)}</div>
      {task.currentFile && (
        <div className="truncate font-mono text-[11px] text-muted-foreground">{task.currentFile}</div>
      )}
    </div>
  );
}

/** Top-bar popover listing every running and recently finished task. */
export function TaskTray() {
  const tasks = useTasks((s) => s.tasks);
  const clearFinished = useTasks((s) => s.clearFinished);
  const list = Object.values(tasks);
  const running = list.filter((t) => t.status === "running").length;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-2">
          {running > 0 ? <Loader2 className="size-4 animate-spin" /> : <Download className="size-4" />}
          {running > 0 ? `${running} running` : "Tasks"}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-96 space-y-4">
        {list.length === 0 ? (
          <p className="text-sm text-muted-foreground">No tasks yet.</p>
        ) : (
          <>
            {list.map((t) => (
              <TaskRow key={t.id} task={t} />
            ))}
            {list.length > running && (
              <Button variant="outline" size="sm" className="w-full" onClick={clearFinished}>
                Clear finished
              </Button>
            )}
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}
