import { useEffect } from "react";
import { useNavigate } from "react-router";
import { Play, Square } from "lucide-react";
import { create } from "zustand";

import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { InstanceIcon } from "@/components/instance-icon";
import { NAV } from "@/components/nav";
import { useInstances, useKill, useLaunch } from "@/hooks/use-instances";
import { loaderLabel, sortInstances } from "@/lib/instances";

export const usePalette = create<{ open: boolean; setOpen: (open: boolean) => void }>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));

/** Ctrl+K: jump to or launch any instance, or go to any page. */
export function CommandPalette() {
  const { open, setOpen } = usePalette();
  const navigate = useNavigate();
  const { data: instances } = useInstances();
  const launch = useLaunch();
  const kill = useKill();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setOpen(!usePalette.getState().open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);

  const go = (fn: () => void) => {
    setOpen(false);
    fn();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Command palette" description="Jump to an instance or page">
      <CommandInput placeholder="Search instances and pages…" />
      <CommandList>
        <CommandEmpty>Nothing matches.</CommandEmpty>
        {instances && instances.length > 0 && (
          <CommandGroup heading="Instances">
            {sortInstances(instances, "played").map((i) => (
              <CommandItem
                key={i.slug}
                value={`${i.name} ${i.mc_version} ${loaderLabel(i)} ${i.group ?? ""}`}
                onSelect={() => go(() => navigate(`/instance/${i.slug}`))}
              >
                <InstanceIcon instance={i} className="size-6" />
                <span className="truncate">{i.name}</span>
                <span className="text-xs text-muted-foreground">
                  {loaderLabel(i)} {i.mc_version}
                </span>
                <button
                  className="ml-auto flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-background hover:text-foreground"
                  onClick={(e) => {
                    e.stopPropagation();
                    go(() => (i.running ? kill.mutate(i.slug) : launch.mutate(i.slug)));
                  }}
                >
                  {i.running ? <Square className="size-3" /> : <Play className="size-3" />}
                  {i.running ? "Stop" : "Play"}
                </button>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        <CommandGroup heading="Go to">
          {NAV.map(({ to, label, icon: Icon, shortcut }) => (
            <CommandItem key={to} value={`page ${label}`} onSelect={() => go(() => navigate(to))}>
              <Icon />
              {label}
              {shortcut && <CommandShortcut>{shortcut}</CommandShortcut>}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  );
}
