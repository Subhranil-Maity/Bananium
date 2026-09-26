import { create } from "zustand";

import type { Event } from "@/bindings/Event";
import type { TaskInfo } from "@/bindings/TaskInfo";
import type { TaskKind } from "@/bindings/TaskKind";

/** One long-running backend task, as tracked for the task tray. */
export interface Task {
  id: string;
  kind: TaskKind | null;
  /** What the task is ("Installing modpack FO"), from when it was queued. */
  label: string;
  /** The step it's on ("Minecraft 26.2 + Fabric 0.19.5", "Placing modpack files"). */
  phase: string | null;
  /** Instance and Modrinth project the task works on, when it has them. */
  instance: string | null;
  project: string | null;
  /** File currently in flight, from the latest per-file `progress` event. */
  currentFile: string | null;
  bytesDone: number;
  bytesTotal: number | null;
  bytesPerSec: number;
  filesDone: number;
  filesTotal: number;
  status: "queued" | "running" | "retrying" | "completed" | "failed" | "cancelled";
  /** 1-based place in the waiting line while queued. */
  position: number | null;
  /** Why a request is being retried ("Modrinth didn't respond — retrying 2/3"). */
  notice: string | null;
  error: string | null;
}

/** Tasks that are still going: queued, running, or retrying. */
export function isActive(task: Task): boolean {
  return task.status === "queued" || task.status === "running" || task.status === "retrying";
}

/** The key a task's retry action is filed under: what it does, not its id (a retry is a new task). */
export function retryKey(kind: TaskKind, instance: string | null, project: string | null): string {
  return `${kind}:${instance ?? ""}:${project ?? ""}`;
}

interface TasksState {
  tasks: Record<string, Task>;
  /** How to run a failed task again, by `retryKey`, registered by whatever started it. */
  retries: Record<string, () => void>;
  /** Fold one backend event into task state. Non-task events are ignored. */
  apply: (event: Event) => void;
  /** Replace the tray with the backend's queue (at startup, after a reload). */
  seed: (tasks: TaskInfo[]) => void;
  registerRetry: (key: string, retry: () => void) => void;
  /** Drop one finished task from the tray. */
  dismiss: (id: string) => void;
  /** Drop every finished (completed, failed or cancelled) task from the tray. */
  clearFinished: () => void;
}

function blank(id: string): Task {
  return {
    id,
    kind: null,
    label: id,
    phase: null,
    instance: null,
    project: null,
    currentFile: null,
    bytesDone: 0,
    bytesTotal: null,
    bytesPerSec: 0,
    filesDone: 0,
    filesTotal: 0,
    status: "running",
    position: null,
    notice: null,
    error: null,
  };
}

export const useTasks = create<TasksState>((set) => ({
  tasks: {},
  retries: {},
  apply: (event) =>
    set(({ tasks }) => {
      if (
        event.event === "log" ||
        event.event === "instance_exited" ||
        event.event === "instance_launched" ||
        event.event === "presence_status_changed" ||
        event.event === "service_retrying"
      )
        return { tasks };
      // Per-file progress ids are "<parent task>/<file>"; fold them into the parent.
      const id = event.task_id.split("/")[0];
      const prev = tasks[id] ?? blank(id);
      let next: Task;
      switch (event.event) {
        case "task_queued":
          next = {
            ...prev,
            kind: event.kind,
            label: event.label,
            instance: event.instance,
            project: event.project,
            status: "queued",
            position: event.position,
          };
          break;
        case "task_started":
          next = { ...prev, status: "running", position: null };
          break;
        case "task_retrying":
          next = {
            ...prev,
            status: "retrying",
            notice: `${event.reason} — retrying (${event.attempt}/${event.max_attempts})`,
          };
          break;
        case "progress":
          next = { ...prev, status: "running", notice: null, currentFile: event.label };
          break;
        case "overall_progress":
          next = {
            ...prev,
            status: "running",
            notice: null,
            // A task announced by `task_queued` keeps its name; the step
            // label goes underneath it.
            label: prev.kind ? prev.label : event.label,
            phase: event.label,
            currentFile: event.current_file,
            bytesDone: event.bytes_done,
            bytesTotal: event.bytes_total,
            bytesPerSec: event.bytes_per_sec,
            filesDone: event.files_done,
            filesTotal: event.files_total,
          };
          break;
        case "task_completed":
          next = { ...prev, status: "completed", currentFile: null, notice: null };
          break;
        case "task_failed":
          next = { ...prev, status: "failed", error: event.error, currentFile: null, notice: null };
          break;
        case "task_cancelled":
          next = { ...prev, status: "cancelled", currentFile: null, notice: null, position: null };
          break;
      }
      return { tasks: { ...tasks, [id]: next } };
    }),
  seed: (list) =>
    set(({ tasks }) => {
      const next = { ...tasks };
      for (const t of list) {
        next[t.task_id] = {
          ...(next[t.task_id] ?? blank(t.task_id)),
          kind: t.kind,
          label: t.label,
          phase: t.phase,
          instance: t.instance,
          project: t.project,
          status: t.state === "queued" ? "queued" : "running",
          position: t.position,
        };
      }
      return { tasks: next };
    }),
  registerRetry: (key, retry) => set(({ retries }) => ({ retries: { ...retries, [key]: retry } })),
  dismiss: (id) =>
    set(({ tasks }) => {
      const next = { ...tasks };
      delete next[id];
      return { tasks: next };
    }),
  clearFinished: () =>
    set(({ tasks }) => ({
      tasks: Object.fromEntries(Object.entries(tasks).filter(([, t]) => isActive(t))),
    })),
}));

/**
 * The queued or running task of `kind` on `instance` (and `project`, when
 * given) — how every Install/Identify button knows it's already in progress,
 * whichever component started it and even after that one unmounted.
 */
export function useActiveTask(kind: TaskKind, instance: string | null, project?: string | null): Task | undefined {
  return useTasks((s) =>
    Object.values(s.tasks).find(
      (t) =>
        isActive(t) &&
        t.kind === kind &&
        (instance === null || t.instance === instance) &&
        (project === undefined || t.project === project),
    ),
  );
}
