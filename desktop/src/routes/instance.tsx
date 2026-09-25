import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { ArrowLeft, ChevronRight, Clock, FolderOpen, ImagePlus, MoreHorizontal, Package, Timer } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ContentManager } from "@/components/content-manager";
import { FileBrowser } from "@/components/file-browser";
import { InstanceDropdownContent } from "@/components/instance-actions";
import { InstanceIcon } from "@/components/instance-icon";
import { InstanceSettings } from "@/components/instance-settings";
import { LogViewer } from "@/components/log-viewer";
import { EmptyState, Page } from "@/components/page";
import { PlayButton } from "@/components/play-button";
import { pickIconFile, useInstance, useSetIcon } from "@/hooks/use-instances";
import { loaderLabel } from "@/lib/instances";
import { formatPlaytime, formatRelative } from "@/lib/utils";
import { ScreenshotsPage } from "@/routes/screenshots";

type Tab = "content" | "files" | "logs" | "screenshots" | "settings";

function Meta({ icon, children, title }: { icon?: ReactNode; children: ReactNode; title?: string }) {
  return (
    <span className="flex items-center gap-1.5 [&_svg]:size-3.5 [&_svg]:text-muted-foreground/70" title={title}>
      {icon}
      {children}
    </span>
  );
}

function Header({ instance }: { instance: InstanceSummary }) {
  const setIcon = useSetIcon();
  return (
    <div className="flex items-center gap-4">
      <button
        className="group relative rounded-lg focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        title="Change icon"
        onClick={() => void pickIconFile().then((path) => path && setIcon.mutate({ slug: instance.slug, path }))}
      >
        <InstanceIcon instance={instance} className="size-16" showRunning={false} />
        <span className="absolute inset-0 flex items-center justify-center rounded-md bg-black/55 text-white opacity-0 transition-opacity group-hover:opacity-100">
          <ImagePlus className="size-5" />
        </span>
      </button>

      <div className="min-w-0 flex-1 space-y-1.5">
        <div className="flex items-center gap-2.5">
          <h1 className="truncate text-xl leading-tight font-semibold tracking-tight">{instance.name}</h1>
          {instance.running && (
            <span className="flex items-center gap-1.5 rounded-full bg-success/15 px-2 py-0.5 text-[11px] font-semibold text-success">
              <span className="size-1.5 animate-pulse rounded-full bg-success" /> Running
            </span>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground tabular-nums">
          <span className="font-medium text-foreground/85">
            {loaderLabel(instance, true)} <span className="text-muted-foreground">·</span> Minecraft {instance.mc_version}
          </span>
          {instance.group && (
            <span className="rounded border px-1.5 py-px text-[11px]" title="Group">
              {instance.group}
            </span>
          )}
          <Meta icon={<Clock />} title="Last played">
            {instance.last_played_unix ? `Played ${formatRelative(instance.last_played_unix)}` : "Never played"}
          </Meta>
          <Meta icon={<Timer />} title="Total playtime">
            {formatPlaytime(instance.playtime_secs)} played
          </Meta>
          {instance.loader === "fabric" && (
            <Meta icon={<Package />}>
              {instance.mod_count} mod{instance.mod_count === 1 ? "" : "s"}
            </Meta>
          )}
        </div>
      </div>

      <div className="flex items-center gap-1.5">
        <Button variant="outline" size="icon" title="Open game folder" onClick={() => void openPath(instance.game_dir)}>
          <FolderOpen />
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="icon" aria-label="More actions">
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <InstanceDropdownContent instance={instance} includeOpen={false} includePlay={false} />
        </DropdownMenu>
        <PlayButton instance={instance} size="lg" className="ml-1 w-32" />
      </div>
    </div>
  );
}

/** One instance: header with play/stop and stats, then content, logs, screenshots and settings. */
export function InstancePage() {
  const { slug } = useParams();
  const navigate = useNavigate();
  const { data: instance, isLoading } = useInstance(slug);
  const [tab, setTab] = useState<Tab>(() => (instance?.running ? "logs" : "content"));

  // Starting the game from here jumps to its live log.
  const wasRunning = useRef(instance?.running ?? false);
  useEffect(() => {
    if (!instance) return;
    if (instance.running && !wasRunning.current) setTab("logs");
    wasRunning.current = instance.running;
  }, [instance]);

  if (isLoading) return null;
  if (!instance) {
    return (
      <Page>
        <EmptyState>
          <p className="mb-3">That instance doesn't exist any more.</p>
          <Button variant="outline" onClick={() => navigate("/")}>
            <ArrowLeft /> Back to library
          </Button>
        </EmptyState>
      </Page>
    );
  }

  return (
    <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)} className="flex h-full min-h-0 flex-col gap-0">
      <div className="border-b px-5 pt-3">
        <nav className="mb-3 flex items-center gap-1 text-xs text-muted-foreground">
          <Link to="/" className="hover:text-foreground">
            Library
          </Link>
          <ChevronRight className="size-3" />
          {instance.group && (
            <>
              <span>{instance.group}</span>
              <ChevronRight className="size-3" />
            </>
          )}
          <span className="text-foreground/80">{instance.name}</span>
        </nav>
        <Header instance={instance} />
        <TabsList variant="line" className="mt-3 h-9 gap-4 p-0">
          {(
            [
              ["content", "Content"],
              ["logs", "Logs"],
              ["files", "Files"],
              ["screenshots", "Screenshots"],
              ["settings", "Settings"],
            ] as const
          ).map(([value, label]) => (
            <TabsTrigger
              key={value}
              value={value}
              className="flex-none px-0.5 text-[13px] after:bottom-[-1px]! data-[state=active]:after:bg-primary"
            >
              {label}
              {value === "logs" && instance.running && <span className="size-1.5 animate-pulse rounded-full bg-success" />}
            </TabsTrigger>
          ))}
        </TabsList>
      </div>

      <TabsContent value="content" className="min-h-0 overflow-y-auto">
        <Page className="pt-3">
          <ContentManager instance={instance} />
        </Page>
      </TabsContent>
      <TabsContent value="logs" className="min-h-0 p-3">
        <LogViewer slug={instance.slug} live={instance.running} />
      </TabsContent>
      <TabsContent value="files" className="min-h-0 overflow-y-auto">
        <Page className="pt-3">
          <FileBrowser instance={instance} />
        </Page>
      </TabsContent>
      <TabsContent value="screenshots" className="min-h-0 overflow-y-auto">
        <Page className="pt-3">
          <ScreenshotsPage instance={instance.slug} />
        </Page>
      </TabsContent>
      <TabsContent value="settings" className="min-h-0 overflow-y-auto">
        <Page className="pt-3">
          <InstanceSettings key={instance.slug} instance={instance} />
        </Page>
      </TabsContent>
    </Tabs>
  );
}
