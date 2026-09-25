import { useEffect, useMemo, useState, type ComponentType } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronRight,
  ClipboardCopy,
  CornerLeftUp,
  ExternalLink,
  File,
  FileArchive,
  FileCode,
  FileImage,
  FilePlus,
  FileText,
  FileUp,
  Folder,
  FolderOpen,
  FolderPlus,
  HardDrive,
  Loader2,
  Pencil,
  RefreshCw,
  ScrollText,
  Search,
  SquarePen,
  Trash2,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { toast } from "sonner";

import type { FileEntry } from "@/bindings/FileEntry";
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
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { errorMessage, run } from "@/lib/api";
import { cn, formatBytes, formatRelative } from "@/lib/utils";

/** Extensions opened in the built-in editor rather than the system app. */
const TEXT_EXTENSIONS = new Set([
  "txt", "json", "json5", "toml", "properties", "cfg", "conf", "ini", "yml", "yaml", "log",
  "md", "mcmeta", "snbt", "js", "zs", "csv", "xml", "lang", "fsh", "vsh", "glsl",
]);

function extensionOf(name: string) {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function isText(e: FileEntry) {
  return !e.is_dir && (TEXT_EXTENSIONS.has(extensionOf(e.name)) || e.name === "options.txt");
}

function iconFor(e: FileEntry): { Icon: ComponentType<{ className?: string }>; tint: string } {
  if (e.is_dir) return { Icon: Folder, tint: "text-primary/85 fill-primary/15" };
  const ext = extensionOf(e.name);
  if (["png", "jpg", "jpeg", "gif", "webp"].includes(ext)) return { Icon: FileImage, tint: "text-sky-400" };
  if (["jar", "zip", "gz", "7z", "rar"].includes(ext)) return { Icon: FileArchive, tint: "text-amber-500" };
  if (["json", "json5", "toml", "yml", "yaml", "js", "zs", "snbt", "mcmeta", "xml"].includes(ext))
    return { Icon: FileCode, tint: "text-violet-400" };
  if (ext === "log") return { Icon: ScrollText, tint: "text-muted-foreground" };
  if (["dat", "mca", "nbt", "dat_old"].includes(ext)) return { Icon: HardDrive, tint: "text-emerald-500" };
  if (TEXT_EXTENSIONS.has(ext)) return { Icon: FileText, tint: "text-muted-foreground" };
  return { Icon: File, tint: "text-muted-foreground" };
}

const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const parentOf = (path: string) => path.split("/").slice(0, -1).join("/");

type Prompt =
  | { kind: "new-file" | "new-folder" }
  | { kind: "rename"; entry: FileEntry };

/** Ask for a single file/folder name (new file, new folder, rename). */
function NameDialog({
  prompt,
  existing,
  onSubmit,
  onClose,
  pending,
}: {
  prompt: Prompt;
  existing: Set<string>;
  onSubmit: (name: string) => void;
  onClose: () => void;
  pending: boolean;
}) {
  const initial = prompt.kind === "rename" ? prompt.entry.name : "";
  const [name, setName] = useState(initial);
  const trimmed = name.trim();
  const invalid = /[/\\]/.test(trimmed) || trimmed === "." || trimmed === "..";
  const taken = trimmed !== initial && existing.has(trimmed.toLowerCase());
  const title =
    prompt.kind === "rename" ? `Rename ${prompt.entry.name}` : prompt.kind === "new-file" ? "New file" : "New folder";

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <form
          className="space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (trimmed && !invalid && !taken && trimmed !== initial) onSubmit(trimmed);
          }}
        >
          <Input
            autoFocus
            placeholder={prompt.kind === "new-folder" ? "folder-name" : "file.txt"}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onFocus={(e) => {
              // Select the stem so typing replaces the name, not the extension.
              const dot = e.target.value.lastIndexOf(".");
              e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length);
            }}
            aria-invalid={invalid || taken}
            className="font-mono"
          />
          {invalid && <p className="text-xs text-destructive">Names can't contain slashes.</p>}
          {taken && <p className="text-xs text-destructive">Something with that name already exists here.</p>}
          <DialogFooter className="pt-2">
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!trimmed || invalid || taken || trimmed === initial || pending}>
              {pending && <Loader2 className="animate-spin" />}
              {prompt.kind === "rename" ? "Rename" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Full-size text editor for one file; Ctrl+S saves. */
function EditorDialog({ slug, path, onClose }: { slug: string; path: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const file = useQuery({
    queryKey: ["file", slug, path],
    queryFn: async () => (await run({ command: "file_read", instance: slug, path }, "file_contents")).text,
    staleTime: 0,
    gcTime: 0,
  });
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? file.data ?? "";
  const dirty = draft !== null && draft !== file.data;

  const save = useMutation({
    mutationFn: () => run({ command: "file_write", instance: slug, path, text, create_new: false }, "file_written"),
    onSuccess: () => {
      toast.success(`Saved ${path.split("/").pop()}`);
      queryClient.setQueryData(["file", slug, path], text);
      setDraft(null);
      void queryClient.invalidateQueries({ queryKey: ["files", slug] });
    },
    onError: (err) => toast.error("Couldn't save", { description: errorMessage(err) }),
  });

  const lines = text.split("\n").length;

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o && (!dirty || window.confirm("Discard unsaved changes?"))) onClose();
      }}
    >
      <DialogContent
        className="flex h-[85vh] flex-col gap-0 p-0 sm:max-w-5xl"
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
            e.preventDefault();
            if (dirty) save.mutate();
          }
        }}
      >
        <DialogHeader className="flex-row items-center gap-3 border-b px-4 py-2.5 pr-12">
          <SquarePen className="size-4 text-muted-foreground" />
          <div className="min-w-0 flex-1">
            <DialogTitle className="truncate font-mono text-[13px]">
              {path}
              {dirty && <span className="ml-1.5 text-primary">●</span>}
            </DialogTitle>
            <DialogDescription className="sr-only">Edit the file and save with Ctrl+S.</DialogDescription>
          </div>
          <span className="text-[11px] text-muted-foreground tabular-nums">{lines} lines</span>
          <Button size="sm" disabled={!dirty || save.isPending} onClick={() => save.mutate()}>
            {save.isPending && <Loader2 className="animate-spin" />}
            Save <span className="text-[10px] opacity-60">Ctrl+S</span>
          </Button>
        </DialogHeader>
        {file.isLoading ? (
          <Skeleton className="m-4 flex-1" />
        ) : file.error ? (
          <p className="p-4 text-sm text-destructive">{errorMessage(file.error)}</p>
        ) : (
          <Textarea
            autoFocus
            spellCheck={false}
            value={text}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              // Tab inserts a tab instead of leaving the editor.
              if (e.key === "Tab") {
                e.preventDefault();
                document.execCommand("insertText", false, "\t");
              }
            }}
            className="flex-1 resize-none rounded-none border-0 bg-console p-4 font-mono text-[12.5px] leading-5 text-console-foreground shadow-none focus-visible:ring-0 dark:bg-console"
            style={{ tabSize: 4 }}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

/**
 * Explorer for an instance's game directory: browse, open, edit text files,
 * create, rename, import (button or drag-and-drop) and delete, with a
 * context menu on every row and on the empty area.
 */
export function FileBrowser({ instance }: { instance: InstanceSummary }) {
  const slug = instance.slug;
  const queryClient = useQueryClient();
  const [dir, setDir] = useState("");
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [deleting, setDeleting] = useState<FileEntry | null>(null);
  const [editing, setEditing] = useState<string | null>(null);

  const listing = useQuery({
    queryKey: ["files", slug, dir],
    queryFn: async () => (await run({ command: "file_list", instance: slug, path: dir }, "file_listed")).entries,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["files", slug] });

  const q = filter.trim().toLowerCase();
  const entries = useMemo(
    () => (listing.data ?? []).filter((e) => !q || e.name.toLowerCase().includes(q)),
    [listing.data, q],
  );
  const existing = useMemo(() => new Set((listing.data ?? []).map((e) => e.name.toLowerCase())), [listing.data]);

  function go(path: string) {
    setDir(path);
    setFilter("");
    setSelected(null);
  }

  function openEntry(e: FileEntry) {
    if (e.is_dir) go(e.path);
    else if (isText(e)) setEditing(e.path);
    else void openPath(e.abs_path);
  }

  const onError = (title: string) => (err: unknown) => toast.error(title, { description: errorMessage(err) });

  const create = useMutation({
    mutationFn: async ({ kind, name }: { kind: "new-file" | "new-folder"; name: string }) => {
      const path = join(dir, name);
      if (kind === "new-folder") await run({ command: "file_create_dir", instance: slug, path }, "file_written");
      else await run({ command: "file_write", instance: slug, path, text: "", create_new: true }, "file_written");
      return { kind, path };
    },
    onSuccess: ({ kind, path }) => {
      setPrompt(null);
      void refresh();
      setSelected(path);
      if (kind === "new-file") setEditing(path);
    },
    onError: onError("Couldn't create it"),
  });

  const rename = useMutation({
    mutationFn: ({ entry, name }: { entry: FileEntry; name: string }) =>
      run({ command: "file_rename", instance: slug, from: entry.path, to: join(parentOf(entry.path), name) }, "file_written"),
    onSuccess: (out) => {
      setPrompt(null);
      setSelected(out.path);
      void refresh();
    },
    onError: onError("Couldn't rename it"),
  });

  const remove = useMutation({
    mutationFn: (entry: FileEntry) => run({ command: "file_delete", instance: slug, path: entry.path }, "file_deleted"),
    onSuccess: (out) => {
      toast.success(`Deleted ${out.path.split("/").pop()}`);
      setDeleting(null);
      setSelected(null);
      void refresh();
    },
    onError: onError("Couldn't delete it"),
  });

  const importFiles = useMutation({
    mutationFn: (sources: string[]) =>
      run({ command: "file_import", instance: slug, path: dir, sources }, "file_imported"),
    onSuccess: (out) => {
      toast.success(`Imported ${out.count} item${out.count === 1 ? "" : "s"}`);
      void refresh();
    },
    onError: onError("Import failed"),
  });

  async function pickImport() {
    const picked = await open({ multiple: true, title: "Import files into this folder" });
    if (picked && picked.length) importFiles.mutate(picked);
  }

  // Drop files from the OS onto the window to copy them into this folder.
  const { mutate: importMutate } = importFiles;
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths.length) importMutate(event.payload.paths);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [importMutate]);

  const selectedEntry = entries.find((e) => e.path === selected) ?? null;
  const crumbs = dir ? dir.split("/") : [];
  const dirAbs = dir ? `${instance.game_dir}/${dir}` : instance.game_dir;

  function onKeyDown(e: React.KeyboardEvent) {
    if (!selectedEntry || prompt || deleting || editing) return;
    if (e.key === "Enter") openEntry(selectedEntry);
    else if (e.key === "Delete") setDeleting(selectedEntry);
    else if (e.key === "F2") setPrompt({ kind: "rename", entry: selectedEntry });
  }

  const areaMenu = (
    <ContextMenuContent className="w-52">
      <ContextMenuItem onSelect={() => setPrompt({ kind: "new-file" })}>
        <FilePlus /> New file
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => setPrompt({ kind: "new-folder" })}>
        <FolderPlus /> New folder
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void pickImport()}>
        <FileUp /> Import files…
      </ContextMenuItem>
      <ContextMenuSeparator />
      {dir && (
        <ContextMenuItem onSelect={() => go(parentOf(dir))}>
          <CornerLeftUp /> Up one level
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={() => void refresh()}>
        <RefreshCw /> Refresh
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void openPath(dirAbs)}>
        <FolderOpen /> Open in file explorer
      </ContextMenuItem>
    </ContextMenuContent>
  );

  return (
    <div className="space-y-3" onKeyDown={onKeyDown}>
      <div className="flex flex-wrap items-center gap-2">
        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden rounded-md border bg-muted/30 px-1.5 py-1 font-mono text-xs">
          <button
            className={cn("rounded px-1.5 py-0.5 hover:bg-accent", !dir && "font-semibold text-foreground")}
            onClick={() => go("")}
          >
            .minecraft
          </button>
          {crumbs.map((c, i) => (
            <span key={i} className="flex min-w-0 items-center gap-0.5">
              <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
              <button
                className={cn(
                  "truncate rounded px-1.5 py-0.5 hover:bg-accent",
                  i === crumbs.length - 1 ? "font-semibold text-foreground" : "text-muted-foreground",
                )}
                onClick={() => go(crumbs.slice(0, i + 1).join("/"))}
              >
                {c}
              </button>
            </span>
          ))}
        </nav>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input className="w-44 pl-8" placeholder="Filter" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="outline" size="icon" aria-label="New file" onClick={() => setPrompt({ kind: "new-file" })}>
              <FilePlus />
            </Button>
          </TooltipTrigger>
          <TooltipContent>New file</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="outline" size="icon" aria-label="New folder" onClick={() => setPrompt({ kind: "new-folder" })}>
              <FolderPlus />
            </Button>
          </TooltipTrigger>
          <TooltipContent>New folder</TooltipContent>
        </Tooltip>
        <Button variant="outline" disabled={importFiles.isPending} onClick={() => void pickImport()}>
          {importFiles.isPending ? <Loader2 className="animate-spin" /> : <FileUp />}
          Import
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="outline" size="icon" aria-label="Open in file explorer" onClick={() => void openPath(dirAbs)}>
              <FolderOpen />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Open in file explorer</TooltipContent>
        </Tooltip>
      </div>

      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div className="min-h-72 overflow-hidden rounded-lg border bg-card" tabIndex={-1}>
            <div className="grid h-8 grid-cols-[1fr_100px_140px] items-center gap-3 border-b bg-muted/30 px-3 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
              <span>Name</span>
              <span className="text-right">Size</span>
              <span>Modified</span>
            </div>
            {dir && (
              <button
                className="grid w-full grid-cols-[1fr_100px_140px] items-center gap-3 border-b px-3 py-1.5 text-left text-[13px] text-muted-foreground hover:bg-accent/40"
                onDoubleClick={() => go(parentOf(dir))}
                onClick={() => go(parentOf(dir))}
              >
                <span className="flex items-center gap-2.5">
                  <CornerLeftUp className="size-4" /> ..
                </span>
              </button>
            )}
            {listing.isLoading && <Skeleton className="m-3 h-24" />}
            {listing.error && <p className="p-3 text-sm text-destructive">{errorMessage(listing.error)}</p>}
            {listing.data && entries.length === 0 && (
              <p className="px-3 py-10 text-center text-sm text-muted-foreground">
                {q ? `Nothing matches "${filter}".` : "This folder is empty. Right-click to create something, or drop files here."}
              </p>
            )}
            {entries.map((e) => {
              const { Icon, tint } = iconFor(e);
              return (
                <ContextMenu key={e.path}>
                  <ContextMenuTrigger asChild>
                    <div
                      role="row"
                      tabIndex={0}
                      className={cn(
                        "grid cursor-default grid-cols-[1fr_100px_140px] items-center gap-3 border-b px-3 py-1.5 text-[13px] outline-none last:border-b-0 hover:bg-accent/40",
                        selected === e.path && "bg-primary/10 hover:bg-primary/15",
                      )}
                      onClick={() => setSelected(e.path)}
                      onDoubleClick={() => openEntry(e)}
                      onContextMenu={() => setSelected(e.path)}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        <Icon className={cn("size-4 shrink-0", tint)} />
                        <span className={cn("truncate", e.is_dir && "font-medium")}>{e.name}</span>
                      </span>
                      <span className="text-right text-xs text-muted-foreground tabular-nums">
                        {e.is_dir ? "—" : formatBytes(e.size)}
                      </span>
                      <span className="text-xs text-muted-foreground" title={new Date(e.modified_unix * 1000).toLocaleString()}>
                        {formatRelative(e.modified_unix)}
                      </span>
                    </div>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-56">
                    <ContextMenuItem className="font-medium" onSelect={() => openEntry(e)}>
                      {e.is_dir ? <FolderOpen /> : isText(e) ? <SquarePen /> : <ExternalLink />}
                      {e.is_dir ? "Open" : isText(e) ? "Edit" : "Open"}
                      <ContextMenuShortcut>Enter</ContextMenuShortcut>
                    </ContextMenuItem>
                    {!e.is_dir && isText(e) && (
                      <ContextMenuItem onSelect={() => void openPath(e.abs_path)}>
                        <ExternalLink /> Open with system editor
                      </ContextMenuItem>
                    )}
                    <ContextMenuItem onSelect={() => void revealItemInDir(e.abs_path)}>
                      <FolderOpen /> Show in file explorer
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem onSelect={() => setPrompt({ kind: "rename", entry: e })}>
                      <Pencil /> Rename <ContextMenuShortcut>F2</ContextMenuShortcut>
                    </ContextMenuItem>
                    <ContextMenuItem
                      onSelect={() =>
                        void navigator.clipboard.writeText(e.abs_path).then(() => toast.success("Path copied"))
                      }
                    >
                      <ClipboardCopy /> Copy path
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem onSelect={() => setPrompt({ kind: "new-file" })}>
                      <FilePlus /> New file here
                    </ContextMenuItem>
                    <ContextMenuItem onSelect={() => setPrompt({ kind: "new-folder" })}>
                      <FolderPlus /> New folder here
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem variant="destructive" onSelect={() => setDeleting(e)}>
                      <Trash2 /> Delete <ContextMenuShortcut>Del</ContextMenuShortcut>
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              );
            })}
          </div>
        </ContextMenuTrigger>
        {areaMenu}
      </ContextMenu>

      <p className="text-[11px] text-muted-foreground">
        Double-click to open · Enter edit · F2 rename · Del delete · drop files onto the window to import them here
      </p>

      {prompt && (
        <NameDialog
          key={prompt.kind === "rename" ? prompt.entry.path : prompt.kind}
          prompt={prompt}
          existing={existing}
          pending={create.isPending || rename.isPending}
          onClose={() => setPrompt(null)}
          onSubmit={(name) =>
            prompt.kind === "rename" ? rename.mutate({ entry: prompt.entry, name }) : create.mutate({ kind: prompt.kind, name })
          }
        />
      )}

      <AlertDialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {deleting?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              {deleting?.is_dir
                ? "This folder and everything inside it will be permanently deleted."
                : "This file will be permanently deleted."}{" "}
              It doesn't go to the recycle bin.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={remove.isPending}
              onClick={(ev) => {
                ev.preventDefault();
                if (deleting) remove.mutate(deleting);
              }}
            >
              {remove.isPending && <Loader2 className="animate-spin" />}
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {editing && <EditorDialog key={editing} slug={slug} path={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}
