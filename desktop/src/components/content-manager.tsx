import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowUpCircle, FileUp, FolderOpen, Loader2, Plus, RefreshCw, ScanSearch, Trash2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { toast } from "sonner";

import type { ContentEntry } from "@/bindings/ContentEntry";
import type { ContentKind } from "@/bindings/ContentKind";
import type { ContentUpdateInfo } from "@/bindings/ContentUpdateInfo";
import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useContent } from "@/hooks/use-content";
import { errorMessage, run } from "@/lib/api";
import { KINDS, contentKey, kindExtension } from "@/lib/content";

function EntryRow({
  entry,
  slug,
  update,
}: {
  entry: ContentEntry;
  slug: string;
  update: ContentUpdateInfo | undefined;
}) {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries({ queryKey: contentKey(slug) });

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
      void refresh();
    },
    onError: (err) => toast.error("Couldn't remove it", { description: errorMessage(err) }),
  });

  return (
    <div className="flex items-center gap-3 rounded-lg border bg-card p-2.5 data-[off=true]:opacity-60" data-off={!entry.enabled}>
      {entry.icon_url ? (
        <img src={entry.icon_url} alt="" className="size-9 shrink-0 rounded-md" loading="lazy" />
      ) : (
        <div className="size-9 shrink-0 rounded-md bg-muted" />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">{entry.title}</span>
          {entry.dependency && <Badge variant="outline">dependency</Badge>}
          {!entry.project_id && <Badge variant="secondary">local</Badge>}
          {update && (
            <Badge className="gap-1 bg-blue-600 text-white" title={`Update to ${update.new_version_number}`}>
              <ArrowUpCircle className="size-3" /> {update.new_version_number}
            </Badge>
          )}
        </div>
        <div className="truncate text-xs text-muted-foreground">
          {entry.version_number ?? entry.filename}
        </div>
      </div>
      <Switch
        checked={entry.enabled}
        disabled={toggle.isPending}
        onCheckedChange={(on) => toggle.mutate(on)}
        title={entry.enabled ? "Disable" : "Enable"}
      />
      <Button variant="ghost" size="icon" title="Remove" disabled={remove.isPending} onClick={() => remove.mutate()}>
        <Trash2 />
      </Button>
    </div>
  );
}

/**
 * Mods / resource packs / shader packs of one instance: toggle, remove,
 * import local files (button or drag-and-drop), identify, and update.
 */
export function ContentManager({ instance }: { instance: InstanceSummary }) {
  const slug = instance.slug;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const vanilla = instance.loader === "vanilla";
  const [kind, setKind] = useState<ContentKind>(vanilla ? "resource_pack" : "mod");
  const [updates, setUpdates] = useState<ContentUpdateInfo[] | null>(null);
  const { data: entries, isLoading, error } = useContent(slug);
  const refresh = () => queryClient.invalidateQueries({ queryKey: contentKey(slug) });

  const shown = entries?.filter((e) => e.kind === kind) ?? [];
  const blocked = vanilla && kind !== "resource_pack";
  const updateFor = (e: ContentEntry) => updates?.find((u) => u.project_id === e.project_id);

  const importFiles = useMutation({
    mutationFn: async (paths: string[]) => {
      for (const path of paths) {
        await run({ command: "content_import", instance: slug, kind, path }, "content_imported");
      }
      return paths.length;
    },
    onSuccess: (n) => {
      toast.success(`Imported ${n} file${n === 1 ? "" : "s"}`);
      void refresh();
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
    mutationFn: (projects: string[]) =>
      run({ command: "content_update", instance: slug, projects }, "content_installed"),
    onSuccess: (out) => {
      toast.success(`Updated ${out.installed.length} item${out.installed.length === 1 ? "" : "s"}`);
      setUpdates([]);
      void refresh();
    },
    onError: (err) => toast.error("Update failed", { description: errorMessage(err) }),
  });

  const identify = useMutation({
    mutationFn: async () =>
      (await run({ command: "content_identify", instance: slug }, "content_identified")).identified,
    onSuccess: (n) => {
      toast.info(n ? `Identified ${n} file${n === 1 ? "" : "s"} on Modrinth` : "No new matches on Modrinth");
      void refresh();
    },
    onError: (err) => toast.error("Couldn't identify files", { description: errorMessage(err) }),
  });

  async function pickFiles() {
    const ext = kindExtension(kind);
    const picked = await open({
      multiple: true,
      filters: [{ name: KINDS.find((k) => k.kind === kind)!.label, extensions: [ext] }],
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

  const pendingUpdates = updates?.filter((u) => u.kind === kind) ?? [];

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Tabs value={kind} onValueChange={(v) => setKind(v as ContentKind)}>
          <TabsList>
            {KINDS.map((k) => (
              <TabsTrigger key={k.kind} value={k.kind}>
                {k.label}
                <span className="ml-1 text-xs text-muted-foreground">
                  {entries?.filter((e) => e.kind === k.kind).length ?? ""}
                </span>
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        <div className="flex-1" />
        <Button
          variant="outline"
          size="sm"
          title="Open folder"
          onClick={() => void openPath(`${instance.game_dir}/${kind === "mod" ? "mods" : kind === "shader" ? "shaderpacks" : "resourcepacks"}`)}
        >
          <FolderOpen />
        </Button>
        <Button variant="outline" size="sm" title="Identify local files on Modrinth" disabled={identify.isPending} onClick={() => identify.mutate()}>
          {identify.isPending ? <Loader2 className="animate-spin" /> : <ScanSearch />}
        </Button>
        <Button variant="outline" size="sm" disabled={checkUpdates.isPending} onClick={() => checkUpdates.mutate()}>
          {checkUpdates.isPending ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          Check updates
        </Button>
        {pendingUpdates.length > 0 && (
          <Button size="sm" disabled={applyUpdates.isPending} onClick={() => applyUpdates.mutate(pendingUpdates.map((u) => u.project_id))}>
            {applyUpdates.isPending ? <Loader2 className="animate-spin" /> : <ArrowUpCircle />}
            Update {pendingUpdates.length}
          </Button>
        )}
        <Button variant="outline" size="sm" disabled={blocked || importFiles.isPending} onClick={() => void pickFiles()}>
          {importFiles.isPending ? <Loader2 className="animate-spin" /> : <FileUp />}
          Import
        </Button>
        <Button size="sm" disabled={blocked} onClick={() => navigate(`/browse?kind=${kind}&instance=${slug}`)}>
          <Plus /> Add from Modrinth
        </Button>
      </div>

      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {blocked ? (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          This is a vanilla instance. {kind === "mod" ? "Mods" : "Shaders"} need Fabric — create a Fabric instance to
          use them.
        </div>
      ) : isLoading ? (
        <Skeleton className="h-32" />
      ) : shown.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          Nothing here yet. Add from Modrinth, or drop .{kindExtension(kind)} files onto the window.
        </div>
      ) : (
        <div className="space-y-1.5">
          {shown.map((e) => (
            <EntryRow key={`${e.kind}:${e.filename}`} entry={e} slug={slug} update={updateFor(e)} />
          ))}
        </div>
      )}
    </div>
  );
}
