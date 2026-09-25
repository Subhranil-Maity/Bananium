import { Check, ChevronDown, UserPlus } from "lucide-react";
import { useNavigate } from "react-router";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { PlayerAvatar } from "@/components/player-avatar";
import { useProfiles, useSetDefaultProfile } from "@/hooks/use-profiles";

/** Top-bar dropdown showing who you'll play as, with one-click switching. */
export function AccountSwitcher() {
  const navigate = useNavigate();
  const { data: profiles } = useProfiles();
  const setDefault = useSetDefaultProfile();
  const active = profiles?.find((p) => p.is_default);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-2">
          <PlayerAvatar name={active?.name ?? "Player"} className="size-6" />
          <span className="max-w-32 truncate">{active?.name ?? "Player"}</span>
          <ChevronDown className="size-4 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuLabel>Play as</DropdownMenuLabel>
        {profiles?.map((p) => (
          <DropdownMenuItem key={p.name} onSelect={() => !p.is_default && setDefault.mutate(p.name)}>
            <PlayerAvatar name={p.name} className="size-5" />
            <span className="flex-1 truncate">{p.name}</span>
            {p.is_default && <Check className="size-4" />}
          </DropdownMenuItem>
        ))}
        {profiles?.length === 0 && (
          <div className="px-2 py-1.5 text-xs text-muted-foreground">
            No accounts yet — "Player" is used until you add one.
          </div>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => navigate("/accounts")}>
          <UserPlus /> Manage accounts
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
