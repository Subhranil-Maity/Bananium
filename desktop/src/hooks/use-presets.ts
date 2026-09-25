import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type { ContentKind } from "@/bindings/ContentKind";
import { errorMessage, run } from "@/lib/api";
import { contentKey } from "@/lib/content";

export const PRESETS_KEY = ["presets"] as const;

export function usePresets() {
  return useQuery({
    queryKey: PRESETS_KEY,
    queryFn: async () => (await run({ command: "preset_list" }, "preset_listed")).presets,
  });
}

export function useSavePreset() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { instance: string; name: string; kinds: ContentKind[] }) =>
      run({ command: "preset_save", ...args }, "preset_saved"),
    onSuccess: (out) => {
      toast.success(`Saved preset "${out.preset.name}"`, {
        description:
          `${out.preset.entries.length} item(s)` +
          (out.skipped_local ? ` · ${out.skipped_local} local file(s) not included` : ""),
      });
      void queryClient.invalidateQueries({ queryKey: PRESETS_KEY });
    },
    onError: (err) => toast.error("Couldn't save preset", { description: errorMessage(err) }),
  });
}

/**
 * Apply a preset. Entries without a compatible version are reported, not
 * fatal, so the result toast lists what was skipped and why.
 */
export function useApplyPreset() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { preset: string; instance: string }) =>
      run({ command: "preset_apply", ...args }, "preset_applied"),
    onSuccess: (out) => {
      const skipped = out.skipped.map((s) => `${s.title} (${s.reason})`).join(", ");
      toast.success(`Installed ${out.applied.length} item(s)`, {
        description: skipped ? `Skipped: ${skipped}` : undefined,
        duration: skipped ? 10_000 : undefined,
      });
      void queryClient.invalidateQueries({ queryKey: contentKey(out.instance) });
    },
    onError: (err) => toast.error("Couldn't apply preset", { description: errorMessage(err) }),
  });
}
