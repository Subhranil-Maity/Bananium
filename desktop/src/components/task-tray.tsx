import { CheckCircle2, Download, Loader2, XCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { cn, formatBytes } from "@/lib/utils";
import { useTasks, type Task } from "@/stores/tasks";

/** A phase with no byte counts (e.g. preparing assets) is measured in steps. */
function isStepPhase(task: Task): boolean {
  return task.bytesTotal === null && task.bytesDone === 0;
}

function percent(task: Task): number {
  if (task.status === "completed") return 100;
  // No byte total (a step phase, or downloads with unpublished sizes):
  // fall back to counting files/steps rather than sitting at 0%.
  if (isStepPhase(task) || !task.bytesTotal) return task.filesTotal ? (task.filesDone / task.filesTotal) * 100 : 0;
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
    <div className="space-y-1.5 px-3 py-2.5">
      <div className="flex items-center gap-2 text-[13px]">
        {task.status === "running" && <Loader2 className="size-3.5 animate-spin text-primary" />}
        {task.status === "completed" && <CheckCircle2 className="size-3.5 text-success" />}
        {task.status === "failed" && <XCircle className="size-3.5 text-destructive" />}
        <span className="flex-1 truncate font-medium">{task.label}</span>
        {task.status === "running" && (
          <span className="text-xs text-muted-foreground tabular-nums">{Math.round(percent(task))}%</span>
        )}
      </div>
      <Progress value={percent(task)} className="h-1" />
      <div className={cn("truncate text-[11px] text-muted-foreground tabular-nums", task.status === "failed" && "text-destructive")}>
        {detail(task)}
      </div>
      {task.currentFile && (
        <div className="truncate font-mono text-[10px] text-muted-foreground/70">{task.currentFile}</div>
      )}
    </div>
  );
}

/** Top-bar popover listing every running and recently finished task. */
export function TaskTray() {
  const tasks = useTasks((s) => s.tasks);
  const clearFinished = useTasks((s) => s.clearFinished);
  const list = Object.values(tasks);
  const runningTasks = list.filter((t) => t.status === "running");
  const running = runningTasks.length;
  const overall = running ? runningTasks.reduce((n, t) => n + percent(t), 0) / running : 0;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-2 text-[13px]">
          {running > 0 ? (
            <>
              <Loader2 className="size-3.5 animate-spin text-primary" />
              <span className="tabular-nums">
                {running} task{running === 1 ? "" : "s"} · {Math.round(overall)}%
              </span>
              <Progress value={overall} className="h-1 w-16" />
            </>
          ) : (
            <>
              <Download className="size-3.5" /> Downloads
            </>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-96 p-0">
        <div className="flex h-9 items-center justify-between border-b px-3">
          <span className="text-xs font-semibold">Downloads & tasks</span>
          {list.length > running && (
            <Button variant="ghost" size="xs" onClick={clearFinished}>
              Clear finished
            </Button>
          )}
        </div>
        {list.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">Nothing running.</p>
        ) : (
          <div className="max-h-96 divide-y overflow-y-auto">
            {list.map((t) => (
              <TaskRow key={t.id} task={t} />
            ))}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
