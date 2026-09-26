import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, FileArchive, Loader2, Package } from "lucide-react";
import { toast } from "sonner";

import type { ModpackSource } from "@/bindings/ModpackSource";
import type { ModrinthVersion } from "@/bindings/ModrinthVersion";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { VALID_INSTANCE_NAME } from "@/components/instance-actions";
import { Field, GroupPicker, uniqueInstanceName } from "@/components/instance-form-parts";
import { INSTANCES_KEY, useInstances } from "@/hooks/use-instances";
import { errorMessage, isAlreadyQueued, logAction, run } from "@/lib/api";
import { allGroups } from "@/lib/instances";
import { cn } from "@/lib/utils";
import { isActive, retryKey, useTasks } from "@/stores/tasks";

/** What the create dialog was opened for in modpack mode. */
export type ModpackTarget =
  | { source: "modrinth"; projectId: string; title: string; iconUrl: string | null; versionId?: string }
  | { source: "file"; path: string };

/** The newest stable version Bananium can run, else the newest it can run at all. */
function defaultVersion(versions: ModrinthVersion[]): ModrinthVersion | undefined {
  return (
    versions.find((v) => v.compatible && v.version_type === "release") ?? versions.find((v) => v.compatible)
  );
}

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1.5 text-[13px]">
      <span className="text-muted-foreground">{label}</span>
      <span className="truncate font-medium tabular-nums">{children}</span>
    </div>
  );
}

/**
 * Create an instance from a modpack: everything is decided by the pack, so
 * only the name (prefilled) and group are up to the user — plus which pack
 * version, for a Modrinth pack.
 */
export function ModpackForm({ modpack, onDone }: { modpack: ModpackTarget; onDone: () => void }) {
  const queryClient = useQueryClient();
  const { data: instances } = useInstances();
  const registerRetry = useTasks((s) => s.registerRetry);
  // Names of instances still being created count as taken too: the backend
  // refuses a second install under a pending name.
  const pendingNames = useTasks((s) =>
    Object.values(s.tasks)
      .filter((t) => isActive(t) && (t.kind === "modpack_install" || t.kind === "install") && t.instance)
      .map((t) => t.instance!.toLowerCase())
      .join("\n"),
  );
  const taken = new Set([
    ...(instances ?? []).map((i) => i.name.toLowerCase()),
    ...pendingNames.split("\n").filter(Boolean),
  ]);
  const groups = allGroups(instances);

  const versions = useQuery({
    // Same key and command as the project sheet's version list, so opening
    // this from the sheet reuses what it already loaded.
    queryKey: ["modrinth-versions", modpack.source === "modrinth" ? modpack.projectId : null, null],
    queryFn: async () =>
      (
        await run(
          { command: "modpack_versions", project: (modpack as { projectId: string }).projectId },
          "modrinth_versions_listed",
        )
      ).versions,
    enabled: modpack.source === "modrinth",
    staleTime: 5 * 60_000,
  });
  const inspected = useQuery({
    queryKey: ["modpack-inspect", modpack.source === "file" ? modpack.path : null],
    queryFn: async () =>
      (await run({ command: "modpack_inspect", path: (modpack as { path: string }).path }, "modpack_inspected")).pack,
    enabled: modpack.source === "file",
  });

  const [pickedVersion, setPickedVersion] = useState<string | null>(
    modpack.source === "modrinth" ? (modpack.versionId ?? null) : null,
  );
  const version =
    versions.data?.find((v) => v.id === pickedVersion) ?? (versions.data ? defaultVersion(versions.data) : undefined);

  const packName = modpack.source === "modrinth" ? modpack.title : (inspected.data?.name ?? "");
  const suggested = packName ? uniqueInstanceName(packName, taken) : "";
  const [name, setName] = useState<string | null>(null);
  const finalName = (name ?? suggested).trim();
  const nameInvalid = finalName !== "" && !VALID_INSTANCE_NAME.test(finalName);
  const nameTaken = name !== null && taken.has(finalName.toLowerCase());
  const [group, setGroup] = useState("");

  const mcVersion =
    modpack.source === "modrinth" ? (version?.game_versions.join(", ") ?? "") : (inspected.data?.mc_version ?? "");
  const loader =
    modpack.source === "modrinth"
      ? version?.loaders.map((l) => (l === "fabric" ? "Fabric" : l === "minecraft" ? "Vanilla" : l)).join(", ")
      : inspected.data
        ? inspected.data.loader_version
          ? `Fabric ${inspected.data.loader_version}`
          : "Vanilla"
        : "";
  const unsupported =
    modpack.source === "modrinth"
      ? version && !version.compatible
        ? `This version needs ${version.loaders.join(", ")}; Bananium only supports Fabric.`
        : versions.data && !version
          ? "No version of this pack runs on Fabric."
          : null
      : (inspected.data?.unsupported ?? null);

  const install = useMutation({
    mutationKey: ["modpack-install"],
    mutationFn: (args: { source: ModpackSource; name: string; group: string }) =>
      run(
        { command: "modpack_install", source: args.source, name: args.name, group: args.group || null },
        "installed",
      ),
    onMutate: (args) => {
      const project = args.source.type === "modrinth" ? args.source.project : null;
      registerRetry(retryKey("modpack_install", args.name, project), () => install.mutate(args));
    },
    onSuccess: (out, args) => {
      toast.success(`${args.name} is ready`, { description: `Minecraft ${out.mc_version}` });
      void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
    },
    onError: (err, args) => {
      if (isAlreadyQueued(err)) {
        toast.info(`${args.name} is already being installed`, { description: "It's in the task tray." });
        return;
      }
      toast.error(`Couldn't install ${args.name}`, {
        description: errorMessage(err),
        closeButton: true,
        duration: 15_000,
        action: { label: "Retry", onClick: () => install.mutate(args) },
      });
    },
  });

  function submit() {
    // The dialog closes right away, so a second click can only come from
    // re-opening it; the name check and the backend both catch that.
    if (install.isPending) return;
    const source: ModpackSource =
      modpack.source === "modrinth"
        ? { type: "modrinth", project: modpack.projectId, version: version!.id }
        : { type: "file", path: modpack.path };
    logAction("modpack_dialog_submitted", {
      name: finalName,
      project: modpack.source === "modrinth" ? modpack.projectId : null,
      version: modpack.source === "modrinth" ? version?.id : null,
      file: modpack.source === "file" ? modpack.path.split(/[\\/]/).pop() : null,
    });
    install.mutate({ source, name: finalName, group: group.trim() });
    // The task shows in the tray the moment it's queued; no need to hold
    // the dialog open.
    onDone();
  }

  const loading = modpack.source === "modrinth" ? versions.isLoading : inspected.isLoading;
  const error = modpack.source === "modrinth" ? versions.error : inspected.error;
  const ready = modpack.source === "modrinth" ? !!version : !!inspected.data;

  return (
    <>
      <DialogHeader>
        <DialogTitle>Install modpack</DialogTitle>
        <DialogDescription>
          The Minecraft version, loader and mods come from the pack. Pick a name and you're set.
        </DialogDescription>
      </DialogHeader>

      <div className="grid grid-cols-[112px_1fr] gap-5">
        <div className="space-y-2">
          {modpack.source === "modrinth" && modpack.iconUrl ? (
            <img src={modpack.iconUrl} alt="" className="size-28 rounded-lg bg-muted object-cover ring-1 ring-border" />
          ) : (
            <div className="flex size-28 items-center justify-center rounded-lg bg-muted ring-1 ring-border">
              {modpack.source === "file" ? (
                <FileArchive className="size-10 text-muted-foreground" />
              ) : (
                <Package className="size-10 text-muted-foreground" />
              )}
            </div>
          )}
          <p className="text-center text-[11px] leading-tight font-medium">{packName || "…"}</p>
        </div>

        <div className="space-y-4">
          <Field label="Name">
            <Input
              autoFocus
              value={name ?? suggested}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={nameInvalid || nameTaken}
            />
            {nameInvalid && <p className="text-xs text-destructive">Only letters, digits, '-' and '_' are allowed.</p>}
            {nameTaken && <p className="text-xs text-destructive">An instance with that name already exists.</p>}
          </Field>

          {modpack.source === "modrinth" && (
            <Field label="Pack version">
              <Select value={version?.id ?? ""} onValueChange={setPickedVersion} disabled={!versions.data}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={versions.isLoading ? "Loading…" : "Choose a version"} />
                </SelectTrigger>
                <SelectContent className="max-h-80">
                  {versions.data?.map((v) => (
                    <SelectItem key={v.id} value={v.id} disabled={!v.compatible}>
                      <span className="font-mono text-xs">{v.version_number}</span>
                      <span className="text-muted-foreground">
                        {v.game_versions.join(", ")}
                        {v.version_type !== "release" && ` · ${v.version_type}`}
                        {!v.compatible && ` · ${v.loaders.join(", ")}`}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          )}

          <div className="rounded-md border bg-muted/30 px-3 py-1">
            {loading ? (
              <Skeleton className="my-2 h-16" />
            ) : error ? (
              <p className="py-2 text-xs text-destructive">{errorMessage(error)}</p>
            ) : (
              <div className="divide-y">
                <InfoRow label="Minecraft">{mcVersion || "—"}</InfoRow>
                <InfoRow label="Loader">{loader || "—"}</InfoRow>
                {modpack.source === "file" && inspected.data && (
                  <>
                    <InfoRow label="Pack version">{inspected.data.version_id}</InfoRow>
                    <InfoRow label="Files to download">{inspected.data.file_count}</InfoRow>
                  </>
                )}
                {modpack.source === "modrinth" && version && (
                  <InfoRow label="Released">{new Date(version.date_published).toLocaleDateString()}</InfoRow>
                )}
              </div>
            )}
          </div>

          {unsupported && (
            <p className={cn("flex items-start gap-2 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive")}>
              <AlertTriangle className="mt-px size-3.5 shrink-0" /> {unsupported}
            </p>
          )}

          <Field label="Group">
            <GroupPicker value={group} onChange={setGroup} groups={groups} />
          </Field>
        </div>
      </div>

      <DialogFooter>
        <Button
          variant="ghost"
          onClick={() => {
            logAction("modpack_dialog_cancelled");
            onDone();
          }}
        >
          Cancel
        </Button>
        <Button
          disabled={!ready || !!unsupported || !finalName || nameInvalid || nameTaken || install.isPending}
          onClick={submit}
        >
          {install.isPending && <Loader2 className="animate-spin" />}
          Install
        </Button>
      </DialogFooter>
    </>
  );
}
