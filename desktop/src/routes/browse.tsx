import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useInfiniteQuery } from "@tanstack/react-query";
import { Check, Download, Heart, Loader2, Search } from "lucide-react";

import type { ContentKind } from "@/bindings/ContentKind";
import type { ModrinthHit } from "@/bindings/ModrinthHit";
import type { SearchSort } from "@/bindings/SearchSort";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ProjectSheet } from "@/components/project-sheet";
import { useInstallContent } from "@/hooks/use-content";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { KINDS, formatCount } from "@/lib/content";

const PAGE = 20;
const ANY = "__any__";
const SORTS: { value: SearchSort; label: string }[] = [
  { value: "relevance", label: "Relevance" },
  { value: "downloads", label: "Downloads" },
  { value: "follows", label: "Follows" },
  { value: "newest", label: "Newest" },
  { value: "updated", label: "Recently updated" },
];

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

function HitRow({
  hit,
  kind,
  instance,
  onOpen,
}: {
  hit: ModrinthHit;
  kind: ContentKind;
  instance: string | null;
  onOpen: () => void;
}) {
  const install = useInstallContent();
  return (
    <div
      className="flex cursor-pointer items-center gap-4 rounded-lg border bg-card p-3 transition-colors hover:border-primary"
      onClick={onOpen}
    >
      {hit.icon_url ? (
        <img src={hit.icon_url} alt="" className="size-14 shrink-0 rounded-lg" loading="lazy" />
      ) : (
        <div className="size-14 shrink-0 rounded-lg bg-muted" />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate font-semibold">{hit.title}</span>
          <span className="truncate text-xs text-muted-foreground">by {hit.author}</span>
        </div>
        <p className="line-clamp-2 text-sm text-muted-foreground">{hit.description}</p>
        <div className="mt-1 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
          <span className="flex items-center gap-1">
            <Download className="size-3" /> {formatCount(hit.downloads)}
          </span>
          <span className="flex items-center gap-1">
            <Heart className="size-3" /> {formatCount(hit.follows)}
          </span>
          {hit.categories.slice(0, 4).map((c) => (
            <Badge key={c} variant="secondary" className="px-1.5 py-0 text-[10px]">
              {c}
            </Badge>
          ))}
        </div>
      </div>
      {instance &&
        (hit.installed ? (
          <Badge variant="outline" className="gap-1">
            <Check className="size-3" /> Installed
          </Badge>
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
        ))}
    </div>
  );
}

/**
 * Modrinth browser. `?kind=` and `?instance=` in the URL let an instance's
 * content tab deep-link here with the right target preselected.
 */
export function BrowsePage() {
  const [params, setParams] = useSearchParams();
  const kind = (params.get("kind") as ContentKind | null) ?? "mod";
  const instance = params.get("instance");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SearchSort>("relevance");
  const [openProject, setOpenProject] = useState<string | null>(null);
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
    queryKey: ["modrinth-search", kind, instance, debouncedQuery, sort],
    queryFn: ({ pageParam }) =>
      run(
        {
          command: "modrinth_search",
          query: debouncedQuery,
          kind,
          instance,
          categories: [],
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
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">Browse Modrinth</h1>
        <Tabs value={kind} onValueChange={(v) => setParam("kind", v)}>
          <TabsList>
            {KINDS.map((k) => (
              <TabsTrigger key={k.kind} value={k.kind}>
                {k.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>

      <div className="flex flex-wrap gap-2">
        <div className="relative min-w-60 flex-1">
          <Search className="absolute top-2.5 left-2.5 size-4 text-muted-foreground" />
          <Input
            className="pl-8"
            placeholder={`Search ${KINDS.find((k) => k.kind === kind)!.label.toLowerCase()}…`}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Select value={sort} onValueChange={(v) => setSort(v as SearchSort)}>
          <SelectTrigger className="w-44">
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
        <Select value={instance ?? ANY} onValueChange={(v) => setParam("instance", v === ANY ? null : v)}>
          <SelectTrigger className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>Any instance</SelectItem>
            {instances?.map((i) => (
              <SelectItem key={i.slug} value={i.slug}>
                {i.name} · {i.mc_version}
                {i.loader === "fabric" ? " · Fabric" : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {!instance && (
        <p className="text-sm text-muted-foreground">
          Pick a target instance to see only compatible results and install with one click.
        </p>
      )}
      {vanillaTarget && (
        <div className="rounded-lg border border-dashed p-8 text-center text-muted-foreground">
          {target?.name} is a vanilla instance. Mods and shaders need Fabric — create a Fabric instance
          to use them. Resource packs work anywhere.
        </div>
      )}
      {results.error && <p className="text-sm text-destructive">{errorMessage(results.error)}</p>}
      {total !== undefined && <p className="text-xs text-muted-foreground">{formatCount(total)} results</p>}

      <div className="space-y-2">
        {results.isLoading && [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-20" />)}
        {hits.map((h) => (
          <HitRow
            key={h.project_id}
            hit={h}
            kind={kind}
            instance={instance}
            onOpen={() => setOpenProject(h.project_id)}
          />
        ))}
        <div ref={sentinel} className="h-8">
          {isFetchingNextPage && <Loader2 className="mx-auto animate-spin text-muted-foreground" />}
        </div>
      </div>

      <ProjectSheet
        projectId={openProject}
        kind={kind}
        instance={instance}
        onOpenChange={(o) => !o && setOpenProject(null)}
      />
    </div>
  );
}
