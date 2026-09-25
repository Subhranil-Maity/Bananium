import { useState, type ComponentType } from "react";
import { useLocation, useNavigate } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Copy,
  FolderInput,
  FolderOpen,
  ImageMinus,
  ImagePlus,
  Loader2,
  Pencil,
  Play,
  Square,
  SquareArrowOutUpRight,
  Terminal,
  Trash2,
} from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { ContextMenuContent, ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { DryRunDialog } from "@/components/dry-run-dialog";
import {
  INSTANCES_KEY,
  pickIconFile,
  useInstances,
  useKill,
  useLaunch,
  useSetGroup,
  useSetIcon,
} from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { allGroups } from "@/lib/instances";
import { cn } from "@/lib/utils";
import { useInstanceDialogs } from "@/stores/instance-dialogs";

/** Mirrors `bananium_instance::is_valid_name`: letters, digits, '-', '_'. */
export const VALID_INSTANCE_NAME = /^[A-Za-z0-9_-]+$/;

interface Action {
  id: string;
  label: string;
  icon: ComponentType<{ className?: string }>;
  onSelect: () => void;
  disabled?: boolean;
  destructive?: boolean;
  primary?: boolean;
}

/**
 * Everything you can do to an instance, in menu order, split into groups
 * that render with separators between them.
 */
export function useInstanceActions(instance: InstanceSummary, { includeOpen = true } = {}): Action[][] {
  const navigate = useNavigate();
  const launch = useLaunch();
  const kill = useKill();
  const setIcon = useSetIcon();
  const show = useInstanceDialogs((s) => s.show);
  const slug = instance.slug;

  const play: Action = instance.running
    ? { id: "stop", label: "Stop", icon: Square, onSelect: () => kill.mutate(slug), destructive: true }
    : { id: "play", label: "Play", icon: Play, onSelect: () => launch.mutate(slug), primary: true };

  return [
    [
      play,
      ...(includeOpen
        ? [{ id: "open", label: "Open", icon: SquareArrowOutUpRight, onSelect: () => navigate(`/instance/${slug}`) }]
        : []),
      { id: "folder", label: "Open folder", icon: FolderOpen, onSelect: () => void openPath(instance.game_dir) },
    ],
    [
      {
        id: "icon",
        label: "Change icon…",
        icon: ImagePlus,
        onSelect: () =>
          void pickIconFile().then((path) => path && setIcon.mutate({ slug, path })),
      },
      ...(instance.icon_path
        ? [{ id: "icon-clear", label: "Remove icon", icon: ImageMinus, onSelect: () => setIcon.mutate({ slug, path: null }) }]
        : []),
      { id: "group", label: "Move to group…", icon: FolderInput, onSelect: () => show("group", slug) },
      { id: "rename", label: "Rename…", icon: Pencil, onSelect: () => show("rename", slug), disabled: instance.running },
      { id: "clone", label: "Duplicate…", icon: Copy, onSelect: () => show("clone", slug) },
      { id: "dry-run", label: "Show launch command", icon: Terminal, onSelect: () => show("dry-run", slug) },
    ],
    [
      {
        id: "delete",
        label: "Delete…",
        icon: Trash2,
        onSelect: () => show("delete", slug),
        disabled: instance.running,
        destructive: true,
      },
    ],
  ];
}

function itemClass(a: Action) {
  return cn(a.primary && "font-medium");
}

/** Right-click menu body for an instance. */
export function InstanceContextMenuContent({ instance }: { instance: InstanceSummary }) {
  const groups = useInstanceActions(instance);
  return (
    <ContextMenuContent className="w-52">
      {groups.map((group, gi) => [
        gi > 0 && <ContextMenuSeparator key={`sep-${gi}`} />,
        ...group.map((a) => (
          <ContextMenuItem
            key={a.id}
            disabled={a.disabled}
            variant={a.destructive ? "destructive" : "default"}
            className={itemClass(a)}
            onSelect={a.onSelect}
          >
            <a.icon />
            {a.label}
          </ContextMenuItem>
        )),
      ])}
    </ContextMenuContent>
  );
}

/** "…" dropdown body for an instance (same actions as the context menu). */
export function InstanceDropdownContent({
  instance,
  includeOpen = true,
  includePlay = true,
}: {
  instance: InstanceSummary;
  includeOpen?: boolean;
  includePlay?: boolean;
}) {
  const groups = useInstanceActions(instance, { includeOpen })
    .map((g) => g.filter((a) => includePlay || (a.id !== "play" && a.id !== "stop")))
    .filter((g) => g.length > 0);
  return (
    // Portaled content still bubbles React events to its trigger's
    // ancestors, which are often clickable cards/rows.
    <DropdownMenuContent align="end" className="w-52" onClick={(e) => e.stopPropagation()}>
      {groups.map((group, gi) => [
        gi > 0 && <DropdownMenuSeparator key={`sep-${gi}`} />,
        ...group.map((a) => (
          <DropdownMenuItem
            key={a.id}
            disabled={a.disabled}
            variant={a.destructive ? "destructive" : "default"}
            className={itemClass(a)}
            onSelect={a.onSelect}
          >
            <a.icon />
            {a.label}
          </DropdownMenuItem>
        )),
      ])}
    </DropdownMenuContent>
  );
}

/** Rename or clone: both just ask for a new valid instance name. */
function NameDialog({ instance, mode }: { instance: InstanceSummary; mode: "rename" | "clone" }) {
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const close = useInstanceDialogs((s) => s.close);
  const [name, setName] = useState(mode === "clone" ? `${instance.name}-copy` : instance.name);
  const invalid = name !== "" && !VALID_INSTANCE_NAME.test(name);

  const submit = useMutation({
    mutationFn: async () =>
      mode === "rename"
        ? (await run({ command: "instance_rename", instance: instance.slug, new_name: name }, "instance_renamed"))
            .instance
        : (await run({ command: "instance_clone", instance: instance.slug, new_name: name }, "instance_cloned"))
            .instance,
    onSuccess: async (slug) => {
      await queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      toast.success(mode === "rename" ? `Renamed to ${name}` : `Duplicated as ${name}`);
      close();
      const onPage = location.pathname === `/instance/${instance.slug}`;
      if (mode === "clone" || onPage) navigate(`/instance/${slug}`, { replace: mode === "rename" });
    },
    onError: (err) => toast.error(mode === "rename" ? "Rename failed" : "Duplicate failed", { description: errorMessage(err) }),
  });

  return (
    <Dialog open onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{mode === "rename" ? `Rename ${instance.name}` : `Duplicate ${instance.name}`}</DialogTitle>
          <DialogDescription>
            {mode === "rename"
              ? "The instance folder is renamed too."
              : "Copies worlds, mods, resource packs and settings. Launch logs and playtime start fresh."}
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name && !invalid) submit.mutate();
          }}
        >
          <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} aria-invalid={invalid} />
          {invalid && <p className="text-xs text-destructive">Only letters, digits, '-' and '_' are allowed.</p>}
          <DialogFooter className="pt-2">
            <Button type="button" variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name || invalid || submit.isPending}>
              {submit.isPending && <Loader2 className="animate-spin" />}
              {mode === "rename" ? "Rename" : "Duplicate"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function GroupDialog({ instance, groups }: { instance: InstanceSummary; groups: string[] }) {
  const close = useInstanceDialogs((s) => s.close);
  const setGroup = useSetGroup();
  const [value, setValue] = useState(instance.group ?? "");
  const apply = (group: string) => setGroup.mutate({ slug: instance.slug, group }, { onSuccess: close });

  return (
    <Dialog open onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Move {instance.name} to a group</DialogTitle>
          <DialogDescription>Groups organise the library. Type a new name to create one.</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            apply(value.trim());
          }}
        >
          <Input autoFocus placeholder="Group name" value={value} onChange={(e) => setValue(e.target.value)} />
          {groups.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {groups.map((g) => (
                <button
                  key={g}
                  type="button"
                  className={cn(
                    "rounded-md border px-2 py-1 text-xs transition-colors hover:bg-accent",
                    g === value && "border-primary/60 bg-primary/10 text-foreground",
                  )}
                  onClick={() => setValue(g)}
                >
                  {g}
                </button>
              ))}
            </div>
          )}
          <DialogFooter className="pt-1">
            {instance.group && (
              <Button type="button" variant="ghost" className="mr-auto" onClick={() => apply("")}>
                Remove from group
              </Button>
            )}
            <Button type="submit" disabled={setGroup.isPending || value.trim() === (instance.group ?? "")}>
              {setGroup.isPending && <Loader2 className="animate-spin" />}
              Move
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function DeleteDialog({ instance }: { instance: InstanceSummary }) {
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const close = useInstanceDialogs((s) => s.close);
  const remove = useMutation({
    mutationFn: () => run({ command: "instance_remove", instance: instance.slug }, "instance_removed"),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      toast.success(`Deleted ${instance.name}`);
      close();
      if (location.pathname.startsWith(`/instance/${instance.slug}`)) navigate("/");
    },
    onError: (err) => toast.error("Delete failed", { description: errorMessage(err) }),
  });

  return (
    <AlertDialog open onOpenChange={(o) => !o && close()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete {instance.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            This permanently deletes the instance folder, including its worlds, mods, screenshots and logs. It
            can't be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            className="bg-destructive text-white hover:bg-destructive/90"
            disabled={remove.isPending}
            onClick={(e) => {
              e.preventDefault();
              remove.mutate();
            }}
          >
            {remove.isPending && <Loader2 className="animate-spin" />}
            Delete instance
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Renders whichever instance dialog is open; mounted once in the app shell. */
export function InstanceDialogsHost() {
  const open = useInstanceDialogs((s) => s.open);
  const close = useInstanceDialogs((s) => s.close);
  const { data: instances } = useInstances();
  const instance = open ? instances?.find((i) => i.slug === open.slug) : undefined;
  if (!open || !instance) return null;

  // Keyed by slug + kind so each opening starts from fresh form state.
  const key = `${open.kind}:${instance.slug}`;
  switch (open.kind) {
    case "rename":
    case "clone":
      return <NameDialog key={key} instance={instance} mode={open.kind} />;
    case "group":
      return <GroupDialog key={key} instance={instance} groups={allGroups(instances)} />;
    case "delete":
      return <DeleteDialog key={key} instance={instance} />;
    case "dry-run":
      return <DryRunDialog slug={instance.slug} open onOpenChange={(o) => !o && close()} />;
  }
}
