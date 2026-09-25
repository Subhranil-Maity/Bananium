import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Download, FileDown, FileUp, Loader2, Pencil, Plus, Trash2 } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

import type { ContentKind } from "@/bindings/ContentKind";
import type { Preset } from "@/bindings/Preset";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useInstances } from "@/hooks/use-instances";
import { PRESETS_KEY, useApplyPreset, usePresets, useSavePreset } from "@/hooks/use-presets";
import { errorMessage, run } from "@/lib/api";
import { KINDS } from "@/lib/content";

/** Mirrors `bananium_instance::presets::is_valid_preset_name`. */
function validPresetName(name: string) {
  return name.trim() === name && name.length > 0 && name.length <= 64 && !/[/\\]/.test(name);
}

function SavePresetDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const { data: instances } = useInstances();
  const savePreset = useSavePreset();
  const [instance, setInstance] = useState<string>("");
  const [name, setName] = useState("");
  const [kinds, setKinds] = useState<ContentKind[]>(["mod", "resource_pack", "shader"]);
  const invalid = name !== "" && !validPresetName(name);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Save a preset</DialogTitle>
          <DialogDescription>
            Captures the Modrinth content you picked in an instance. Dependencies are re-resolved when the preset is
            applied, and local files can't be included.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label>From instance</Label>
            <Select value={instance} onValueChange={setInstance}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Choose an instance" />
              </SelectTrigger>
              <SelectContent>
                {instances?.map((i) => (
                  <SelectItem key={i.slug} value={i.slug}>
                    {i.name} · {i.mc_version}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="preset-name">Name</Label>
            <Input
              id="preset-name"
              placeholder="e.g. Performance + Shaders"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={invalid}
            />
          </div>
          <div className="space-y-2">
            <Label>Include</Label>
            {KINDS.map((k) => (
              <label key={k.kind} className="flex items-center justify-between text-sm">
                {k.label}
                <Switch
                  checked={kinds.includes(k.kind)}
                  onCheckedChange={(on) =>
                    setKinds((ks) => (on ? [...ks, k.kind] : ks.filter((x) => x !== k.kind)))
                  }
                />
              </label>
            ))}
          </div>
        </div>
        <DialogFooter>
          <Button
            disabled={!instance || !name || invalid || kinds.length === 0 || savePreset.isPending}
            onClick={() =>
              savePreset.mutate(
                { instance, name, kinds },
                {
                  onSuccess: () => {
                    onOpenChange(false);
                    setName("");
                  },
                },
              )
            }
          >
            {savePreset.isPending && <Loader2 className="animate-spin" />}
            Save preset
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function PresetCard({ preset }: { preset: Preset }) {
  const queryClient = useQueryClient();
  const { data: instances } = useInstances();
  const apply = useApplyPreset();
  const [target, setTarget] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const refresh = () => queryClient.invalidateQueries({ queryKey: PRESETS_KEY });

  const remove = useMutation({
    mutationFn: () => run({ command: "preset_delete", name: preset.name }, "preset_deleted"),
    onSuccess: refresh,
    onError: (err) => toast.error("Couldn't delete", { description: errorMessage(err) }),
  });
  const rename = useMutation({
    mutationFn: (newName: string) =>
      run({ command: "preset_rename", name: preset.name, new_name: newName }, "preset_renamed"),
    onSuccess: () => {
      setRenaming(null);
      void refresh();
    },
    onError: (err) => toast.error("Couldn't rename", { description: errorMessage(err) }),
  });

  async function exportPreset() {
    const path = await save({
      defaultPath: `${preset.name}.bananium-preset.toml`,
      filters: [{ name: "Bananium preset", extensions: ["toml"] }],
    });
    if (!path) return;
    try {
      await run({ command: "preset_export", name: preset.name, path }, "preset_exported");
      toast.success("Preset exported");
    } catch (err) {
      toast.error("Export failed", { description: errorMessage(err) });
    }
  }

  const counts = KINDS.map((k) => [k.label, preset.entries.filter((e) => e.kind === k.kind).length] as const).filter(
    ([, n]) => n > 0,
  );

  return (
    <Card className="gap-3">
      <CardHeader>
        {renaming !== null ? (
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (validPresetName(renaming)) rename.mutate(renaming);
            }}
          >
            <Input autoFocus value={renaming} onChange={(e) => setRenaming(e.target.value)} />
            <Button type="submit" size="sm" disabled={!validPresetName(renaming)}>
              Save
            </Button>
          </form>
        ) : (
          <CardTitle className="flex items-center gap-2">
            <span className="truncate">{preset.name}</span>
            <Badge variant="secondary">
              {preset.loader === "fabric" ? "Fabric" : "Vanilla"} {preset.mc_version}
            </Badge>
          </CardTitle>
        )}
        <CardDescription>{counts.map(([label, n]) => `${n} ${label.toLowerCase()}`).join(" · ") || "Empty"}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap gap-1.5">
          {preset.entries.map((e) => (
            <span key={e.project_id} className="flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs">
              {e.icon_url && <img src={e.icon_url} alt="" className="size-4 rounded-sm" />}
              {e.title}
            </span>
          ))}
        </div>
        <div className="flex gap-2">
          <Select value={target} onValueChange={setTarget}>
            <SelectTrigger className="flex-1">
              <SelectValue placeholder="Apply to instance…" />
            </SelectTrigger>
            <SelectContent>
              {instances?.map((i) => (
                <SelectItem key={i.slug} value={i.slug}>
                  {i.name} · {i.mc_version}
                  {i.loader === "fabric" ? " · Fabric" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button disabled={!target || apply.isPending} onClick={() => apply.mutate({ preset: preset.name, instance: target })}>
            {apply.isPending ? <Loader2 className="animate-spin" /> : <Download />}
            Apply
          </Button>
        </div>
        <div className="flex justify-end gap-1">
          <Button variant="ghost" size="icon" title="Rename" onClick={() => setRenaming(preset.name)}>
            <Pencil />
          </Button>
          <Button variant="ghost" size="icon" title="Export" onClick={() => void exportPreset()}>
            <FileDown />
          </Button>
          <Button variant="ghost" size="icon" title="Delete" onClick={() => remove.mutate()}>
            <Trash2 />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

/** Saved content presets: create from an instance, apply to others, share as files. */
export function PresetsPage() {
  const queryClient = useQueryClient();
  const { data: presets, isLoading, error } = usePresets();
  const [saving, setSaving] = useState(false);

  async function importPreset() {
    const path = await open({ multiple: false, filters: [{ name: "Bananium preset", extensions: ["toml"] }] });
    if (typeof path !== "string") return;
    try {
      const out = await run({ command: "preset_import", path }, "preset_imported");
      toast.success(`Imported "${out.preset.name}"`);
      void queryClient.invalidateQueries({ queryKey: PRESETS_KEY });
    } catch (err) {
      toast.error("Import failed", { description: errorMessage(err) });
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h1 className="text-2xl font-semibold">Presets</h1>
          <p className="text-sm text-muted-foreground">
            Reusable sets of mods, resource packs and shaders. Applying one picks the right version of each project for
            the target instance.
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => void importPreset()}>
            <FileUp /> Import
          </Button>
          <Button onClick={() => setSaving(true)}>
            <Plus /> New preset
          </Button>
        </div>
      </div>
      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {isLoading && <Skeleton className="h-40" />}
      {presets?.length === 0 && (
        <div className="rounded-lg border border-dashed p-12 text-center text-muted-foreground">
          No presets yet. Set up an instance the way you like it, then save it as a preset.
        </div>
      )}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] gap-4">
        {presets?.map((p) => (
          <PresetCard key={p.name} preset={p} />
        ))}
      </div>
      <SavePresetDialog open={saving} onOpenChange={setSaving} />
    </div>
  );
}
