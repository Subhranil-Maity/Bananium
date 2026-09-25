import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FileSearch, Loader2, RefreshCw } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

import type { Config } from "@/bindings/Config";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { errorMessage, run } from "@/lib/api";
import { useTheme } from "@/stores/theme";

const CONFIG_KEY = ["config"] as const;

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[160px_1fr] gap-4 py-1.5 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-xs select-text" title={value}>
        {value}
      </span>
    </div>
  );
}

/** Settings form, seeded from the loaded config (remounted when it changes). */
function GeneralSettings({ config }: { config: Config }) {
  const queryClient = useQueryClient();
  const [downloads, setDownloads] = useState(String(config.max_concurrent_downloads));
  const [java, setJava] = useState(config.java_path ?? "");
  const downloadsInvalid = !/^\d+$/.test(downloads) || Number(downloads) < 1 || Number(downloads) > 64;

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
    <Card>
      <CardHeader>
        <CardTitle>Game &amp; downloads</CardTitle>
        <CardDescription>Stored in config.toml; instances can override Java in their own settings.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="space-y-2">
          <Label htmlFor="downloads">Parallel downloads</Label>
          <Input
            id="downloads"
            inputMode="numeric"
            className="w-32"
            value={downloads}
            onChange={(e) => setDownloads(e.target.value)}
            aria-invalid={downloadsInvalid}
          />
          <p className="text-xs text-muted-foreground">1–64. More is faster on a fast connection but uses more memory.</p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="java">Java executable</Label>
          <div className="flex gap-2">
            <Input
              id="java"
              className="font-mono text-xs"
              placeholder="Auto-detect"
              value={java}
              onChange={(e) => setJava(e.target.value)}
            />
            <Button variant="outline" size="icon" title="Browse" onClick={() => void browse()}>
              <FileSearch />
            </Button>
          </div>
          <div className="space-y-1 rounded-md border p-2">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              Detected on this computer
              <Button
                variant="ghost"
                size="icon"
                className="size-6"
                title="Scan again"
                disabled={javaList.isFetching}
                onClick={() => void javaList.refetch()}
              >
                <RefreshCw className={javaList.isFetching ? "animate-spin" : ""} />
              </Button>
            </div>
            {javaList.isLoading && <Skeleton className="h-8" />}
            {javaList.data?.length === 0 && <p className="text-xs text-muted-foreground">No Java installations found.</p>}
            {javaList.data?.map((j) => (
              <button
                key={j.path}
                className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-accent data-[on=true]:bg-accent"
                data-on={java === j.path}
                onClick={() => setJava(j.path)}
              >
                <span className="w-16 shrink-0 font-medium">Java {j.major_version}</span>
                <span className="truncate font-mono text-muted-foreground">{j.path}</span>
              </button>
            ))}
          </div>
        </div>

        <Button disabled={downloadsInvalid || save.isPending} onClick={() => save.mutate()}>
          {save.isPending && <Loader2 className="animate-spin" />}
          Save
        </Button>
      </CardContent>
    </Card>
  );
}

/** Appearance, downloads and Java, plus a read-only view of data paths. */
export function SettingsPage() {
  const { theme, setTheme } = useTheme();
  const { data, isLoading, error } = useQuery({
    queryKey: CONFIG_KEY,
    queryFn: () => run({ command: "config_show" }, "config_shown"),
  });

  return (
    <div className="max-w-3xl space-y-6">
      <h1 className="text-2xl font-semibold">Settings</h1>

      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
        </CardHeader>
        <CardContent className="flex items-center gap-3">
          <Switch id="dark" checked={theme === "dark"} onCheckedChange={(on) => setTheme(on ? "dark" : "banana")} />
          <Label htmlFor="dark">Dark mode</Label>
        </CardContent>
      </Card>

      {isLoading && <Skeleton className="h-60" />}
      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {data && (
        <>
          <GeneralSettings key={JSON.stringify(data.config)} config={data.config} />
          <Card>
            <CardHeader>
              <CardTitle>Data locations</CardTitle>
              <CardDescription>Set the BANANIUM_HOME environment variable to move everything.</CardDescription>
            </CardHeader>
            <CardContent>
              <Row label="Data directory" value={data.paths.home} />
              <Row label="Instances" value={data.paths.instances_dir} />
              <Row label="Store" value={data.paths.store_dir} />
              <Row label="Assets" value={data.paths.assets_dir} />
              <Row label="config.toml" value={data.paths.config_toml} />
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
