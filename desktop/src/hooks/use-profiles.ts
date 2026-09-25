import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { errorMessage, run } from "@/lib/api";

export const PROFILES_KEY = ["profiles"] as const;

/** Mirrors `bananium_launch::is_valid_player_name`. */
export const VALID_PLAYER_NAME = /^[A-Za-z0-9_]{3,16}$/;

export function useProfiles() {
  return useQuery({
    queryKey: PROFILES_KEY,
    queryFn: async () => (await run({ command: "profile_list" }, "profile_listed")).profiles,
  });
}

/**
 * The active account is simply the backend's default profile, so the GUI,
 * CLI, and TUI all agree on who `launch` plays as.
 */
export function useSetDefaultProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => run({ command: "profile_set_default", name }, "profile_default_set"),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: PROFILES_KEY }),
    onError: (err) => toast.error("Couldn't switch account", { description: errorMessage(err) }),
  });
}

export function useAddProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => run({ command: "profile_add", name }, "profile_added"),
    onSuccess: (out) => {
      toast.success(`Added ${out.profile.name}`);
      void queryClient.invalidateQueries({ queryKey: PROFILES_KEY });
    },
    onError: (err) => toast.error("Couldn't add account", { description: errorMessage(err) }),
  });
}

export function useRemoveProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => run({ command: "profile_remove", name }, "profile_removed"),
    onSuccess: (out) => {
      toast.success(`Removed ${out.name}`);
      void queryClient.invalidateQueries({ queryKey: PROFILES_KEY });
    },
    onError: (err) => toast.error("Couldn't remove account", { description: errorMessage(err) }),
  });
}
