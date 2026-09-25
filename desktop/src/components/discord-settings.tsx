import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import type { Config } from "@/bindings/Config";
import type { DiscordConfig } from "@/bindings/DiscordConfig";
import type { PresencePreview } from "@/bindings/PresencePreview";
import type { PresenceStatus } from "@/bindings/PresenceStatus";
import type { PreviewScenario } from "@/bindings/PreviewScenario";
import type { StatusDisplay } from "@/bindings/StatusDisplay";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Section } from "@/components/page";
import { errorMessage, run } from "@/lib/api";
import { PRESENCE_STATUS_KEY } from "@/lib/presence";
import { cn } from "@/lib/utils";

const CONFIG_KEY = ["config"] as const;

/** Mirrors `RETRY_INTERVAL_SECS` in `bananium-api`. */
const RETRY_INTERVAL_SECS = 5;

/** Where `bananium_discord::art` serves the presence images from. */
const ART_BASE_URL = "https://raw.githubusercontent.com/Subhranil-Maity/Bananium/main/assets/discord/";

/** Launcher art is bundled too, so the preview works offline and before it's pushed. */
function imageSrc(url: string | null) {
  if (!url) return null;
  return url.startsWith(ART_BASE_URL) ? `/discord/${url.slice(ART_BASE_URL.length)}` : url;
}

const SCENARIOS: { value: PreviewScenario; label: string }[] = [
  { value: "live", label: "Live" },
  { value: "playing", label: "Playing" },
  { value: "installing", label: "Installing" },
  { value: "browsing", label: "Browsing" },
  { value: "idle", label: "Idle" },
];

type Toggle = { key: keyof DiscordConfig; label: string; needs?: keyof DiscordConfig };

const PLAYING: Toggle[] = [
  { key: "show_version", label: "Minecraft version" },
  { key: "show_loader", label: "Mod loader" },
  { key: "show_loader_version", label: "Loader version", needs: "show_loader" },
  { key: "show_mod_count", label: "Mod count" },
  { key: "show_username", label: "Username" },
  { key: "show_instance_name", label: "Instance name" },
  { key: "instance_name_in_status", label: "Instance name as the title", needs: "show_instance_name" },
  { key: "show_modpack_icon", label: "Modpack icon" },
];

const LAUNCHER: Toggle[] = [
  { key: "show_browsing", label: "What I'm browsing", needs: "show_in_launcher" },
  { key: "show_tasks", label: "Installs & downloads", needs: "show_in_launcher" },
];

const GENERAL: Toggle[] = [
  { key: "show_elapsed", label: "Elapsed time" },
  { key: "show_buttons", label: "Link buttons" },
];

function usePresenceStatus() {
  return useQuery({
    queryKey: PRESENCE_STATUS_KEY,
    queryFn: async () => (await run({ command: "presence_status" }, "presence_status_shown")).status,
  });
}

function StatusBadge({ status }: { status: PresenceStatus | undefined }) {
  if (!status) return null;
  const [dot, text] = (() => {
    switch (status.state) {
      case "connected":
        return ["bg-success", "Connected to Discord"];
      case "connecting":
        return ["bg-warning animate-pulse", "Connecting…"];
      case "waiting":
        return ["bg-warning", "Waiting for Discord"];
      case "disabled":
        return ["bg-muted-foreground/50", "Off"];
    }
  })();
  return (
    <span className="flex items-center gap-2 text-xs">
      <span className={cn("size-2 rounded-full", dot)} />
      <span>{text}</span>
    </span>
  );
}

function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [active]);
  return now;
}

function clock(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** A look-alike of Discord's activity card, rendered from what the backend would send. */
function PreviewCard({ preview }: { preview: PresencePreview }) {
  const now = useNow(preview.start_ms !== null);
  const large = imageSrc(preview.large_image);
  const small = imageSrc(preview.small_image);
  const progress =
    preview.start_ms !== null && preview.end_ms !== null && preview.end_ms > preview.start_ms
      ? Math.min(1, (now - preview.start_ms) / (preview.end_ms - preview.start_ms))
      : null;
  return (
    <div className="w-full max-w-sm rounded-lg border bg-background p-3 shadow-sm">
      <p className="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">Playing</p>
      <div className="mt-2 flex gap-3">
        <div className="relative size-[72px] shrink-0">
          {large ? (
            <img src={large} alt="" title={preview.large_text ?? undefined} className="size-full rounded-lg object-cover" />
          ) : (
            <div className="size-full rounded-lg bg-muted" />
          )}
          {small && (
            <img
              src={small}
              alt=""
              title={preview.small_text ?? undefined}
              className="absolute -right-1.5 -bottom-1.5 size-7 rounded-full object-cover ring-4 ring-background"
            />
          )}
        </div>
        <div className="min-w-0 space-y-0.5 text-xs leading-snug">
          <p className="truncate text-sm font-semibold">{preview.app_name}</p>
          {preview.details && <p className="truncate">{preview.details}</p>}
          {preview.state && <p className="truncate text-muted-foreground">{preview.state}</p>}
          {progress !== null ? (
            <div className="flex items-center gap-2 pt-1 tabular-nums text-muted-foreground">
              <div className="h-1 w-24 overflow-hidden rounded-full bg-muted">
                <div className="h-full rounded-full bg-foreground" style={{ width: `${progress * 100}%` }} />
              </div>
              {clock(preview.end_ms! - now)} left
            </div>
          ) : (
            preview.start_ms !== null && (
              <p className="tabular-nums text-muted-foreground">{clock(now - preview.start_ms)} elapsed</p>
            )
          )}
        </div>
      </div>
      {preview.buttons.length > 0 && (
        <div className="mt-3 space-y-1.5">
          {preview.buttons.map((b) => (
            <div key={b.label} className="rounded-md bg-muted py-1.5 text-center text-xs font-medium" title={b.url}>
              {b.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SwitchRow({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label
      className={cn(
        "flex items-center justify-between gap-3 rounded-md px-2 py-1.5 text-[13px] hover:bg-accent/40",
        disabled && "opacity-50",
      )}
    >
      {label}
      <Switch size="sm" checked={checked} disabled={disabled} onCheckedChange={onChange} />
    </label>
  );
}

function ToggleGroupBlock({
  title,
  toggles,
  draft,
  set,
}: {
  title: string;
  toggles: Toggle[];
  draft: DiscordConfig;
  set: (key: keyof DiscordConfig, value: boolean) => void;
}) {
  return (
    <div>
      <p className="mb-1 px-2 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{title}</p>
      {toggles.map((t) => (
        <SwitchRow
          key={t.key}
          label={t.label}
          checked={draft[t.key] as boolean}
          disabled={!draft.enabled || (t.needs !== undefined && !draft[t.needs])}
          onChange={(v) => set(t.key, v)}
        />
      ))}
    </div>
  );
}

/** Settings → Discord: switches for every detail, with a live preview of the card. */
export function DiscordSettings({ config }: { config: Config }) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<DiscordConfig>(config.discord);
  const [scenario, setScenario] = useState<PreviewScenario>("live");
  const status = usePresenceStatus();
  const dirty = JSON.stringify(draft) !== JSON.stringify(config.discord);
  const set = (key: keyof DiscordConfig, value: boolean) => setDraft((d) => ({ ...d, [key]: value }));

  const preview = useQuery({
    queryKey: ["presence-preview", scenario, draft],
    queryFn: async () =>
      (await run({ command: "presence_preview", scenario, config: draft }, "presence_previewed")).preview,
    // "Live" follows what's actually going on.
    refetchInterval: scenario === "live" ? 3000 : false,
    placeholderData: (prev) => prev,
  });

  const save = useMutation({
    mutationFn: () => run({ command: "discord_config_set", config: draft }, "config_shown"),
    onSuccess: (out) => {
      queryClient.setQueryData(CONFIG_KEY, out);
      toast.success("Discord settings saved");
    },
    onError: (err) => toast.error("Couldn't save Discord settings", { description: errorMessage(err) }),
  });

  const reconnect = useMutation({
    mutationFn: () => run({ command: "presence_reconnect" }, "presence_status_shown"),
    onError: (err) => toast.error("Couldn't reconnect", { description: errorMessage(err) }),
  });

  const s = status.data;
  return (
    <Section
      title="Discord Rich Presence"
      description="Show what you're playing on your Discord profile. Needs the Discord desktop app running."
      actions={
        <Button size="sm" disabled={!dirty || save.isPending} onClick={() => save.mutate()}>
          {save.isPending && <Loader2 className="animate-spin" />}
          Save changes
        </Button>
      }
    >
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
        <label className="flex items-center gap-2.5 text-[13px] font-medium">
          <Switch checked={draft.enabled} onCheckedChange={(v) => set("enabled", v)} />
          Show my activity on Discord
        </label>
        <div className="ml-auto flex items-center gap-3">
          <StatusBadge status={s} />
          {(s?.state === "waiting" || s?.state === "connected") && (
            <Button
              variant="outline"
              size="sm"
              disabled={reconnect.isPending}
              onClick={() => reconnect.mutate()}
            >
              <RefreshCw className={cn(reconnect.isPending && "animate-spin")} />
              {s.state === "waiting" ? "Retry now" : "Reconnect"}
            </Button>
          )}
        </div>
      </div>
      {s?.state === "waiting" && (
        <p className="rounded-md border px-3 py-2 text-xs text-muted-foreground">
          Can't reach Discord ({s.error}). Bananium checks again every {RETRY_INTERVAL_SECS} seconds, so your
          activity appears as soon as the Discord desktop app is open.
        </p>
      )}

      <div className="grid gap-6 md:grid-cols-[1fr_minmax(0,22rem)]">
        <div className="grid content-start gap-4 sm:grid-cols-2">
          <ToggleGroupBlock title="While playing" toggles={PLAYING} draft={draft} set={set} />
          <div className="space-y-4">
            <div>
              <p className="mb-1 px-2 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
                In the launcher
              </p>
              <SwitchRow
                label="Show launcher activity"
                checked={draft.show_in_launcher}
                disabled={!draft.enabled}
                onChange={(v) => set("show_in_launcher", v)}
              />
              {LAUNCHER.map((t) => (
                <SwitchRow
                  key={t.key}
                  label={t.label}
                  checked={draft[t.key] as boolean}
                  disabled={!draft.enabled || !draft.show_in_launcher}
                  onChange={(v) => set(t.key, v)}
                />
              ))}
            </div>
            <ToggleGroupBlock title="General" toggles={GENERAL} draft={draft} set={set} />
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex items-center justify-between gap-2">
            <Label className="text-[13px]">Preview</Label>
            <select
              className="h-7 rounded-md border bg-background px-2 text-xs"
              value={scenario}
              onChange={(e) => setScenario(e.target.value as PreviewScenario)}
            >
              {SCENARIOS.map((sc) => (
                <option key={sc.value} value={sc.value}>
                  {sc.label}
                </option>
              ))}
            </select>
          </div>
          {preview.data ? (
            <PreviewCard preview={preview.data} />
          ) : (
            <p className="rounded-lg border border-dashed px-4 py-6 text-center text-xs text-muted-foreground">
              {preview.isLoading ? "Loading…" : "Nothing is shown right now."}
            </p>
          )}
          <div>
            <p className="mb-1.5 text-xs text-muted-foreground">Status in the member list</p>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              disabled={!draft.enabled}
              value={draft.status_display}
              onValueChange={(v) => v && setDraft((d) => ({ ...d, status_display: v as StatusDisplay }))}
            >
              <ToggleGroupItem value="name" className="px-2.5 text-xs">
                Bananium
              </ToggleGroupItem>
              <ToggleGroupItem value="details" className="px-2.5 text-xs">
                First line
              </ToggleGroupItem>
              <ToggleGroupItem value="state" className="px-2.5 text-xs">
                Second line
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
        </div>
      </div>
    </Section>
  );
}
