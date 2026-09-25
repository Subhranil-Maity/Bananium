import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { errorMessage, run } from "@/lib/api";

export const JAVA_LIST_KEY = ["java-list"] as const;

/**
 * Downloaded Mojang runtimes plus every JVM detected on this machine.
 * Detection runs `java -version` per candidate, so it's cached until the
 * user rescans.
 */
export function useJavaList() {
  return useQuery({
    queryKey: JAVA_LIST_KEY,
    queryFn: async () => (await run({ command: "java_list" }, "java_listed")).installs,
    staleTime: Infinity,
  });
}

/** The Mojang runtime an instance uses by default, and whether it's downloaded. */
export function useInstanceJava(slug: string) {
  return useQuery({
    queryKey: ["instance-java", slug],
    queryFn: () => run({ command: "instance_java", instance: slug }, "instance_java_shown"),
    staleTime: 60_000,
  });
}

export function useRemoveRuntime() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (component: string) =>
      run({ command: "java_runtime_remove", component }, "java_runtime_removed"),
    onSuccess: (out) => {
      toast.success(`Removed ${out.component}`, { description: "It downloads again when an instance needs it." });
      void queryClient.invalidateQueries({ queryKey: JAVA_LIST_KEY });
      void queryClient.invalidateQueries({ queryKey: ["instance-java"] });
    },
    onError: (err) => toast.error("Couldn't remove the runtime", { description: errorMessage(err) }),
  });
}
