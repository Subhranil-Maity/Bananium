import { useState } from "react";
import { Check, Loader2, Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState, Page, PageHeader } from "@/components/page";
import { PlayerAvatar } from "@/components/player-avatar";
import {
  VALID_PLAYER_NAME,
  useAddProfile,
  useProfiles,
  useRemoveProfile,
  useSetDefaultProfile,
} from "@/hooks/use-profiles";
import { errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";
import { usePresenceView } from "@/lib/presence";

/** Manage offline accounts: add, choose who you play as, remove. */
export function AccountsPage() {
  usePresenceView({ view: "accounts" });
  const { data: profiles, isLoading, error } = useProfiles();
  const add = useAddProfile();
  const remove = useRemoveProfile();
  const setDefault = useSetDefaultProfile();
  const [name, setName] = useState("");

  const trimmed = name.trim();
  const invalid = trimmed !== "" && !VALID_PLAYER_NAME.test(trimmed);
  const duplicate = profiles?.some((p) => p.name.toLowerCase() === trimmed.toLowerCase()) ?? false;

  function submit(e: React.FormEvent) {
    e.preventDefault();
    add.mutate(trimmed, { onSuccess: () => setName("") });
  }

  return (
    <Page className="max-w-3xl">
      <PageHeader title="Accounts" meta="Offline usernames — the selected one is who you play as" />

      <form className="mb-1 flex gap-2" onSubmit={submit}>
        <Input
          placeholder="Add a username (3–16 letters, digits or _)"
          value={name}
          onChange={(e) => setName(e.target.value)}
          aria-invalid={invalid || duplicate}
          maxLength={16}
        />
        <Button type="submit" disabled={!trimmed || invalid || duplicate || add.isPending}>
          {add.isPending ? <Loader2 className="animate-spin" /> : <Plus />}
          Add
        </Button>
      </form>
      <div className="mb-4 h-4 text-xs text-destructive">
        {invalid && "Not a valid Minecraft username."}
        {duplicate && "That account already exists."}
      </div>

      {error && <p className="mb-3 text-sm text-destructive">{errorMessage(error)}</p>}
      {isLoading && <Skeleton className="h-32" />}
      {profiles?.length === 0 && <EmptyState>No accounts yet — "Player" is used until you add one.</EmptyState>}

      {profiles && profiles.length > 0 && (
        <div className="overflow-hidden rounded-lg border bg-card">
          {profiles.map((p) => (
            <div
              key={p.name}
              className={cn(
                "group flex items-center gap-3 border-b px-3 py-2 last:border-b-0",
                p.is_default ? "bg-primary/[0.05]" : "hover:bg-accent/40",
              )}
            >
              <PlayerAvatar name={p.name} uuid={p.uuid} className="size-9" />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-[13px] font-semibold">
                  {p.name}
                  {p.is_default && (
                    <span className="flex items-center gap-1 rounded bg-primary/15 px-1.5 py-px text-[10px] font-semibold tracking-wide text-primary uppercase">
                      <Check className="size-3" /> Playing as
                    </span>
                  )}
                </div>
                <div className="truncate font-mono text-[11px] text-muted-foreground select-text">{p.uuid}</div>
              </div>
              {!p.is_default && (
                <Button variant="outline" size="sm" onClick={() => setDefault.mutate(p.name)}>
                  Use this account
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-muted-foreground hover:text-destructive"
                title="Remove"
                onClick={() => remove.mutate(p.name)}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      )}
    </Page>
  );
}
