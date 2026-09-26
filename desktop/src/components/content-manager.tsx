import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowUpCircle,
  ExternalLink,
  FileUp,
  FolderOpen,
  Loader2,
  MoreHorizontal,
  Plus,
  Power,
  PowerOff,
  RefreshCw,
  ScanSearch,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { toast } from "sonner";

import type { ContentEntry } from "@/bindings/ContentEntry";
import type { ContentKind } from "@/bindings/ContentKind";
import type { ContentUpdateInfo } from "@/bindings/ContentUpdateInfo";
import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { EmptyState } from "@/components/page";
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { useContent } from "@/hooks/use-content";
import { errorMessage, isAlreadyQueued, logAction, run } from "@/lib/api";
import { KINDS, contentKey, kindExtension } from "@/lib/content";
import { cn } from "@/lib/utils";
import { retryKey, useActiveTask, useTasks } from "@/stores/tasks";

function folderOf(kind: ContentKind) {
  return kind === "mod" ? "mods" : kind === "shader" ? "shaderpacks" : "resourcepacks";
}

const entryKey = (e: ContentEntry) => `${e.kind}:${e.filename}`;

function Tag({ children, className, title }: { children: React.ReactNode; className?: string; title?: string }) {
  return (
    <span
      title={title}
      className={cn("shrink-0 rounded px-1.5 py-px text-[10px] font-semibold tracking-wide uppercase", className)}
    >
      {children}
    </span>
  );
}

function EntryRow({
  entry,
  instance,
  update,
  selected,
  onSelect,
}: {
  entry: ContentEntry;
  instance: InstanceSummary;
  update: ContentUpdateInfo | undefined;
  selected: boolean;
  onSelect: (on: boolean) => void;
}) {
  const slug = instance.slug;
  const queryClient = useQueryClient();
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: contentKey(slug) });
    void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
  };
  const filePath = `${instance.game_dir}/${folderOf(entry.kind)}/${entry.filename}${entry.enabled ? "" : ".disabled"}`;

  const toggle = useMutation({
    mutationFn: (enabled: boolean) =>
      run(
        { command: "content_toggle", instance: slug, kind: entry.kind, filename: entry.filename, enabled },
        "content_toggled",
      ),
    onSuccess: refresh,
    onError: (err) => toast.error("Couldn't change it", { description: errorMessage(err) }),
  });
  const remove = useMutation({
    mutationFn: () =>
      run({ command: "content_remove", instance: slug, kind: entry.kind, filename: entry.filename }, "content_removed"),
    onSuccess: () => {
      toast.success(`Removed ${entry.title}`);
      refresh();
    },
    onError: (err) => toast.error("Couldn't remove it", { description: errorMessage(err) }),
  });

  return (
    <div
      className={cn(
        "group grid grid-cols-[28px_1fr_minmax(0,220px)_44px_32px] items-center gap-3 border-b px-3 py-1.5 last:border-b-0 hover:bg-accent/40",
        selected && "bg-primary/[0.06] hover:bg-primary/10",
      )}
    >
      <Checkbox checked={selected} onCheckedChange={(v) => onSelect(v === true)} aria-label={`Select ${entry.title}`} />
      <div className={cn("flex min-w-0 items-center gap-2.5", !entry.enabled && "opacity-50")}>
        {entry.icon_url ? (
          <img src={entry.icon_url} alt="" className="size-8 shrink-0 rounded-md bg-muted" loading="lazy" />
        ) : (
          <div className="size-8 shrink-0 rounded-md bg-muted ring-1 ring-border ring-inset" />
        )}
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[13px] font-medium">{entry.title}</span>
            {entry.dependency && (
              <Tag className="bg-muted text-muted-foreground" title="Installed as another project's dependency">
                dep
              </Tag>
            )}
            {entry.modrinth === "not_found" && (
              <Tag className="bg-muted text-muted-foreground" title="Checked: Modrinth doesn't know this file">
                not on Modrinth
              </Tag>
            )}
            {entry.modrinth === "unchecked" && (
              <Tag className="bg-warning/15 text-warning" title="Not identified yet; use Identify to look it up on Modrinth">
                unidentified
              </Tag>
            )}
            {!entry.enabled && <Tag className="bg-muted text-muted-foreground">disabled</Tag>}
          </div>
          <div className="truncate font-mono text-[11px] text-muted-foreground">{entry.filename}</div>
        </div>
      </div>
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate font-mono text-[11px] text-muted-foreground" title={entry.version_number ?? undefined}>
          {entry.version_number ?? "—"}
        </span>
        {update && (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="flex shrink-0 items-center gap-1 rounded bg-sky-500/15 px-1.5 py-px text-[10px] font-semibold text-sky-400">
                <ArrowUpCircle className="size-3" /> Update
              </span>
            </TooltipTrigger>
            <TooltipContent>{update.new_version_number}</TooltipContent>
          </Tooltip>
        )}
      </div>
      <Switch
        checked={entry.enabled}
        disabled={toggle.isPending}
        onCheckedChange={(on) => toggle.mutate(on)}
        aria-label={entry.enabled ? "Disable" : "Enable"}
      />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="More">
            <MoreHorizontal />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-48">
          {entry.project_id && (
            <DropdownMenuItem onSelect={() => void openUrl(`https://modrinth.com/project/${entry.project_id}`)}>
              <ExternalLink /> View on Modrinth
            </DropdownMenuItem>
          )}
          <DropdownMenuItem onSelect={() => void revealItemInDir(filePath)}>
            <FolderOpen /> Show file
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" disabled={remove.isPending} onSelect={() => remove.mutate()}>
            <Trash2 /> Remove
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

/**
 * Mods / resource packs / shader packs of one instance: a filterable table
 * with bulk enable/disable/update/remove, import (button or drag-and-drop),
 * Modrinth identification and update checks.
 */
export function ContentManager({ instance }: { instance: InstanceSummary }) {
  const slug = instance.slug;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const vanilla = instance.loader === "vanilla";
  const [kind, setKind] = useState<ContentKind>(vanilla ? "resource_pack" : "mod");
  const [updates, setUpdates] = useState<ContentUpdateInfo[] | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const { data: entries, isLoading, error } = useContent(slug);
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: contentKey(slug) });
    void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
  };

  const blocked = vanilla && kind !== "resource_pack";
  const q = query.trim().toLowerCase();
  const shown = useMemo(
    () =>
      (entries ?? [])
        .filter((e) => e.kind === kind)
        .filter((e) => !q || e.title.toLowerCase().includes(q) || e.filename.toLowerCase().includes(q))
        .sort((a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" })),
    [entries, kind, q],
  );
  const updateFor = (e: ContentEntry) => updates?.find((u) => u.project_id === e.project_id);
  const pendingUpdates = updates?.filter((u) => u.kind === kind) ?? [];
  const selectedEntries = shown.filter((e) => selected.has(entryKey(e)));
  const allSelected = shown.length > 0 && selectedEntries.length === shown.length;

  const importFiles = useMutation({
    mutationFn: async (paths: string[]) => {
      for (const path of paths) {
        await run({ command: "content_import", instance: slug, kind, path }, "content_imported");
      }
      return paths.length;
    },
    onSuccess: (n) => {
      toast.success(`Imported ${n} file${n === 1 ? "" : "s"}`);
      refresh();
    },
    onError: (err) => toast.error("Import failed", { description: errorMessage(err) }),
  });

  const checkUpdates = useMutation({
    mutationFn: async () =>
      (await run({ command: "content_check_updates", instance: slug }, "content_updates_found")).updates,
    onSuccess: (found) => {
      setUpdates(found);
      toast.info(found.length ? `${found.length} update${found.length === 1 ? "" : "s"} available` : "Everything is up to date");
    },
    onError: (err) => toast.error("Couldn't check for updates", { description: errorMessage(err) }),
  });

  const applyUpdates = useMutation({
    mutationFn: (projects: string[]) => run({ command: "content_update", instance: slug, projects }, "content_installed"),
    onSuccess: (out, projects) => {
      toast.success(`Updated ${out.installed.length} item${out.installed.length === 1 ? "" : "s"}`);
      setUpdates((u) => u?.filter((x) => !projects.includes(x.project_id)) ?? null);
      refresh();
    },
    onError: (err) => toast.error("Update failed", { description: errorMessage(err) }),
  });

  const registerRetry = useTasks((s) => s.registerRetry);
  const identifyTask = useActiveTask("content_identify", slug);
  const identify = useMutation({
    mutationKey: ["content-identify", slug],
    mutationFn: async () => await run({ command: "content_identify", instance: slug }, "content_identified"),
    onMutate: () => registerRetry(retryKey("content_identify", slug, null), () => identify.mutate()),
    onSuccess: ({ identified, not_found }) => {
      const found = identified
        ? `Identified ${identified} file${identified === 1 ? "" : "s"}`
        : "No new matches on Modrinth";
      toast.info(found, {
        description: not_found
          ? `${not_found} file${not_found === 1 ? " isn't" : "s aren't"} on Modrinth; the console lists which.`
          : undefined,
      });
      refresh();
    },
    onError: (err) => {
      if (isAlreadyQueued(err)) {
        toast.info("Already identifying", { description: "It's in the task tray." });
        return;
      }
      toast.error("Couldn't identify files", {
        description: errorMessage(err),
        closeButton: true,
        duration: 15_000,
        action: { label: "Retry", onClick: () => identify.mutate() },
      });
    },
  });
  const identifying = identify.isPending || !!identifyTask;
  const unidentified = (entries ?? []).filter((e) => e.modrinth === "unchecked").length;
  function startIdentify(from: string) {
    if (identifying) {
      logAction("identify_ignored", { instance: slug, reason: "already in progress" });
      return;
    }
    logAction("identify_clicked", { instance: slug, unidentified, from });
    identify.mutate();
  }

  /** Bulk actions run the per-item commands one after another. */
  const bulk = useMutation({
    mutationFn: async (action: "enable" | "disable" | "remove") => {
      for (const e of selectedEntries) {
        if (action === "remove") {
          await run({ command: "content_remove", instance: slug, kind: e.kind, filename: e.filename }, "content_removed");
        } else if (e.enabled !== (action === "enable")) {
          await run(
            { command: "content_toggle", instance: slug, kind: e.kind, filename: e.filename, enabled: action === "enable" },
            "content_toggled",
          );
        }
      }
      return { action, n: selectedEntries.length };
    },
    onSuccess: ({ action, n }) => {
      toast.success(`${action === "remove" ? "Removed" : action === "enable" ? "Enabled" : "Disabled"} ${n} item${n === 1 ? "" : "s"}`);
      setSelected(new Set());
    },
    onError: (err) => toast.error("Couldn't finish", { description: errorMessage(err) }),
    onSettled: refresh,
  });

  async function pickFiles() {
    const picked = await open({
      multiple: true,
      filters: [{ name: KINDS.find((k) => k.kind === kind)!.label, extensions: [kindExtension(kind)] }],
    });
    if (picked && picked.length) importFiles.mutate(picked);
  }

  // Drag files from the OS onto the window to import them into the current tab.
  const { mutate: importMutate } = importFiles;
  useEffect(() => {
    if (blocked) return;
    const ext = `.${kindExtension(kind)}`;
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const paths = event.payload.paths.filter((p) => p.toLowerCase().endsWith(ext));
      if (paths.length) importMutate(paths);
      else toast.error(`Only ${ext} files can be dropped here`);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [kind, blocked, importMutate]);

  const selectedUpdates = pendingUpdates.filter((u) => selectedEntries.some((e) => e.project_id === u.project_id));

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex rounded-md border bg-muted/40 p-0.5">
          {KINDS.map((k) => {
            const count = entries?.filter((e) => e.kind === k.kind).length ?? 0;
            return (
              <button
                key={k.kind}
                onClick={() => {
                  // Selection is per tab; switching kinds starts over.
                  setKind(k.kind);
                  setSelected(new Set());
                }}
                className={cn(
                  "flex h-7 items-center gap-1.5 rounded px-3 text-[13px] font-medium text-muted-foreground transition-colors hover:text-foreground",
                  kind === k.kind && "bg-background text-foreground shadow-sm dark:bg-accent",
                )}
              >
                {k.label}
                <span className="text-[11px] tabular-nums opacity-70">{count}</span>
              </button>
            );
          })}
        </div>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input className="w-52 pl-8" placeholder="Filter" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <div className="flex-1" />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon"
              aria-label="Open folder"
              onClick={() => void openPath(`${instance.game_dir}/${folderOf(kind)}`)}
            >
              <FolderOpen />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Open {folderOf(kind)} folder</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon"
              aria-label="Identify mods on Modrinth"
              disabled={identifying}
              onClick={() => startIdentify("toolbar")}
              className={cn(
                "relative",
                unidentified > 0 &&
                  !identifying &&
                  "border-primary text-primary shadow-[0_0_12px_-2px] shadow-primary/70 ring-2 ring-primary/40 hover:text-primary",
              )}
            >
              {identifying ? <Loader2 className="animate-spin" /> : <ScanSearch />}
              {unidentified > 0 && !identifying && (
                <span className="absolute -top-1.5 -right-1.5 min-w-4 rounded-full bg-primary px-1 text-[10px] leading-4 font-semibold text-primary-foreground tabular-nums">
                  {unidentified}
                </span>
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {identifying
              ? identifyTask?.status === "queued"
                ? "Identify is queued"
                : "Identifying…"
              : unidentified > 0
                ? `Identify ${unidentified} unidentified file${unidentified === 1 ? "" : "s"} on Modrinth`
                : "Everything is identified"}
          </TooltipContent>
        </Tooltip>
        <Button variant="outline" disabled={blocked || importFiles.isPending} onClick={() => void pickFiles()}>
          {importFiles.isPending ? <Loader2 className="animate-spin" /> : <FileUp />}
          Import
        </Button>
        {pendingUpdates.length > 0 ? (
          <Button
            variant="outline"
            className="border-sky-500/40 text-sky-400 hover:text-sky-300"
            disabled={applyUpdates.isPending}
            onClick={() => applyUpdates.mutate(pendingUpdates.map((u) => u.project_id))}
          >
            {applyUpdates.isPending ? <Loader2 className="animate-spin" /> : <ArrowUpCircle />}
            Update all ({pendingUpdates.length})
          </Button>
        ) : (
          <Button variant="outline" disabled={checkUpdates.isPending} onClick={() => checkUpdates.mutate()}>
            {checkUpdates.isPending ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            Check updates
          </Button>
        )}
        <Button disabled={blocked} onClick={() => navigate(`/browse?kind=${kind}&instance=${slug}`)}>
          <Plus /> Add content
        </Button>
      </div>

      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {unidentified > 0 && (
        <div className="flex items-center gap-2.5 rounded-md border border-primary/40 bg-primary/10 px-3 py-2 text-[13px]">
          <AlertTriangle className="size-4 shrink-0 text-primary" />
          <span className="flex-1">
            <span className="font-medium">
              {unidentified} file{unidentified === 1 ? " is" : "s are"} unidentified.
            </span>{" "}
            <span className="text-muted-foreground">Identify them to get icons, names and update checks.</span>
          </span>
          <Button size="sm" disabled={identifying} onClick={() => startIdentify("banner")}>
            {identifying ? <Loader2 className="animate-spin" /> : <ScanSearch />}
            {identifying ? (identifyTask?.status === "queued" ? "Queued" : "Identifying…") : "Identify"}
          </Button>
        </div>
      )}
      {blocked ? (
        <EmptyState>
          {instance.name} is a vanilla instance. {kind === "mod" ? "Mods" : "Shaders"} need Fabric — create a Fabric
          instance to use them. Resource packs work anywhere.
        </EmptyState>
      ) : isLoading ? (
        <Skeleton className="h-40" />
      ) : (entries?.filter((e) => e.kind === kind).length ?? 0) === 0 ? (
        <EmptyState>
          Nothing installed yet. Use <span className="font-medium text-foreground">Add content</span>, or drop .
          {kindExtension(kind)} files onto the window.
        </EmptyState>
      ) : (
        <div className="overflow-hidden rounded-lg border bg-card">
          <div className="grid h-9 grid-cols-[28px_1fr_minmax(0,220px)_44px_32px] items-center gap-3 border-b bg-muted/30 px-3 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
            <Checkbox
              checked={allSelected ? true : selectedEntries.length ? "indeterminate" : false}
              onCheckedChange={(v) => setSelected(v === true ? new Set(shown.map(entryKey)) : new Set())}
              aria-label="Select all"
            />
            {selectedEntries.length > 0 ? (
              <div className="col-span-4 flex items-center gap-1 normal-case">
                <span className="mr-2 text-xs font-semibold tracking-normal text-foreground">
                  {selectedEntries.length} selected
                </span>
                <Button variant="ghost" size="sm" disabled={bulk.isPending} onClick={() => bulk.mutate("enable")}>
                  <Power /> Enable
                </Button>
                <Button variant="ghost" size="sm" disabled={bulk.isPending} onClick={() => bulk.mutate("disable")}>
                  <PowerOff /> Disable
                </Button>
                {selectedUpdates.length > 0 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-sky-400"
                    disabled={applyUpdates.isPending}
                    onClick={() => applyUpdates.mutate(selectedUpdates.map((u) => u.project_id))}
                  >
                    <ArrowUpCircle /> Update {selectedUpdates.length}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  disabled={bulk.isPending}
                  onClick={() => bulk.mutate("remove")}
                >
                  {bulk.isPending ? <Loader2 className="animate-spin" /> : <Trash2 />} Remove
                </Button>
                <Button variant="ghost" size="icon-sm" className="ml-auto" onClick={() => setSelected(new Set())} aria-label="Clear selection">
                  <X />
                </Button>
              </div>
            ) : (
              <>
                <span>Name</span>
                <span>Version</span>
                <span>On</span>
                <span />
              </>
            )}
          </div>
          {shown.length === 0 ? (
            <p className="px-3 py-6 text-center text-sm text-muted-foreground">Nothing matches "{query}".</p>
          ) : (
            shown.map((e) => (
              <EntryRow
                key={entryKey(e)}
                entry={e}
                instance={instance}
                update={updateFor(e)}
                selected={selected.has(entryKey(e))}
                onSelect={(on) =>
                  setSelected((s) => {
                    const next = new Set(s);
                    if (on) next.add(entryKey(e));
                    else next.delete(entryKey(e));
                    return next;
                  })
                }
              />
            ))
          )}
        </div>
      )}
    </div>
  );
}
