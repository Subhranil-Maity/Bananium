import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type { ContentKind } from "@/bindings/ContentKind";
import { errorMessage, run } from "@/lib/api";
import { contentKey } from "@/lib/content";

export function useContent(instance: string) {
  return useQuery({
    queryKey: contentKey(instance),
    queryFn: async () => (await run({ command: "content_list", instance }, "content_listed")).entries,
  });
}

/** Install a Modrinth project (plus dependencies) into an instance. */
export function useInstallContent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { instance: string; kind: ContentKind; project: string; version?: string }) =>
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
    onSuccess: (out) => {
      const [root, ...deps] = out.installed;
      toast.success(`Installed ${root?.title ?? "content"}`, {
        description: deps.length ? `Also installed: ${deps.map((d) => d.title).join(", ")}` : undefined,
      });
      void queryClient.invalidateQueries({ queryKey: contentKey(out.instance) });
      void queryClient.invalidateQueries({ queryKey: ["modrinth-search"] });
    },
    onError: (err) => toast.error("Install failed", { description: errorMessage(err) }),
  });
}
