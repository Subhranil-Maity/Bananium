import { useState } from "react";
import { useNavigate, useParams } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Copy, FolderOpen, Loader2, Pencil, Terminal, Trash2 } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ContentManager } from "@/components/content-manager";
import { DryRunDialog } from "@/components/dry-run-dialog";
import { InstanceSettings } from "@/components/instance-settings";
import { LoaderBadge } from "@/components/loader-badge";
import { LogViewer } from "@/components/log-viewer";
import { PlayButton } from "@/components/play-button";
import { INSTANCES_KEY, useInstance } from "@/hooks/use-instances";
import { ScreenshotsPage } from "@/routes/screenshots";
import { errorMessage, run } from "@/lib/api";

const VALID_NAME = /^[A-Za-z0-9_-]+$/;

/** Rename or clone: both just ask for a new valid instance name. */
function NameDialog({
  instance,
  mode,
  onOpenChange,
}: {
  instance: InstanceSummary;
  mode: "rename" | "clone" | null;
  onOpenChange: (open: boolean) => void;
}) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const invalid = name !== "" && !VALID_NAME.test(name);

  const submit = useMutation({
    mutationFn: async () => {
      if (mode === "rename") {
        return (
          await run(
            { command: "instance_rename", instance: instance.slug, new_name: name },
            "instance_renamed",
          )
        ).instance;
      }
      return (
        await run({ command: "instance_clone", instance: instance.slug, new_name: name }, "instance_cloned")
      ).instance;
    },
    onSuccess: async (slug) => {
      await queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      toast.success(mode === "rename" ? "Renamed" : "Cloned");
      onOpenChange(false);
      setName("");
      navigate(`/instance/${slug}`, { replace: mode === "rename" });
    },
    onError: (err) => toast.error("Failed", { description: errorMessage(err) }),
  });

  return (
    <Dialog open={mode !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{mode === "rename" ? "Rename instance" : "Clone instance"}</DialogTitle>
        </DialogHeader>
        <Input
          autoFocus
          placeholder={mode === "clone" ? `${instance.name}-copy` : instance.name}
          value={name}
          onChange={(e) => setName(e.target.value)}
          aria-invalid={invalid}
        />
        {invalid && <p className="text-xs text-destructive">Only letters, digits, '-' and '_' are allowed.</p>}
        <DialogFooter>
          <Button disabled={!name || invalid || submit.isPending} onClick={() => submit.mutate()}>
            {submit.isPending && <Loader2 className="animate-spin" />}
            {mode === "rename" ? "Rename" : "Clone"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** One instance: play/stop, logs, settings, and management actions. */
export function InstancePage() {
  const { slug } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: instance, isLoading } = useInstance(slug);
  const [nameMode, setNameMode] = useState<"rename" | "clone" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [dryRun, setDryRun] = useState(false);

  const remove = useMutation({
    mutationFn: (s: string) => run({ command: "instance_remove", instance: s }, "instance_removed"),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
      toast.success("Instance deleted");
      navigate("/");
    },
    onError: (err) => toast.error("Delete failed", { description: errorMessage(err) }),
  });

  if (isLoading) return null;
  if (!instance) {
    return (
      <div className="space-y-4">
        <p className="text-muted-foreground">Instance not found.</p>
        <Button variant="outline" onClick={() => navigate("/")}>
          <ArrowLeft /> Back to library
        </Button>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex items-center gap-3">
        <Button variant="ghost" size="icon" onClick={() => navigate("/")}>
          <ArrowLeft />
        </Button>
        <div className="min-w-0 flex-1">
          <h1 className="flex items-center gap-2 truncate text-2xl font-semibold">
            {instance.name}
            {instance.running && <Badge className="bg-green-600 text-white">Running</Badge>}
          </h1>
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            Minecraft {instance.mc_version} <LoaderBadge instance={instance} />
          </p>
        </div>
        <Button variant="outline" size="icon" title="Open folder" onClick={() => void openPath(instance.game_dir)}>
          <FolderOpen />
        </Button>
        <Button variant="outline" size="icon" title="Preview launch command" onClick={() => setDryRun(true)}>
          <Terminal />
        </Button>
        <PlayButton instance={instance} className="w-28" />
      </div>

      <Tabs defaultValue="content" className="flex min-h-0 flex-1 flex-col">
        <TabsList>
          <TabsTrigger value="content">Content</TabsTrigger>
          <TabsTrigger value="screenshots">Screenshots</TabsTrigger>
          <TabsTrigger value="logs">Logs</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>
        <TabsContent value="screenshots" className="overflow-y-auto">
          <ScreenshotsPage instance={instance.slug} />
        </TabsContent>
        <TabsContent value="content" className="overflow-y-auto">
          <ContentManager instance={instance} />
        </TabsContent>
        <TabsContent value="logs" className="min-h-0 flex-1">
          <LogViewer slug={instance.slug} live={instance.running} />
        </TabsContent>
        <TabsContent value="settings" className="space-y-6 overflow-y-auto">
          <InstanceSettings key={instance.slug} instance={instance} />
          <div className="max-w-2xl space-y-3 rounded-lg border p-4">
            <h2 className="font-medium">Manage</h2>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" disabled={instance.running} onClick={() => setNameMode("rename")}>
                <Pencil /> Rename
              </Button>
              <Button variant="outline" onClick={() => setNameMode("clone")}>
                <Copy /> Clone
              </Button>
              <Button variant="destructive" disabled={instance.running} onClick={() => setConfirmDelete(true)}>
                <Trash2 /> Delete
              </Button>
            </div>
          </div>
        </TabsContent>
      </Tabs>

      <NameDialog instance={instance} mode={nameMode} onOpenChange={(o) => !o && setNameMode(null)} />
      <DryRunDialog slug={instance.slug} open={dryRun} onOpenChange={setDryRun} />
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {instance.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes the instance, including its worlds, mods and screenshots.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() => remove.mutate(instance.slug)}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
