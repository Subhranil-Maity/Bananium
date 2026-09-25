import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, FileSearch, FolderOpen, Loader2, Moon, RefreshCw, Sun } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { Config } from "@/bindings/Config";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Page, PageHeader, Section } from "@/components/page";
import { errorMessage, run } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useTheme, type Theme } from "@/stores/theme";

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

  const javaList = useQuery({
    queryKey: ["java-list"],
    queryFn: async () => (await run({ command: "java_list" }, "java_listed")).installs,
    staleTime: Infinity,
  });

  const save = useMutation({
    mutationFn: () =>
      run(
        {
          command: "config_set",
          max_concurrent_downloads: Number(downloads),
          // "" clears back to auto-detection.
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

  async function browse() {
    const picked = await open({ multiple: false, title: "Choose a Java executable" });
    if (typeof picked === "string") setJava(picked);
  }

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
      <Row label="Java executable" hint="Used by every instance without its own override.">
        <div className="space-y-2">
          <div className="flex gap-2">
            <Input
              className="font-mono text-xs"
              placeholder="Auto-detect"
              value={java}
              onChange={(e) => setJava(e.target.value)}
            />
            <Button variant="outline" size="icon" title="Browse" onClick={() => void browse()}>
              <FileSearch />
            </Button>
          </div>
          <div className="overflow-hidden rounded-md border">
            <div className="flex h-8 items-center justify-between border-b bg-muted/30 pr-1 pl-2.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
              Detected installations
              <Button
                variant="ghost"
                size="icon-xs"
                title="Scan again"
                disabled={javaList.isFetching}
                onClick={() => void javaList.refetch()}
              >
                <RefreshCw className={javaList.isFetching ? "animate-spin" : ""} />
              </Button>
            </div>
            {javaList.isLoading && <Skeleton className="m-2 h-7" />}
            {javaList.data?.length === 0 && (
              <p className="px-2.5 py-2 text-xs text-muted-foreground">No Java installations found.</p>
            )}
            {javaList.data?.map((j) => (
              <button
                key={j.path}
                className={cn(
                  "flex w-full items-center gap-3 border-b px-2.5 py-1.5 text-left text-xs last:border-b-0 hover:bg-accent/60",
                  java === j.path && "bg-primary/[0.07]",
                )}
                onClick={() => setJava(j.path)}
              >
                <span className="w-14 shrink-0 font-semibold tabular-nums">Java {j.major_version}</span>
                <span className="flex-1 truncate font-mono text-muted-foreground">{j.path}</span>
                {java === j.path && <Check className="size-3.5 text-primary" />}
              </button>
            ))}
          </div>
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

/** Appearance, Java and downloads, plus a read-only view of data paths. */
export function SettingsPage() {
  const { theme, setTheme } = useTheme();
  const { data, isLoading, error } = useQuery({
    queryKey: CONFIG_KEY,
    queryFn: () => run({ command: "config_show" }, "config_shown"),
  });

  return (
    <Page className="max-w-4xl">
      <PageHeader title="Settings" />
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
              </div>
            </Section>
          </>
        )}
      </div>
    </Page>
  );
}
