import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

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
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { useApplyPreset, usePresets } from "@/hooks/use-presets";
import { errorMessage, run } from "@/lib/api";

/** Mirrors `bananium_instance::is_valid_name`: letters, digits, '-', '_'. */
const VALID_NAME = /^[A-Za-z0-9_-]+$/;
const LATEST = "latest";
const NO_PRESET = "__none__";

/** Create (install) a new instance; download progress shows in the task tray. */
export function NewInstanceDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [snapshots, setSnapshots] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [fabric, setFabric] = useState(true);
  const [loader, setLoader] = useState(LATEST);

  const versions = useQuery({
    queryKey: ["versions", snapshots],
    queryFn: () => run({ command: "version_list", include_snapshots: snapshots }, "version_listed"),
    enabled: open,
    staleTime: 10 * 60_000,
  });
  // Default to the latest release until the user picks something.
  const version = picked ?? versions.data?.latest_release ?? "";

  const loaders = useQuery({
    queryKey: ["fabric-loaders", version],
    queryFn: async () =>
      (await run({ command: "fabric_loader_list", mc_version: version }, "fabric_loader_listed")).loaders,
    enabled: open && fabric && version !== "",
    staleTime: 10 * 60_000,
  });
  const fabricUnsupported = fabric && loaders.data?.length === 0;

  const { data: presets } = usePresets();
  const [preset, setPreset] = useState(NO_PRESET);
  const applyPreset = useApplyPreset();

  const nameInvalid = name !== "" && !VALID_NAME.test(name);

  const install = useMutation({
    // The preset rides along as the mutation variable so it's the one
    // chosen at submit time, even if the dialog's state changes meanwhile.
    mutationFn: (_preset: string) =>
      run(
        {
          command: "install",
          version,
          name: name.trim() || null,
          fabric_loader: fabric ? loader : null,
        },
        "installed",
      ),
    onSuccess: (out, chosenPreset) => {
      toast.success(`Installed ${out.mc_version}`, { description: `Instance "${out.instance}" is ready` });
      void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      if (chosenPreset !== NO_PRESET) applyPreset.mutate({ preset: chosenPreset, instance: out.instance });
    },
    onError: (err) => toast.error("Install failed", { description: errorMessage(err) }),
  });

  function submit() {
    install.mutate(preset);
    // The download can take minutes and is tracked in the task tray, so
    // there's no reason to hold the dialog open for it.
    onOpenChange(false);
    setName("");
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New instance</DialogTitle>
          <DialogDescription>Downloads everything needed to play offline afterwards.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="instance-name">Name</Label>
            <Input
              id="instance-name"
              placeholder="random if left blank"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={nameInvalid}
            />
            {nameInvalid && (
              <p className="text-xs text-destructive">Only letters, digits, '-' and '_' are allowed.</p>
            )}
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>Minecraft version</Label>
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                <Switch checked={snapshots} onCheckedChange={setSnapshots} /> Show snapshots
              </label>
            </div>
            <Select value={version} onValueChange={setPicked} disabled={!versions.data}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder={versions.isLoading ? "Loading…" : "Choose a version"} />
              </SelectTrigger>
              <SelectContent className="max-h-72">
                {versions.data?.versions.map((v) => (
                  <SelectItem key={v.id} value={v.id}>
                    {v.id}
                    {v.kind !== "release" && <span className="text-muted-foreground"> · {v.kind}</span>}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {versions.error && <p className="text-xs text-destructive">{errorMessage(versions.error)}</p>}
          </div>

          <div className="space-y-2 rounded-md border p-3">
            <label className="flex items-center justify-between text-sm font-medium">
              Fabric mod loader
              <Switch checked={fabric} onCheckedChange={setFabric} />
            </label>
            <p className="text-xs text-muted-foreground">
              Needed for mods and shaders. Resource packs work either way.
            </p>
            {fabric && (
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
            )}
            {fabricUnsupported && (
              <p className="text-xs text-destructive">Fabric doesn't support Minecraft {version}.</p>
            )}
          </div>

          {presets && presets.length > 0 && (
            <div className="space-y-2">
              <Label>Start from preset</Label>
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
                  Without Fabric, only the preset's resource packs will be installed.
                </p>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={nameInvalid || !version || fabricUnsupported} onClick={submit}>
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
