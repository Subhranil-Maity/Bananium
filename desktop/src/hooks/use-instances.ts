import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

import { errorMessage, run } from "@/lib/api";

export const INSTANCES_KEY = ["instances"] as const;

export function useInstances() {
  return useQuery({
    queryKey: INSTANCES_KEY,
    queryFn: async () => (await run({ command: "instance_list" }, "instance_listed")).instances,
    // Games started from the CLI/TUI aren't announced to this window, so a
    // slow poll backs up the `instance_exited` event for those.
    refetchInterval: 5000,
  });
}

export function useInstance(slug: string | undefined) {
  const query = useInstances();
  return { ...query, data: query.data?.find((i) => i.slug === slug) };
}

/** Launch as the active (default) account; `profile: null` means exactly that. */
export function useLaunch() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (slug: string) =>
      run({ command: "launch", instance: slug, profile: null, dry_run: false }, "launched"),
    onSuccess: (out) => {
      toast.success(`Launched ${out.instance}`, { description: `pid ${out.pid}` });
      void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
    },
    onError: (err) => toast.error("Launch failed", { description: errorMessage(err) }),
  });
}

export function useKill() {
  return useMutation({
    mutationFn: (slug: string) => run({ command: "instance_kill", instance: slug }, "instance_killed"),
    onError: (err) => toast.error("Couldn't stop the game", { description: errorMessage(err) }),
  });
}

/** Move an instance into a library group (`""` ungroups it). */
export function useSetGroup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { slug: string; group: string }) =>
      run(
        {
          command: "instance_set",
          instance: args.slug,
          ram_mb: null,
          jvm_args: null,
          java_path: null,
          group: args.group,
        },
        "instance_updated",
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: INSTANCES_KEY }),
    onError: (err) => toast.error("Couldn't change the group", { description: errorMessage(err) }),
  });
}

/** Set (`path`) or clear (`null`) an instance's custom icon. */
export function useSetIcon() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { slug: string; path: string | null }) =>
      run({ command: "instance_set_icon", instance: args.slug, path: args.path }, "instance_updated"),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: INSTANCES_KEY }),
    onError: (err) => toast.error("Couldn't change the icon", { description: errorMessage(err) }),
  });
}

/** Ask the user for an icon image; `null` if they cancel. */
export async function pickIconFile(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    title: "Choose an instance icon",
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
  });
  return typeof picked === "string" ? picked : null;
}
