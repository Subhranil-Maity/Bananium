import { Ban, CheckCircle2, Clock, Download, Loader2, RotateCw, X, XCircle } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { errorMessage, isServiceUnavailable, logAction, run } from "@/lib/api";
import { cn, formatBytes } from "@/lib/utils";
import { isActive, retryKey, useTasks, type Task } from "@/stores/tasks";

/** A phase with no byte counts (e.g. preparing assets) is measured in steps. */
function isStepPhase(task: Task): boolean {
  return task.bytesTotal === null && task.bytesDone === 0;
}

function percent(task: Task): number {
  if (task.status === "completed") return 100;
  if (task.status === "queued") return 0;
  // No byte total (a step phase, or downloads with unpublished sizes):
  // fall back to counting files/steps rather than sitting at 0%.
  if (isStepPhase(task) || !task.bytesTotal) return task.filesTotal ? (task.filesDone / task.filesTotal) * 100 : 0;
  return Math.min(100, (task.bytesDone / task.bytesTotal) * 100);
}

function detail(task: Task): string {
  if (task.status === "failed") {
    return task.error && isServiceUnavailable(task.error)
      ? "Modrinth didn't respond. Check your connection and retry."
      : (task.error ?? "Failed");
  }
  if (task.status === "cancelled") return "Cancelled before it started";
  if (task.status === "queued") {
    const ahead = (task.position ?? 1) - 1;
    return ahead > 0 ? `Waiting to start · ${ahead} ahead` : "Waiting to start";
  }
  if (task.status === "completed") return "Done";
  const phase = task.phase && task.phase !== task.label ? `${task.phase} · ` : "";
  if (task.filesTotal === 0) return `${phase}Starting…`;
  if (isStepPhase(task)) return `${phase}${task.filesDone}/${task.filesTotal}`;
  return `${phase}${task.filesDone}/${task.filesTotal} files · ${formatBytes(task.bytesDone)}${
    task.bytesTotal ? ` / ${formatBytes(task.bytesTotal)}` : ""
  } · ${formatBytes(Math.round(task.bytesPerSec))}/s`;
}

function TaskRow({ task }: { task: Task }) {
  const dismiss = useTasks((s) => s.dismiss);
  const retry = useTasks((s) => (task.kind ? s.retries[retryKey(task.kind, task.instance, task.project)] : undefined));
  const active = isActive(task);

  async function cancel() {
    logAction("task_cancel_clicked", { task: task.id });
    try {
      await run({ command: "task_cancel", task_id: task.id }, "task_cancelled");
    } catch (err) {
      toast.error("Couldn't cancel", { description: errorMessage(err) });
    }
  }

  return (
    <div className="space-y-1.5 px-3 py-2.5">
      <div className="flex items-center gap-2 text-[13px]">
        {(task.status === "running" || task.status === "retrying") && (
          <Loader2 className={cn("size-3.5 animate-spin", task.status === "retrying" ? "text-warning" : "text-primary")} />
        )}
        {task.status === "queued" && <Clock className="size-3.5 text-muted-foreground" />}
        {task.status === "completed" && <CheckCircle2 className="size-3.5 text-success" />}
        {task.status === "failed" && <XCircle className="size-3.5 text-destructive" />}
        {task.status === "cancelled" && <Ban className="size-3.5 text-muted-foreground" />}
        <span className="flex-1 truncate font-medium">{task.label}</span>
        {(task.status === "running" || task.status === "retrying") && (
          <span className="text-xs text-muted-foreground tabular-nums">{Math.round(percent(task))}%</span>
        )}
        {task.status === "queued" && (
          <Button size="xs" variant="ghost" onClick={() => void cancel()}>
            Cancel
          </Button>
        )}
        {task.status === "failed" && retry && (
          <Button
            size="xs"
            variant="outline"
            onClick={() => {
              logAction("task_retry_clicked", { task: task.id, kind: task.kind });
              dismiss(task.id);
              retry();
            }}
          >
            <RotateCw /> Retry
          </Button>
        )}
        {!active && (
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Close"
            onClick={() => {
              logAction("task_dismissed", { task: task.id, status: task.status });
              dismiss(task.id);
            }}
          >
            <X />
          </Button>
        )}
      </div>
      {task.status !== "cancelled" && (
        <Progress value={percent(task)} className={cn("h-1", task.status === "queued" && "opacity-40")} />
      )}
      <div
        className={cn(
          "truncate text-[11px] text-muted-foreground tabular-nums",
          task.status === "failed" && "whitespace-normal text-destructive",
        )}
        title={task.error ?? undefined}
      >
        {detail(task)}
      </div>
      {task.notice && (
        <div className="flex items-center gap-1.5 rounded bg-warning/10 px-2 py-1 text-[11px] text-warning">
          <RotateCw className="size-3 shrink-0" />
          <span className="truncate">{task.notice}</span>
        </div>
      )}
      {task.currentFile && active && (
        <div className="truncate font-mono text-[10px] text-muted-foreground/70">{task.currentFile}</div>
      )}
    </div>
  );
}

/** Top-bar popover listing every queued, running and recently finished task. */
export function TaskTray() {
  const tasks = useTasks((s) => s.tasks);
  const clearFinished = useTasks((s) => s.clearFinished);
  const list = Object.values(tasks);
  const activeTasks = list.filter(isActive);
  const runningTasks = activeTasks.filter((t) => t.status !== "queued");
  const queued = activeTasks.length - runningTasks.length;
  const overall = runningTasks.length
    ? runningTasks.reduce((n, t) => n + percent(t), 0) / runningTasks.length
    : 0;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-2 text-[13px]">
          {activeTasks.length > 0 ? (
            <>
              {runningTasks.length > 0 ? (
                <Loader2 className="size-3.5 animate-spin text-primary" />
              ) : (
                <Clock className="size-3.5 text-muted-foreground" />
              )}
              <span className="tabular-nums">
                {runningTasks.length > 0
                  ? `${runningTasks.length} task${runningTasks.length === 1 ? "" : "s"} · ${Math.round(overall)}%`
                  : "Queued"}
                {queued > 0 && runningTasks.length > 0 && ` · ${queued} queued`}
              </span>
              {runningTasks.length > 0 && <Progress value={overall} className="h-1 w-16" />}
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
          {list.length > activeTasks.length && (
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
