import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { ChevronRight, Clock, FileUp, LayoutGrid, List, MoreHorizontal, Plus, Search, Timer } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ContextMenu, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { InstanceContextMenuContent, InstanceDropdownContent } from "@/components/instance-actions";
import { InstanceIcon } from "@/components/instance-icon";
import { useNewInstance } from "@/components/new-instance-dialog";
import { EmptyState, Page, PageHeader } from "@/components/page";
import { PlayButton } from "@/components/play-button";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage } from "@/lib/api";
import { groupInstances, loaderLabel, sortInstances } from "@/lib/instances";
import { cn, formatPlaytime, formatRelative } from "@/lib/utils";
import { usePrefs, type LibraryGroupBy, type LibrarySort } from "@/stores/prefs";

const GROUP_BY: { value: LibraryGroupBy; label: string }[] = [
  { value: "group", label: "Group" },
  { value: "loader", label: "Loader" },
  { value: "version", label: "Game version" },
  { value: "none", label: "Nothing" },
];
const SORTS: { value: LibrarySort; label: string }[] = [
  { value: "played", label: "Last played" },
  { value: "name", label: "Name" },
  { value: "version", label: "Game version" },
  { value: "playtime", label: "Playtime" },
];

/** "Fabric 1.21.1" in a muted line, with the loader tinted. */
function VersionLine({ instance, className }: { instance: InstanceSummary; className?: string }) {
  return (
    <span className={cn("truncate text-xs text-muted-foreground", className)}>
      <span className={instance.loader === "fabric" ? "text-foreground/80" : undefined}>{loaderLabel(instance)}</span>{" "}
      {instance.mc_version}
    </span>
  );
}

function InstanceCard({ instance }: { instance: InstanceSummary }) {
  const navigate = useNavigate();
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          role="link"
          tabIndex={0}
          className={cn(
            "group relative flex cursor-pointer items-center gap-3 rounded-lg border bg-card p-2.5 transition-colors outline-none",
            "hover:border-foreground/15 hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
            instance.running && "border-success/40 bg-success/[0.04]",
          )}
          onClick={() => navigate(`/instance/${instance.slug}`)}
          onKeyDown={(e) => e.key === "Enter" && navigate(`/instance/${instance.slug}`)}
        >
          <div className="relative">
            <InstanceIcon instance={instance} className="size-14" />
            <div
              className={cn(
                "absolute inset-0 flex items-center justify-center rounded-md bg-black/50 opacity-0 transition-opacity group-hover:opacity-100",
                instance.running && "opacity-100 bg-black/40",
              )}
            >
              <PlayButton instance={instance} iconOnly size="sm" className="shadow-md" />
            </div>
          </div>
          <div className="min-w-0 flex-1 space-y-0.5">
            <div className="truncate text-[13px] leading-tight font-semibold">{instance.name}</div>
            <VersionLine instance={instance} className="block" />
            <div className="flex items-center gap-2.5 pt-0.5 text-[11px] text-muted-foreground tabular-nums">
              {instance.running ? (
                <span className="flex items-center gap-1 font-medium text-success">
                  <span className="size-1.5 animate-pulse rounded-full bg-success" /> Running
                </span>
              ) : (
                <span className="flex items-center gap-1" title="Last played">
                  <Clock className="size-3" /> {formatRelative(instance.last_played_unix)}
                </span>
              )}
              {instance.playtime_secs > 0 && (
                <span className="flex items-center gap-1" title="Total playtime">
                  <Timer className="size-3" /> {formatPlaytime(instance.playtime_secs)}
                </span>
              )}
            </div>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon-xs"
                className="absolute top-1.5 right-1.5 opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100"
                onClick={(e) => e.stopPropagation()}
                aria-label="More actions"
              >
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <InstanceDropdownContent instance={instance} />
          </DropdownMenu>
        </div>
      </ContextMenuTrigger>
      <InstanceContextMenuContent instance={instance} />
    </ContextMenu>
  );
}

function InstanceGrid({ instances }: { instances: InstanceSummary[] }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-2">
      {instances.map((i) => (
        <InstanceCard key={i.slug} instance={i} />
      ))}
    </div>
  );
}

function InstanceTable({ instances }: { instances: InstanceSummary[] }) {
  const navigate = useNavigate();
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <Table className="text-[13px]">
        <TableHeader>
          <TableRow className="hover:bg-transparent [&>th]:h-8 [&>th]:text-[11px] [&>th]:font-medium [&>th]:tracking-wide [&>th]:text-muted-foreground [&>th]:uppercase">
            <TableHead className="pl-3">Name</TableHead>
            <TableHead className="w-36">Version</TableHead>
            <TableHead className="w-24 text-right">Mods</TableHead>
            <TableHead className="w-36">Last played</TableHead>
            <TableHead className="w-28 text-right">Playtime</TableHead>
            <TableHead className="w-24 pr-3" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {instances.map((i) => (
            <ContextMenu key={i.slug}>
              <ContextMenuTrigger asChild>
                <TableRow
                  className="group cursor-pointer data-[running=true]:bg-success/[0.04]"
                  data-running={i.running}
                  onClick={() => navigate(`/instance/${i.slug}`)}
                >
                  <TableCell className="py-1.5 pl-3">
                    <div className="flex items-center gap-2.5">
                      <InstanceIcon instance={i} className="size-7" />
                      <span className="truncate font-medium">{i.name}</span>
                      {i.group && (
                        <span className="truncate rounded border px-1.5 text-[10px] text-muted-foreground">{i.group}</span>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="py-1.5">
                    <VersionLine instance={i} />
                  </TableCell>
                  <TableCell className="py-1.5 text-right text-muted-foreground tabular-nums">
                    {i.loader === "fabric" ? i.mod_count : "—"}
                  </TableCell>
                  <TableCell className="py-1.5 text-muted-foreground">
                    {i.running ? <span className="font-medium text-success">Running</span> : formatRelative(i.last_played_unix)}
                  </TableCell>
                  <TableCell className="py-1.5 text-right text-muted-foreground tabular-nums">
                    {formatPlaytime(i.playtime_secs)}
                  </TableCell>
                  <TableCell className="py-1.5 pr-3">
                    <div className="flex items-center justify-end gap-1">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            className="opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100"
                            onClick={(e) => e.stopPropagation()}
                            aria-label="More actions"
                          >
                            <MoreHorizontal />
                          </Button>
                        </DropdownMenuTrigger>
                        <InstanceDropdownContent instance={i} />
                      </DropdownMenu>
                      <PlayButton instance={i} iconOnly size="sm" />
                    </div>
                  </TableCell>
                </TableRow>
              </ContextMenuTrigger>
              <InstanceContextMenuContent instance={i} />
            </ContextMenu>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

/** Every installed instance: search, group, sort, grid or list, right-click for everything else. */
export function LibraryPage() {
  const { data: instances, isLoading, error } = useInstances();
  const openNew = useNewInstance((s) => s.setOpen);
  const openModpack = useNewInstance((s) => s.openModpack);
  const { view, groupBy, sort, collapsed, set, toggleCollapsed } = usePrefs();
  const [query, setQuery] = useState("");

  async function importMrpack() {
    const path = await open({
      multiple: false,
      title: "Import a Modrinth modpack",
      filters: [{ name: "Modrinth modpack", extensions: ["mrpack"] }],
    });
    if (typeof path === "string") openModpack({ source: "file", path });
  }

  // Dropping a .mrpack onto the library starts creating an instance from it.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const pack = event.payload.paths.find((p) => p.toLowerCase().endsWith(".mrpack"));
      if (pack) openModpack({ source: "file", path: pack });
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [openModpack]);

  const sections = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = (instances ?? []).filter(
      (i) =>
        !q ||
        i.name.toLowerCase().includes(q) ||
        i.mc_version.includes(q) ||
        loaderLabel(i).toLowerCase().includes(q) ||
        (i.group ?? "").toLowerCase().includes(q),
    );
    return groupInstances(sortInstances(filtered, sort), groupBy);
  }, [instances, query, sort, groupBy]);

  const running = instances?.filter((i) => i.running).length ?? 0;
  const shown = sections.reduce((n, s) => n + s.instances.length, 0);
  const List_ = view === "grid" ? InstanceGrid : InstanceTable;

  return (
    <Page>
      <PageHeader
        title="Library"
        meta={
          instances && (
            <>
              {instances.length} instance{instances.length === 1 ? "" : "s"}
              {running > 0 && <span className="text-success"> · {running} running</span>}
            </>
          )
        }
      >
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="w-56 pl-8"
            placeholder="Filter instances"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Select value={groupBy} onValueChange={(v) => set({ groupBy: v as LibraryGroupBy })}>
          <SelectTrigger className="w-40">
            <span className="text-muted-foreground">Group:</span>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {GROUP_BY.map((g) => (
              <SelectItem key={g.value} value={g.value}>
                {g.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={sort} onValueChange={(v) => set({ sort: v as LibrarySort })}>
          <SelectTrigger className="w-40">
            <span className="text-muted-foreground">Sort:</span>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SORTS.map((s) => (
              <SelectItem key={s.value} value={s.value}>
                {s.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ToggleGroup
          type="single"
          variant="outline"
          value={view}
          onValueChange={(v) => v && set({ view: v as "grid" | "list" })}
        >
          <ToggleGroupItem value="grid" aria-label="Grid view" title="Grid view">
            <LayoutGrid />
          </ToggleGroupItem>
          <ToggleGroupItem value="list" aria-label="List view" title="List view">
            <List />
          </ToggleGroupItem>
        </ToggleGroup>
        <Button variant="outline" onClick={() => void importMrpack()} title="Create an instance from a .mrpack file">
          <FileUp /> Import .mrpack
        </Button>
        <Button onClick={() => openNew(true)}>
          <Plus /> Create
        </Button>
      </PageHeader>

      {error && <p className="mb-3 text-sm text-destructive">{errorMessage(error)}</p>}

      {isLoading && (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-2">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-[78px]" />
          ))}
        </div>
      )}

      {instances?.length === 0 && (
        <EmptyState>
          <p className="mb-3">No instances yet.</p>
          <Button onClick={() => openNew(true)}>
            <Plus /> Create your first instance
          </Button>
        </EmptyState>
      )}
      {instances && instances.length > 0 && shown === 0 && <EmptyState>Nothing matches "{query}".</EmptyState>}

      <div className="space-y-5">
        {sections.map((section) =>
          section.instances.length === 0 ? null : groupBy === "none" ? (
            <List_ key={section.key} instances={section.instances} />
          ) : (
            <Collapsible
              key={section.key}
              open={!collapsed.includes(section.key)}
              onOpenChange={() => toggleCollapsed(section.key)}
            >
              <CollapsibleTrigger className="group/section mb-2 flex w-full items-center gap-1.5 text-left text-xs font-semibold tracking-wide text-muted-foreground uppercase hover:text-foreground">
                <ChevronRight className="size-3.5 transition-transform group-data-[state=open]/section:rotate-90" />
                {section.label}
                <span className="font-normal tabular-nums">{section.instances.length}</span>
                <span className="ml-2 h-px flex-1 bg-border" />
              </CollapsibleTrigger>
              <CollapsibleContent>
                <List_ instances={section.instances} />
              </CollapsibleContent>
            </Collapsible>
          ),
        )}
      </div>
    </Page>
  );
}
