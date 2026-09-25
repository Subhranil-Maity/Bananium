import { create } from "zustand";

import type { Event } from "@/bindings/Event";

/** One long-running backend task, as tracked for the task tray. */
export interface Task {
  id: string;
  label: string;
  /** File currently in flight, from the latest per-file `progress` event. */
  currentFile: string | null;
  bytesDone: number;
  bytesTotal: number | null;
  bytesPerSec: number;
  filesDone: number;
  filesTotal: number;
  status: "running" | "completed" | "failed";
  error: string | null;
}

interface TasksState {
  tasks: Record<string, Task>;
  /** Fold one backend event into task state. Non-task events are ignored. */
  apply: (event: Event) => void;
  /** Drop every finished (completed or failed) task from the tray. */
  clearFinished: () => void;
}

function blank(id: string): Task {
  return {
    id,
    label: id,
    currentFile: null,
    bytesDone: 0,
    bytesTotal: null,
    bytesPerSec: 0,
    filesDone: 0,
    filesTotal: 0,
    status: "running",
    error: null,
  };
}

export const useTasks = create<TasksState>((set) => ({
  tasks: {},
  apply: (event) =>
    set(({ tasks }) => {
      if (event.event === "log" || event.event === "instance_exited") return { tasks };
      // Per-file progress ids are "<parent task>/<file>"; fold them into the parent.
      const id = event.task_id.split("/")[0];
      const prev = tasks[id] ?? blank(id);
      let next: Task;
      switch (event.event) {
        case "progress":
          next = { ...prev, status: "running", currentFile: event.label };
          break;
        case "overall_progress":
          next = {
            ...prev,
            status: "running",
            label: event.label,
            currentFile: event.current_file,
            bytesDone: event.bytes_done,
            bytesTotal: event.bytes_total,
            bytesPerSec: event.bytes_per_sec,
            filesDone: event.files_done,
            filesTotal: event.files_total,
          };
          break;
        case "task_completed":
          next = { ...prev, status: "completed", currentFile: null };
          break;
        case "task_failed":
          next = { ...prev, status: "failed", error: event.error, currentFile: null };
          break;
      }
      return { tasks: { ...tasks, [id]: next } };
    }),
  clearFinished: () =>
    set(({ tasks }) => ({
      tasks: Object.fromEntries(Object.entries(tasks).filter(([, t]) => t.status === "running")),
    })),
}));
