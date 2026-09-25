import { useQuery } from "@tanstack/react-query";
import { Download, ExternalLink, Heart, Loader2 } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { ContentKind } from "@/bindings/ContentKind";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useInstallContent } from "@/hooks/use-content";
import { errorMessage, run } from "@/lib/api";
import { formatCount } from "@/lib/content";
import { cn } from "@/lib/utils";

const VERSION_TYPE: Record<string, string> = {
  release: "bg-success/15 text-success",
  beta: "bg-warning/15 text-warning",
  alpha: "bg-destructive/15 text-destructive",
};

/** Full Modrinth project view: description, gallery, versions with install buttons. */
export function ProjectSheet({
  projectId,
  kind,
  instance,
  onOpenChange,
}: {
  projectId: string | null;
  kind: ContentKind;
  instance: string | null;
  onOpenChange: (open: boolean) => void;
}) {
  const install = useInstallContent();
  const project = useQuery({
    queryKey: ["modrinth-project", projectId],
    queryFn: async () =>
      (await run({ command: "modrinth_project", project: projectId! }, "modrinth_project_shown")).project,
    enabled: projectId !== null,
    staleTime: 5 * 60_000,
  });
  const versions = useQuery({
    queryKey: ["modrinth-versions", projectId, instance],
    queryFn: async () =>
      (
        await run(
          { command: "modrinth_versions", project: projectId!, kind, instance },
          "modrinth_versions_listed",
        )
      ).versions,
    enabled: projectId !== null,
    staleTime: 5 * 60_000,
  });

  const p = project.data;
  const links = p
    ? ([
        ["Modrinth", `https://modrinth.com/project/${p.slug ?? p.id}`],
        ["Source", p.source_url],
        ["Issues", p.issues_url],
        ["Wiki", p.wiki_url],
        ["Discord", p.discord_url],
      ].filter(([, url]) => url) as [string, string][])
    : [];

  return (
    <Sheet open={projectId !== null} onOpenChange={onOpenChange}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-2xl">
        <SheetHeader className="border-b p-5">
          {p ? (
            <div className="flex items-start gap-4">
              {p.icon_url ? (
                <img src={p.icon_url} alt="" className="size-20 shrink-0 rounded-xl bg-muted" />
              ) : (
                <div className="size-20 shrink-0 rounded-xl bg-muted" />
              )}
              <div className="min-w-0 space-y-1">
                <SheetTitle className="text-lg leading-tight">{p.title}</SheetTitle>
                <SheetDescription className="text-[13px]">{p.description}</SheetDescription>
                <div className="flex flex-wrap items-center gap-3 pt-1 text-xs text-muted-foreground tabular-nums">
                  <span className="flex items-center gap-1">
                    <Download className="size-3.5" />
                    <span className="font-semibold text-foreground">{formatCount(p.downloads)}</span> downloads
                  </span>
                  <span className="flex items-center gap-1">
                    <Heart className="size-3.5" /> {formatCount(p.followers)}
                  </span>
                  {p.license && <span className="rounded border px-1.5 py-px text-[11px]">{p.license}</span>}
                </div>
              </div>
            </div>
          ) : (
            <>
              <SheetTitle className="sr-only">Loading…</SheetTitle>
              <Skeleton className="h-20" />
            </>
          )}
          {project.error && <p className="text-sm text-destructive">{errorMessage(project.error)}</p>}
          {p && (
            <div className="flex flex-wrap gap-1.5 pt-3">
              {links.map(([label, url]) => (
                <Button key={label} variant="outline" size="sm" onClick={() => void openUrl(url)}>
                  <ExternalLink /> {label}
                </Button>
              ))}
            </div>
          )}
        </SheetHeader>

        {p && (
          <Tabs defaultValue="versions" className="gap-3 px-5 py-4">
            <TabsList variant="line" className="h-8 gap-4 p-0">
              <TabsTrigger value="versions" className="flex-none px-0.5 data-[state=active]:after:bg-primary">
                Versions {versions.data && <span className="text-xs text-muted-foreground">{versions.data.length}</span>}
              </TabsTrigger>
              <TabsTrigger value="description" className="flex-none px-0.5 data-[state=active]:after:bg-primary">
                Description
              </TabsTrigger>
              {p.gallery.length > 0 && (
                <TabsTrigger value="gallery" className="flex-none px-0.5 data-[state=active]:after:bg-primary">
                  Gallery
                </TabsTrigger>
              )}
            </TabsList>

            <TabsContent value="versions">
              {!instance && (
                <p className="mb-2 rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
                  Pick an instance under "Install to" on the Browse page to install a specific version.
                </p>
              )}
              {versions.isLoading && <Skeleton className="h-40" />}
              <div className="overflow-hidden rounded-lg border">
                {versions.data?.map((v) => (
                  <div
                    key={v.id}
                    className={cn(
                      "flex items-center gap-3 border-b px-3 py-2 last:border-b-0 hover:bg-accent/40",
                      !v.compatible && "opacity-45",
                    )}
                  >
                    <span
                      className={cn(
                        "w-14 shrink-0 rounded px-1.5 py-px text-center text-[10px] font-semibold tracking-wide uppercase",
                        VERSION_TYPE[v.version_type] ?? "bg-muted text-muted-foreground",
                      )}
                    >
                      {v.version_type}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-mono text-xs font-medium">{v.version_number}</div>
                      <div className="truncate text-[11px] text-muted-foreground">
                        {v.loaders.join(", ")} · {v.game_versions.slice(-4).join(", ")}
                        {v.game_versions.length > 4 ? "…" : ""}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={!instance || !v.compatible || install.isPending}
                      title={v.compatible ? "Install this version" : "Not compatible with the target instance"}
                      onClick={() => install.mutate({ instance: instance!, kind, project: p.id, version: v.id })}
                    >
                      {install.isPending ? <Loader2 className="animate-spin" /> : <Download />}
                      Install
                    </Button>
                  </div>
                ))}
              </div>
            </TabsContent>

            <TabsContent value="description">
              {/* Modrinth bodies are markdown with some inline HTML; the HTML
                  is intentionally not rendered (react-markdown skips it). */}
              <article className="prose prose-sm max-w-none select-text dark:prose-invert prose-img:my-1 prose-img:inline-block">
                <Markdown remarkPlugins={[remarkGfm]}>{p.body}</Markdown>
              </article>
            </TabsContent>

            <TabsContent value="gallery" className="grid grid-cols-2 gap-3">
              {p.gallery.map((g) => (
                <figure key={g.url} className="space-y-1">
                  <img src={g.url} alt={g.title ?? ""} className="rounded-md border" loading="lazy" />
                  {g.title && <figcaption className="text-xs text-muted-foreground">{g.title}</figcaption>}
                </figure>
              ))}
            </TabsContent>
          </Tabs>
        )}
      </SheetContent>
    </Sheet>
  );
}
