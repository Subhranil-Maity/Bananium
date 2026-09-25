import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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
