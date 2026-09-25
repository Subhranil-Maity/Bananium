import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useInfiniteQuery } from "@tanstack/react-query";
import { CalendarClock, Check, Download, Heart, Loader2, Search } from "lucide-react";

import type { ModrinthHit } from "@/bindings/ModrinthHit";
import type { SearchSort } from "@/bindings/SearchSort";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { InstanceIcon } from "@/components/instance-icon";
import { EmptyState, Page, PageHeader } from "@/components/page";
import { ProjectSheet } from "@/components/project-sheet";
import { useInstallContent } from "@/hooks/use-content";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { BROWSE_KINDS, formatCount, type BrowseKind } from "@/lib/content";
import { useNewInstance } from "@/components/new-instance-dialog";
import { loaderLabel } from "@/lib/instances";
import { cn, formatRelative } from "@/lib/utils";
import { usePresenceView } from "@/lib/presence";

const PAGE = 20;
const ANY = "__any__";
const SORTS: { value: SearchSort; label: string }[] = [
  { value: "relevance", label: "Relevance" },
  { value: "downloads", label: "Downloads" },
  { value: "follows", label: "Follows" },
  { value: "newest", label: "Newest" },
  { value: "updated", label: "Recently updated" },
];

/** Modrinth category slugs offered as filters, per content kind. */
const CATEGORIES: Record<BrowseKind, string[]> = {
  modpack: [
    "adventure",
    "challenging",
    "combat",
    "kitchen-sink",
    "lightweight",
    "magic",
    "multiplayer",
    "optimization",
    "quests",
    "technology",
  ],
  mod: [
    "adventure",
    "decoration",
    "equipment",
    "food",
    "game-mechanics",
    "library",
    "magic",
    "management",
    "mobs",
    "optimization",
    "social",
    "storage",
    "technology",
    "transportation",
    "utility",
    "worldgen",
  ],
  resource_pack: [
    "16x",
    "32x",
    "64x",
    "128x",
    "audio",
    "blocks",
    "combat",
    "decoration",
    "entities",
    "fonts",
    "gui",
    "items",
    "models",
    "realistic",
    "simplistic",
    "themed",
    "tweaks",
    "vanilla-like",
  ],
  shader: [
    "vanilla-like",
    "semi-realistic",
    "realistic",
    "fantasy",
    "cartoon",
    "atmosphere",
    "colored-lighting",
    "path-tracing",
    "pbr",
    "reflections",
    "shadows",
    "potato",
    "low",
    "medium",
    "high",
  ],
};

function prettyCategory(slug: string) {
  return slug.replace(/-/g, " ").replace(/^\w/, (c) => c.toUpperCase());
}

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

function HitCard({
  hit,
  kind,
  instance,
  onOpen,
}: {
  hit: ModrinthHit;
  kind: BrowseKind;
  instance: string | null;
  onOpen: () => void;
}) {
  const install = useInstallContent();
  const openModpack = useNewInstance((s) => s.openModpack);
  return (
    <div
      className="group flex cursor-pointer gap-3.5 rounded-lg border bg-card p-3 transition-colors hover:border-foreground/15 hover:bg-accent/40"
      onClick={onOpen}
    >
      {hit.icon_url ? (
        <img src={hit.icon_url} alt="" className="size-[72px] shrink-0 rounded-lg bg-muted" loading="lazy" />
      ) : (
        <div className="size-[72px] shrink-0 rounded-lg bg-muted" />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-[15px] font-semibold">{hit.title}</span>
          <span className="shrink-0 truncate text-xs text-muted-foreground">by {hit.author}</span>
        </div>
        <p className="mt-0.5 line-clamp-2 text-[13px] text-muted-foreground">{hit.description}</p>
        <div className="mt-2 flex flex-wrap gap-1">
          {hit.categories.slice(0, 5).map((c) => (
            <span key={c} className="rounded bg-muted px-1.5 py-px text-[11px] text-muted-foreground">
              {prettyCategory(c)}
            </span>
          ))}
        </div>
      </div>
      <div className="flex w-36 shrink-0 flex-col items-end justify-between gap-2 text-xs text-muted-foreground tabular-nums">
        <div className="space-y-0.5 text-right">
          <div className="flex items-center justify-end gap-1.5">
            <Download className="size-3.5" />
            <span className="font-semibold text-foreground">{formatCount(hit.downloads)}</span>
          </div>
          <div className="flex items-center justify-end gap-1.5">
            <Heart className="size-3.5" /> {formatCount(hit.follows)}
          </div>
          <div className="flex items-center justify-end gap-1.5" title={new Date(hit.date_modified).toLocaleString()}>
            <CalendarClock className="size-3.5" /> {formatRelative(Date.parse(hit.date_modified) / 1000)}
          </div>
        </div>
        {kind === "modpack" ? (
          <Button
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              openModpack({ source: "modrinth", projectId: hit.project_id, title: hit.title, iconUrl: hit.icon_url });
            }}
          >
            <Download /> Install
          </Button>
        ) : (
          instance &&
          (hit.installed ? (
            <span className="flex h-7 items-center gap-1 rounded-md bg-success/10 px-2.5 text-xs font-medium text-success">
              <Check className="size-3.5" /> Installed
            </span>
          ) : (
            <Button
              size="sm"
              disabled={install.isPending}
              onClick={(e) => {
                e.stopPropagation();
                install.mutate({ instance, kind, project: hit.project_id });
              }}
            >
              {install.isPending ? <Loader2 className="animate-spin" /> : <Download />}
              Install
            </Button>
          ))
        )}
      </div>
    </div>
  );
}

function FilterHeading({ children, action }: { children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className="mb-1.5 flex items-center justify-between text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
      {children}
      {action}
    </div>
  );
}

/**
 * Modrinth browser. `?kind=` and `?instance=` in the URL let an instance's
 * content tab deep-link here with the right target preselected.
 */
export function BrowsePage() {
  const [params, setParams] = useSearchParams();
  const kind = (params.get("kind") as BrowseKind | null) ?? "mod";
  const modpacks = kind === "modpack";
  // Modpacks become new instances, so there's no target instance for them.
  const instance = modpacks ? null : params.get("instance");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SearchSort>("relevance");
  const [categories, setCategories] = useState<string[]>([]);
  const [openProject, setOpenProject] = useState<string | null>(null);
  usePresenceView({ view: "browse", kind });
  const debouncedQuery = useDebounced(query, 300);
  const { data: instances } = useInstances();
  const target = instances?.find((i) => i.slug === instance);
  const vanillaTarget = target?.loader === "vanilla" && kind !== "resource_pack";

  function setParam(key: string, value: string | null) {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    setParams(next, { replace: true });
  }

  const results = useInfiniteQuery({
    queryKey: ["modrinth-search", kind, instance, debouncedQuery, sort, categories],
    queryFn: ({ pageParam }) =>
      run(
        kind === "modpack"
          ? { command: "modpack_search", query: debouncedQuery, categories, sort, offset: pageParam, limit: PAGE }
          : {
              command: "modrinth_search",
              query: debouncedQuery,
              kind,
              instance,
              categories,
              sort,
              offset: pageParam,
              limit: PAGE,
            },
        "modrinth_searched",
      ),
    initialPageParam: 0,
    getNextPageParam: (last) =>
      last.offset + last.hits.length < last.total_hits ? last.offset + last.hits.length : undefined,
    enabled: !vanillaTarget,
    staleTime: 60_000,
  });

  // Infinite scroll: fetch the next page when the sentinel scrolls into view.
  const sentinel = useRef<HTMLDivElement>(null);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = results;
  useEffect(() => {
    const el = sentinel.current;
    if (!el) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries[0]?.isIntersecting && hasNextPage && !isFetchingNextPage) void fetchNextPage();
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const hits = results.data?.pages.flatMap((p) => p.hits) ?? [];
  const total = results.data?.pages[0]?.total_hits;

  return (
    <Page>
      <PageHeader title="Browse Modrinth" meta={total !== undefined && `${formatCount(total)} results`}>
        <div className="flex rounded-md border bg-muted/40 p-0.5">
          {BROWSE_KINDS.map((k) => (
            <button
              key={k.kind}
              onClick={() => {
                setCategories([]);
                setParam("kind", k.kind);
              }}
              className={cn(
                "h-7 rounded px-3 text-[13px] font-medium text-muted-foreground transition-colors hover:text-foreground",
                kind === k.kind && "bg-background text-foreground shadow-sm dark:bg-accent",
              )}
            >
              {k.label}
            </button>
          ))}
        </div>
      </PageHeader>

      <div className="grid grid-cols-[220px_1fr] items-start gap-5">
        <aside className="sticky top-4 space-y-5">
          {modpacks ? (
            <p className="rounded-md border bg-muted/30 px-2.5 py-2 text-[11px] text-muted-foreground">
              Installing a modpack creates a new instance with the pack's Minecraft version, Fabric loader and mods.
            </p>
          ) : (
          <div>
            <FilterHeading>Install to</FilterHeading>
            <Select value={instance ?? ANY} onValueChange={(v) => setParam("instance", v === ANY ? null : v)}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>No instance</SelectItem>
                {instances?.map((i) => (
                  <SelectItem key={i.slug} value={i.slug}>
                    <InstanceIcon instance={i} className="size-4" showRunning={false} />
                    {i.name}
                    <span className="text-muted-foreground">
                      {loaderLabel(i)} {i.mc_version}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="mt-1.5 text-[11px] text-muted-foreground">
              {target
                ? `Showing only what runs on ${loaderLabel(target)} ${target.mc_version}.`
                : "Pick an instance to filter by compatibility and install in one click."}
            </p>
          </div>
          )}

          <div>
            <FilterHeading
              action={
                categories.length > 0 && (
                  <button className="text-[11px] font-medium tracking-normal normal-case hover:text-foreground" onClick={() => setCategories([])}>
                    Clear
                  </button>
                )
              }
            >
              Categories
            </FilterHeading>
            <div className="space-y-px">
              {CATEGORIES[kind].map((c) => {
                const on = categories.includes(c);
                return (
                  <label
                    key={c}
                    className={cn(
                      "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-[13px] text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                      on && "text-foreground",
                    )}
                  >
                    <Checkbox
                      checked={on}
                      onCheckedChange={(v) =>
                        setCategories((cs) => (v === true ? [...cs, c] : cs.filter((x) => x !== c)))
                      }
                    />
                    {prettyCategory(c)}
                  </label>
                );
              })}
            </div>
          </div>
        </aside>

        <div className="min-w-0 space-y-3">
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                className="h-9 pl-8"
                placeholder={`Search ${BROWSE_KINDS.find((k) => k.kind === kind)!.label.toLowerCase()}…`}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <Select value={sort} onValueChange={(v) => setSort(v as SearchSort)}>
              <SelectTrigger className="h-9! w-48">
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
          </div>

          {vanillaTarget && (
            <EmptyState>
              {target?.name} is a vanilla instance. Mods and shaders need Fabric — create a Fabric instance to use
              them. Resource packs work anywhere.
            </EmptyState>
          )}
          {results.error && <p className="text-sm text-destructive">{errorMessage(results.error)}</p>}
          {!vanillaTarget && total === 0 && <EmptyState>No results. Try fewer filters or a different search.</EmptyState>}

          <div className="space-y-2">
            {results.isLoading && [0, 1, 2, 3, 4].map((i) => <Skeleton key={i} className="h-[98px]" />)}
            {hits.map((h) => (
              <HitCard key={h.project_id} hit={h} kind={kind} instance={instance} onOpen={() => setOpenProject(h.project_id)} />
            ))}
            <div ref={sentinel} className="h-8">
              {isFetchingNextPage && <Loader2 className="mx-auto animate-spin text-muted-foreground" />}
            </div>
          </div>
        </div>
      </div>

      <ProjectSheet
        projectId={openProject}
        author={hits.find((h) => h.project_id === openProject)?.author ?? null}
        kind={kind}
        instance={instance}
        onOpenChange={(o) => !o && setOpenProject(null)}
      />
    </Page>
  );
}
