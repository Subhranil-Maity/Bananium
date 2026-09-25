import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ImagePlus, X } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { create } from "zustand";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { VALID_INSTANCE_NAME } from "@/components/instance-actions";
import { InstanceIcon } from "@/components/instance-icon";
import { INSTANCES_KEY, pickIconFile, useInstances } from "@/hooks/use-instances";
import { useApplyPreset, usePresets } from "@/hooks/use-presets";
import { errorMessage, run } from "@/lib/api";
import { allGroups } from "@/lib/instances";
import { cn } from "@/lib/utils";

const LATEST = "latest";
const NO_PRESET = "__none__";

/** Open state for the dialog, so the rail, library and palette can all open it. */
export const useNewInstance = create<{ open: boolean; setOpen: (open: boolean) => void }>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));

/** "Fabric-1_21_1", made unique against existing names with a numeric suffix. */
function suggestName(loader: "vanilla" | "fabric", version: string, taken: Set<string>): string {
  const base = `${loader === "fabric" ? "Fabric" : "Vanilla"}-${version.replace(/[^A-Za-z0-9_-]/g, "_")}`;
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base}-${n}`.toLowerCase())) return `${base}-${n}`;
}

function Field({ label, children, aside }: { label: string; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <div className="flex h-5 items-center justify-between">
        <Label className="text-xs font-medium text-muted-foreground">{label}</Label>
        {aside}
      </div>
      {children}
    </div>
  );
}

/** The dialog body; remounted on every open so the form starts fresh. */
function NewInstanceForm({ onDone }: { onDone: () => void }) {
  const queryClient = useQueryClient();
  const { data: instances } = useInstances();
  const [name, setName] = useState("");
  const [group, setGroup] = useState("");
  const [icon, setIcon] = useState<string | null>(null);
  const [snapshots, setSnapshots] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [loaderKind, setLoaderKind] = useState<"vanilla" | "fabric">("fabric");
  const [loader, setLoader] = useState(LATEST);
  const fabric = loaderKind === "fabric";

  const versions = useQuery({
    queryKey: ["versions", snapshots],
    queryFn: () => run({ command: "version_list", include_snapshots: snapshots }, "version_listed"),
    staleTime: 10 * 60_000,
  });
  // Default to the latest release until the user picks something.
  const version = picked ?? versions.data?.latest_release ?? "";

  const loaders = useQuery({
    queryKey: ["fabric-loaders", version],
    queryFn: async () =>
      (await run({ command: "fabric_loader_list", mc_version: version }, "fabric_loader_listed")).loaders,
    enabled: fabric && version !== "",
    staleTime: 10 * 60_000,
  });
  const fabricUnsupported = fabric && loaders.data?.length === 0;

  const { data: presets } = usePresets();
  const [preset, setPreset] = useState(NO_PRESET);
  const applyPreset = useApplyPreset();

  const taken = new Set((instances ?? []).map((i) => i.name.toLowerCase()));
  const suggested = version ? suggestName(loaderKind, version, taken) : "";
  const finalName = name.trim() || suggested;
  const nameInvalid = name !== "" && !VALID_INSTANCE_NAME.test(name.trim());
  const nameTaken = name !== "" && taken.has(name.trim().toLowerCase());
  const groups = allGroups(instances);

  const install = useMutation({
    // Everything chosen rides along as the mutation variable, so it's what
    // was on screen at submit time even though the dialog closes at once.
    mutationFn: (args: { name: string; group: string; icon: string | null; preset: string }) =>
      run(
        {
          command: "install",
          version,
          name: args.name,
          fabric_loader: fabric ? loader : null,
          group: args.group || null,
        },
        "installed",
      ),
    onSuccess: async (out, args) => {
      if (args.icon) {
        try {
          await run({ command: "instance_set_icon", instance: out.instance, path: args.icon }, "instance_updated");
        } catch (err) {
          toast.error("Couldn't set the icon", { description: errorMessage(err) });
        }
      }
      toast.success(`${args.name} is ready`, { description: `Minecraft ${out.mc_version}` });
      void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      if (args.preset !== NO_PRESET) applyPreset.mutate({ preset: args.preset, instance: out.instance });
    },
    onError: (err, args) => toast.error(`Couldn't create ${args.name}`, { description: errorMessage(err) }),
  });

  function submit() {
    install.mutate({ name: finalName, group: group.trim(), icon, preset });
    // The download can take minutes and is tracked in the task tray, so
    // there's no reason to hold the dialog open for it.
    onDone();
  }

  const previewIcon = { name: finalName || "new", icon_path: icon, running: false };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Create instance</DialogTitle>
        <DialogDescription>Everything is downloaded up front, so it plays offline afterwards.</DialogDescription>
      </DialogHeader>

      <div className="grid grid-cols-[112px_1fr] gap-5">
        <div className="space-y-2">
          <button
            type="button"
            className="group relative block rounded-lg focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            title="Choose icon"
            onClick={() => void pickIconFile().then((p) => p && setIcon(p))}
          >
            {icon ? (
              <img src={convertFileSrc(icon)} alt="" className="pixelated size-28 rounded-lg object-cover ring-1 ring-border" />
            ) : (
              <InstanceIcon instance={previewIcon} className="size-28" />
            )}
            <span className="absolute inset-0 flex items-center justify-center rounded-lg bg-black/55 text-white opacity-0 transition-opacity group-hover:opacity-100">
              <ImagePlus className="size-5" />
            </span>
          </button>
          {icon && (
            <Button variant="ghost" size="xs" className="w-full text-muted-foreground" onClick={() => setIcon(null)}>
              <X /> Remove icon
            </Button>
          )}
        </div>

        <div className="space-y-4">
          <Field label="Name">
            <Input
              placeholder={suggested}
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={nameInvalid || nameTaken}
            />
            {nameInvalid && <p className="text-xs text-destructive">Only letters, digits, '-' and '_' are allowed.</p>}
            {nameTaken && <p className="text-xs text-destructive">An instance with that name already exists.</p>}
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field
              label="Game version"
              aside={
                <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  Snapshots
                  <Switch className="scale-75" checked={snapshots} onCheckedChange={setSnapshots} />
                </label>
              }
            >
              <Select value={version} onValueChange={setPicked} disabled={!versions.data}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={versions.isLoading ? "Loading…" : "Choose a version"} />
                </SelectTrigger>
                <SelectContent className="max-h-80">
                  {versions.data?.versions.map((v) => (
                    <SelectItem key={v.id} value={v.id}>
                      {v.id}
                      {v.kind !== "release" && <span className="text-muted-foreground"> · {v.kind}</span>}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {versions.error && <p className="text-xs text-destructive">{errorMessage(versions.error)}</p>}
            </Field>

            <Field label="Loader">
              <ToggleGroup
                type="single"
                variant="outline"
                className="w-full"
                value={loaderKind}
                onValueChange={(v) => v && setLoaderKind(v as "vanilla" | "fabric")}
              >
                <ToggleGroupItem value="vanilla" className="flex-1">
                  Vanilla
                </ToggleGroupItem>
                <ToggleGroupItem value="fabric" className="flex-1">
                  Fabric
                </ToggleGroupItem>
              </ToggleGroup>
            </Field>
          </div>

          {fabric && (
            <Field label="Fabric loader version">
              <Select value={loader} onValueChange={setLoader} disabled={!loaders.data?.length}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="max-h-72">
                  <SelectItem value={LATEST}>Latest stable</SelectItem>
                  {loaders.data?.map((l) => (
                    <SelectItem key={l.version} value={l.version}>
                      {l.version}
                      {!l.stable && <span className="text-muted-foreground"> · beta</span>}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {fabricUnsupported && (
                <p className="text-xs text-destructive">Fabric doesn't support Minecraft {version}.</p>
              )}
            </Field>
          )}

          <Field label="Group">
            <Input placeholder="None" value={group} onChange={(e) => setGroup(e.target.value)} />
            {groups.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {groups.map((g) => (
                  <button
                    key={g}
                    type="button"
                    className={cn(
                      "rounded border px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                      g === group && "border-primary/60 text-foreground",
                    )}
                    onClick={() => setGroup(g === group ? "" : g)}
                  >
                    {g}
                  </button>
                ))}
              </div>
            )}
          </Field>

          {presets && presets.length > 0 && (
            <Field label="Start from preset">
              <Select value={preset} onValueChange={setPreset}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_PRESET}>None</SelectItem>
                  {presets.map((p) => (
                    <SelectItem key={p.name} value={p.name}>
                      {p.name} · {p.entries.length} item(s)
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {preset !== NO_PRESET && !fabric && (
                <p className="text-xs text-muted-foreground">
                  Without Fabric, only the preset's resource packs are installed.
                </p>
              )}
            </Field>
          )}
        </div>
      </div>

      <DialogFooter>
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!finalName || nameInvalid || nameTaken || !version || fabricUnsupported} onClick={submit}>
          Create
        </Button>
      </DialogFooter>
    </>
  );
}

/** Create (install) a new instance; download progress shows in the task tray. */
export function NewInstanceDialog() {
  const { open, setOpen } = useNewInstance();
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-2xl">{open && <NewInstanceForm onDone={() => setOpen(false)} />}</DialogContent>
    </Dialog>
  );
}
