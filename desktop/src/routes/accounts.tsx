import { useState } from "react";
import { Loader2, Star, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PlayerAvatar } from "@/components/player-avatar";
import {
  VALID_PLAYER_NAME,
  useAddProfile,
  useProfiles,
  useRemoveProfile,
  useSetDefaultProfile,
} from "@/hooks/use-profiles";
import { errorMessage } from "@/lib/api";

/** Manage offline accounts: add, choose the default, remove. */
export function AccountsPage() {
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
    <div className="max-w-2xl space-y-6">
      <div>
        <h1 className="text-2xl font-semibold">Accounts</h1>
        <p className="text-sm text-muted-foreground">
          Offline usernames. The starred account is who you play as.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Add account</CardTitle>
          <CardDescription>3–16 letters, digits, or underscores — the same rules vanilla uses.</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex gap-2" onSubmit={submit}>
            <Input
              placeholder="Username"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={invalid || duplicate}
              maxLength={16}
            />
            <Button type="submit" disabled={!trimmed || invalid || duplicate || add.isPending}>
              {add.isPending && <Loader2 className="animate-spin" />}
              Add
            </Button>
          </form>
          {invalid && <p className="mt-2 text-xs text-destructive">Not a valid Minecraft username.</p>}
          {duplicate && <p className="mt-2 text-xs text-destructive">That account already exists.</p>}
        </CardContent>
      </Card>

      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {isLoading && <Skeleton className="h-32" />}

      <div className="space-y-2">
        {profiles?.map((p) => (
          <div key={p.name} className="flex items-center gap-3 rounded-lg border bg-card p-3">
            <PlayerAvatar name={p.name} className="size-9" />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 font-medium">
                {p.name}
                {p.is_default && <Badge>Active</Badge>}
              </div>
              <div className="truncate font-mono text-xs text-muted-foreground select-text">{p.uuid}</div>
            </div>
            <Button
              variant="ghost"
              size="icon"
              title="Play as this account"
              disabled={p.is_default}
              onClick={() => setDefault.mutate(p.name)}
            >
              <Star className={p.is_default ? "fill-primary text-primary" : ""} />
            </Button>
            <Button variant="ghost" size="icon" title="Remove" onClick={() => remove.mutate(p.name)}>
              <Trash2 />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
