import { useQuery } from "@tanstack/react-query";
import { Download, ExternalLink, Heart, Loader2 } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { ContentKind } from "@/bindings/ContentKind";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useInstallContent } from "@/hooks/use-content";
import { errorMessage, run } from "@/lib/api";
import { formatCount } from "@/lib/content";

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
        ["Source", p.source_url],
        ["Issues", p.issues_url],
        ["Wiki", p.wiki_url],
        ["Discord", p.discord_url],
        ["Modrinth", `https://modrinth.com/project/${p.slug ?? p.id}`],
      ].filter(([, url]) => url) as [string, string][])
    : [];

  return (
    <Sheet open={projectId !== null} onOpenChange={onOpenChange}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-2xl">
        <SheetHeader>
          {p ? (
            <div className="flex items-start gap-3">
              {p.icon_url && <img src={p.icon_url} alt="" className="size-14 rounded-lg" />}
              <div className="min-w-0">
                <SheetTitle className="text-xl">{p.title}</SheetTitle>
                <SheetDescription>{p.description}</SheetDescription>
                <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                  <span className="flex items-center gap-1">
                    <Download className="size-3" /> {formatCount(p.downloads)}
                  </span>
                  <span className="flex items-center gap-1">
                    <Heart className="size-3" /> {formatCount(p.followers)}
                  </span>
                  {p.license && <span>{p.license}</span>}
                </div>
              </div>
            </div>
          ) : (
            <>
              <SheetTitle>Loading…</SheetTitle>
              <Skeleton className="h-14" />
            </>
          )}
          {project.error && <p className="text-sm text-destructive">{errorMessage(project.error)}</p>}
          {p && (
            <div className="flex flex-wrap gap-2 pt-2">
              {links.map(([label, url]) => (
                <Button key={label} variant="outline" size="sm" onClick={() => void openUrl(url)}>
                  <ExternalLink /> {label}
                </Button>
              ))}
            </div>
          )}
        </SheetHeader>

        {p && (
          <Tabs defaultValue="versions" className="px-4 pb-6">
            <TabsList>
              <TabsTrigger value="versions">Versions</TabsTrigger>
              <TabsTrigger value="description">Description</TabsTrigger>
              {p.gallery.length > 0 && <TabsTrigger value="gallery">Gallery</TabsTrigger>}
            </TabsList>

            <TabsContent value="versions" className="space-y-1">
              {!instance && (
                <p className="py-2 text-sm text-muted-foreground">
                  Choose a target instance on the Browse page to install.
                </p>
              )}
              {versions.isLoading && <Skeleton className="h-40" />}
              {versions.data?.map((v) => (
                <div
                  key={v.id}
                  className="flex items-center gap-3 rounded-md border px-3 py-2 text-sm data-[incompatible=true]:opacity-50"
                  data-incompatible={!v.compatible}
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{v.version_number}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {v.loaders.join(", ")} · {v.game_versions.slice(-3).join(", ")}
                      {v.game_versions.length > 3 ? "…" : ""}
                    </div>
                  </div>
                  {v.version_type !== "release" && <Badge variant="outline">{v.version_type}</Badge>}
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!instance || !v.compatible || install.isPending}
                    onClick={() => install.mutate({ instance: instance!, kind, project: p.id, version: v.id })}
                  >
                    {install.isPending ? <Loader2 className="animate-spin" /> : <Download />}
                  </Button>
                </div>
              ))}
            </TabsContent>

            <TabsContent value="description">
              {/* Modrinth bodies are markdown with some inline HTML; the HTML
                  is intentionally not rendered (react-markdown skips it). */}
              <article className="prose prose-sm max-w-none select-text dark:prose-invert prose-img:inline-block prose-img:my-1">
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
