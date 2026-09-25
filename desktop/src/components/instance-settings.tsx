import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, FileSearch, FolderInput, Loader2, Pencil, Trash2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Section } from "@/components/page";
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useInstanceDialogs } from "@/stores/instance-dialogs";

const RAM_MIN = 1024;
const RAM_MAX = 32768;
const RAM_PRESETS = [2048, 4096, 6144, 8192, 12288];

function formatMb(mb: number) {
  return mb % 1024 === 0 ? `${mb / 1024} GB` : `${(mb / 1024).toFixed(1)} GB`;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[180px_1fr] items-start gap-6">
      <div className="pt-1.5">
        <Label className="text-[13px]">{label}</Label>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/** Memory, Java and JVM arguments for one instance, plus management actions. */
export function InstanceSettings({ instance }: { instance: InstanceSummary }) {
  const queryClient = useQueryClient();
  const show = useInstanceDialogs((s) => s.show);
  const [customRam, setCustomRam] = useState(instance.ram_mb !== null);
  const [ram, setRam] = useState(instance.ram_mb ?? 4096);
  const [jvmArgs, setJvmArgs] = useState(instance.jvm_args.join("\n"));
  const [java, setJava] = useState(instance.java_path ?? "");

  const ramInvalid = customRam && (ram < 256 || ram > 262_144);
  const dirty =
    (customRam ? ram : null) !== instance.ram_mb ||
    jvmArgs.split("\n").map((a) => a.trim()).filter(Boolean).join("\n") !== instance.jvm_args.join("\n") ||
    java.trim() !== (instance.java_path ?? "");

  const save = useMutation({
    mutationFn: () =>
      run(
        {
          command: "instance_set",
          instance: instance.slug,
          // 0 and "" are the backend's "clear back to default" sentinels.
          ram_mb: customRam ? ram : 0,
          jvm_args: jvmArgs
            .split("\n")
            .map((a) => a.trim())
            .filter(Boolean),
          java_path: java.trim(),
          group: null,
        },
        "instance_updated",
      ),
    onSuccess: () => {
      toast.success("Settings saved");
      void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
    },
    onError: (err) => toast.error("Save failed", { description: errorMessage(err) }),
  });

  async function browseJava() {
    const picked = await open({ multiple: false, directory: false, title: "Choose a Java executable" });
    if (typeof picked === "string") setJava(picked);
  }

  return (
    <div className="max-w-3xl space-y-4">
      <Section
        title="Java & memory"
        description="Applied the next time the game starts."
        actions={
          <Button size="sm" disabled={!dirty || ramInvalid || save.isPending} onClick={() => save.mutate()}>
            {save.isPending && <Loader2 className="animate-spin" />}
            Save changes
          </Button>
        }
      >
        <Row label="Maximum memory" hint="The -Xmx heap cap. More isn't always faster.">
          <div className="space-y-3">
            <label className="flex items-center gap-2 text-[13px]">
              <Switch checked={customRam} onCheckedChange={setCustomRam} />
              {customRam ? "Custom" : "JVM default"}
            </label>
            {customRam && (
              <>
                <div className="flex items-center gap-3">
                  <Slider
                    min={RAM_MIN}
                    max={RAM_MAX}
                    step={512}
                    value={[ram]}
                    onValueChange={([v]) => setRam(v)}
                    className="flex-1"
                  />
                  <div className="flex items-center gap-1">
                    <Input
                      inputMode="numeric"
                      aria-invalid={ramInvalid}
                      className="w-20 text-right tabular-nums"
                      value={ram}
                      onChange={(e) => {
                        const n = Number(e.target.value.replace(/\D/g, ""));
                        if (!Number.isNaN(n)) setRam(n);
                      }}
                    />
                    <span className="text-xs text-muted-foreground">MB</span>
                  </div>
                </div>
                <div className="flex flex-wrap gap-1">
                  {RAM_PRESETS.map((mb) => (
                    <button
                      key={mb}
                      onClick={() => setRam(mb)}
                      className={cn(
                        "rounded border px-2 py-0.5 text-[11px] text-muted-foreground tabular-nums transition-colors hover:bg-accent hover:text-foreground",
                        ram === mb && "border-primary/60 bg-primary/10 text-foreground",
                      )}
                    >
                      {formatMb(mb)}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        </Row>

        <Row label="Java executable" hint="Overrides the global setting for this instance.">
          <div className="flex gap-2">
            <Input
              placeholder="Use global setting / auto-detect"
              value={java}
              onChange={(e) => setJava(e.target.value)}
              className="font-mono text-xs"
            />
            <Button variant="outline" size="icon" title="Browse" onClick={() => void browseJava()}>
              <FileSearch />
            </Button>
          </div>
        </Row>

        <Row label="JVM arguments" hint="One per line, appended after everything else.">
          <Textarea
            className="min-h-24 font-mono text-xs"
            placeholder={"-XX:+UseG1GC\n-XX:MaxGCPauseMillis=50"}
            value={jvmArgs}
            onChange={(e) => setJvmArgs(e.target.value)}
          />
        </Row>
      </Section>

      <Section title="Instance">
        <Row label="Game directory">
          <div className="truncate rounded-md border bg-muted/40 px-2.5 py-1.5 font-mono text-xs text-muted-foreground select-text">
            {instance.game_dir}
          </div>
        </Row>
        <Row label="Organise">
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => show("group", instance.slug)}>
              <FolderInput /> {instance.group ? `Group: ${instance.group}` : "Move to group"}
            </Button>
            <Button variant="outline" disabled={instance.running} onClick={() => show("rename", instance.slug)}>
              <Pencil /> Rename
            </Button>
            <Button variant="outline" onClick={() => show("clone", instance.slug)}>
              <Copy /> Duplicate
            </Button>
          </div>
        </Row>
      </Section>

      <section className="rounded-lg border border-destructive/30 bg-destructive/[0.04] px-4 py-3">
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-sm font-semibold">Delete instance</h2>
            <p className="text-xs text-muted-foreground">
              Removes the whole folder: worlds, mods, screenshots and logs. There's no undo.
            </p>
          </div>
          <Button variant="destructive" disabled={instance.running} onClick={() => show("delete", instance.slug)}>
            <Trash2 /> Delete
          </Button>
        </div>
      </section>
    </div>
  );
}
