import { useMutation, useMutationState, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type { ContentKind } from "@/bindings/ContentKind";
import { errorMessage, isAlreadyQueued, logAction, run } from "@/lib/api";
import { contentKey } from "@/lib/content";
import { retryKey, useActiveTask, useTasks } from "@/stores/tasks";

export function useContent(instance: string) {
  return useQuery({
    queryKey: contentKey(instance),
    queryFn: async () => (await run({ command: "content_list", instance }, "content_listed")).entries,
  });
}

/** Everything a content install needs; `version` pins one exact version. */
export interface InstallArgs {
  instance: string;
  kind: ContentKind;
  project: string;
  version?: string;
}

const INSTALL_KEY = ["content-install"];

/**
 * Install a Modrinth project (plus dependencies) into an instance. Shares
 * one mutation key app-wide so [`useInstallState`] can tell, from anywhere,
 * that this project is already on its way into this instance.
 */
export function useInstallContent() {
  const queryClient = useQueryClient();
  const registerRetry = useTasks((s) => s.registerRetry);
  const mutation = useMutation({
    mutationKey: INSTALL_KEY,
    mutationFn: (args: InstallArgs) =>
      run(
        {
          command: "content_install",
          instance: args.instance,
          kind: args.kind,
          project: args.project,
          version: args.version ?? null,
        },
        "content_installed",
      ),
    onMutate: (args) => {
      // The tray's Retry on a failed task re-runs exactly this request.
      registerRetry(retryKey("content_install", args.instance, args.project), () => mutation.mutate(args));
    },
    onSuccess: (out) => {
      const [root, ...deps] = out.installed;
      toast.success(`Installed ${root?.title ?? "content"}`, {
        description: deps.length ? `Also installed: ${deps.map((d) => d.title).join(", ")}` : undefined,
      });
      void queryClient.invalidateQueries({ queryKey: contentKey(out.instance) });
    },
    onError: (err, args) => {
      if (isAlreadyQueued(err)) {
        toast.info("Already installing", { description: "It's in the task tray." });
        return;
      }
      toast.error("Install failed", {
        description: errorMessage(err),
        closeButton: true,
        duration: 15_000,
        action: { label: "Retry", onClick: () => mutation.mutate(args) },
      });
    },
  });
  return mutation;
}

/**
 * Where an install of `project` into `instance` stands, wherever it was
 * started: `pending` while the request is on its way to the queue or the
 * task is queued or running, `queued` while it waits its turn. Buttons use
 * this rather than their own mutation's `isPending`, which a remount (or a
 * second button for the same project) knows nothing about.
 */
export function useInstallState(instance: string | null, project: string | null) {
  const task = useActiveTask("content_install", instance, project);
  const sending = useMutationState({
    filters: {
      mutationKey: INSTALL_KEY,
      status: "pending",
      predicate: (m) => {
        const v = m.state.variables as InstallArgs | undefined;
        return v?.instance === instance && v?.project === project;
      },
    },
    select: (m) => (m.state.variables as InstallArgs | undefined)?.version ?? null,
  });
  return {
    pending: !!task || sending.length > 0,
    queued: task?.status === "queued",
    /** The exact versions being installed, when the request picked one. */
    versions: sending,
  };
}

/** Run `mutate` unless `pending`: the first line of every Install click, so a double-click never sends twice. */
export function guardedInstall(pending: boolean, fields: Record<string, string | null | undefined>, go: () => void) {
  if (pending) {
    logAction("install_ignored", { ...fields, reason: "already in progress" });
    return;
  }
  logAction("install_clicked", fields);
  go();
}
