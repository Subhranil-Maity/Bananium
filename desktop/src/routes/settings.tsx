import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, Loader2, Moon, Sun, Terminal, Trash2 } from "lucide-react";
import { Link } from "react-router";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { Config } from "@/bindings/Config";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { DiscordSettings } from "@/components/discord-settings";
import { JavaPicker } from "@/components/java-picker";
import { Page, PageHeader, Section } from "@/components/page";
import { useJavaList, useRemoveRuntime } from "@/hooks/use-java";
import { errorMessage, run } from "@/lib/api";
import { formatBytes } from "@/lib/utils";
import { useTheme, type Theme } from "@/stores/theme";
import { usePresenceView } from "@/lib/presence";

const CONFIG_KEY = ["config"] as const;

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] items-start gap-6">
      <div className="pt-1.5">
        <Label className="text-[13px]">{label}</Label>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function PathRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="group grid grid-cols-[200px_1fr_auto] items-center gap-6 py-0.5">
      <span className="text-[13px] text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-xs select-text" title={value}>
        {value}
      </span>
      <Button
        variant="ghost"
        size="icon-sm"
        className="opacity-0 group-hover:opacity-100"
        title="Open"
        onClick={() => void openPath(value)}
      >
        <FolderOpen />
      </Button>
    </div>
  );
}

/** Settings form, seeded from the loaded config (remounted when it changes). */
function GameSettings({ config }: { config: Config }) {
  const queryClient = useQueryClient();
  const [downloads, setDownloads] = useState(String(config.max_concurrent_downloads));
  const [java, setJava] = useState(config.java_path ?? "");
  const downloadsInvalid = !/^\d+$/.test(downloads) || Number(downloads) < 1 || Number(downloads) > 64;
  const dirty = downloads !== String(config.max_concurrent_downloads) || java.trim() !== (config.java_path ?? "");

  const javaList = useJavaList();
  const removeRuntime = useRemoveRuntime();
  const runtimes = javaList.data?.filter((j) => j.source === "mojang") ?? [];

  const save = useMutation({
    mutationFn: () =>
      run(
        {
          command: "config_set",
          max_concurrent_downloads: Number(downloads),
          // "" clears back to Mojang's runtime per version.
          java_path: java.trim(),
        },
        "config_shown",
      ),
    onSuccess: (out) => {
      queryClient.setQueryData(CONFIG_KEY, out);
      toast.success("Settings saved");
    },
    onError: (err) => toast.error("Couldn't save settings", { description: errorMessage(err) }),
  });

  return (
    <Section
      title="Java & downloads"
      description="Stored in config.toml. Instances can override Java in their own settings."
      actions={
        <Button size="sm" disabled={!dirty || downloadsInvalid || save.isPending} onClick={() => save.mutate()}>
          {save.isPending && <Loader2 className="animate-spin" />}
          Save changes
        </Button>
      }
    >
      <Row
        label="Java"
        hint="By default each version runs on the Java runtime Mojang ships for it, downloaded automatically. Pick one here to use it for every instance instead."
      >
        <JavaPicker
          value={java.trim()}
          onChange={setJava}
          defaultLabel={
            <span className="flex items-center gap-2">
              <span className="font-medium">Automatic</span>
              <span className="text-muted-foreground">Mojang official runtime per version</span>
            </span>
          }
        />
      </Row>

      <Row label="Mojang runtimes" hint={`Downloaded into java/ and shared by every instance.`}>
        <div className="overflow-hidden rounded-md border">
          {javaList.isLoading && <Skeleton className="m-2 h-7" />}
          {!javaList.isLoading && runtimes.length === 0 && (
            <p className="px-2.5 py-2 text-xs text-muted-foreground">
              None yet. They download when you create or first launch an instance.
            </p>
          )}
          {runtimes.map((j) => (
            <div
              key={j.path}
              className="group flex items-center gap-3 border-b px-2.5 py-1.5 text-xs last:border-b-0 hover:bg-accent/40"
            >
              <span className="w-14 shrink-0 font-semibold tabular-nums">Java {j.major_version}</span>
              <span className="w-40 shrink-0 truncate font-mono">{j.component}</span>
              <span className="flex-1 truncate text-muted-foreground tabular-nums">
                {j.version} · {formatBytes(j.size_bytes ?? 0)}
              </span>
              <Button
                variant="ghost"
                size="icon-xs"
                className="opacity-0 group-hover:opacity-100"
                title="Show in folder"
                onClick={() => void openPath(j.path.replace(/[\\/]bin[\\/]java(\.exe)?$/, ""))}
              >
                <FolderOpen />
              </Button>
              <Button
                variant="ghost"
                size="icon-xs"
                className="text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-destructive"
                title="Remove (downloads again when needed)"
                disabled={removeRuntime.isPending}
                onClick={() => removeRuntime.mutate(j.component!)}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      </Row>

      <Row label="Parallel downloads" hint="1–64. Higher is faster on a fast line, but uses more memory.">
        <Input
          inputMode="numeric"
          className="w-24 tabular-nums"
          value={downloads}
          onChange={(e) => setDownloads(e.target.value)}
          aria-invalid={downloadsInvalid}
        />
      </Row>
    </Section>
  );
}

/** Appearance, Java and downloads, Discord, plus a read-only view of data paths. */
export function SettingsPage() {
  usePresenceView({ view: "settings" });
  const { theme, setTheme } = useTheme();
  const { data, isLoading, error } = useQuery({
    queryKey: CONFIG_KEY,
    queryFn: () => run({ command: "config_show" }, "config_shown"),
  });

  return (
    <Page className="max-w-4xl">
      <PageHeader title="Settings">
        <Button variant="outline" size="sm" asChild>
          <Link to="/console">
            <Terminal /> Console
          </Link>
        </Button>
      </PageHeader>
      <div className="space-y-4">
        <Section title="Appearance">
          <Row label="Theme">
            <ToggleGroup
              type="single"
              variant="outline"
              value={theme}
              onValueChange={(v) => v && setTheme(v as Theme)}
            >
              <ToggleGroupItem value="dark" className="px-3">
                <Moon /> Dark
              </ToggleGroupItem>
              <ToggleGroupItem value="light" className="px-3">
                <Sun /> Light
              </ToggleGroupItem>
            </ToggleGroup>
          </Row>
        </Section>

        {isLoading && <Skeleton className="h-60" />}
        {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
        {data && (
          <>
            <GameSettings key={JSON.stringify(data.config)} config={data.config} />
            <DiscordSettings key={`discord-${JSON.stringify(data.config.discord)}`} config={data.config} />
            <Section
              title="Data locations"
              description="Set the BANANIUM_HOME environment variable to move everything."
            >
              <div>
                <PathRow label="Data directory" value={data.paths.home} />
                <PathRow label="Instances" value={data.paths.instances_dir} />
                <PathRow label="Content store" value={data.paths.store_dir} />
                <PathRow label="Assets" value={data.paths.assets_dir} />
                <PathRow label="config.toml" value={data.paths.config_toml} />
                <PathRow label="Launcher logs" value={data.paths.logs_dir} />
              </div>
            </Section>
          </>
        )}
      </div>
    </Page>
  );
}
