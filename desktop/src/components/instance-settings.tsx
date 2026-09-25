import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FileSearch, Loader2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { errorMessage, run } from "@/lib/api";

/** Memory, JVM arguments and Java override for one instance. */
export function InstanceSettings({ instance }: { instance: InstanceSummary }) {
  const queryClient = useQueryClient();
  const [ram, setRam] = useState(instance.ram_mb?.toString() ?? "");
  const [jvmArgs, setJvmArgs] = useState(instance.jvm_args.join("\n"));
  const [java, setJava] = useState(instance.java_path ?? "");

  const ramInvalid = ram !== "" && !/^\d+$/.test(ram);

  const save = useMutation({
    mutationFn: () =>
      run(
        {
          command: "instance_set",
          instance: instance.slug,
          // 0 and "" are the backend's "clear back to default" sentinels.
          ram_mb: ram === "" ? 0 : Number(ram),
          jvm_args: jvmArgs
            .split("\n")
            .map((a) => a.trim())
            .filter(Boolean),
          java_path: java.trim(),
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
    <div className="max-w-2xl space-y-4 rounded-lg border p-4">
      <h2 className="font-medium">Java &amp; memory</h2>
      <div className="space-y-2">
        <Label htmlFor="ram">Max memory (MB)</Label>
        <Input
          id="ram"
          inputMode="numeric"
          placeholder="JVM default"
          value={ram}
          onChange={(e) => setRam(e.target.value)}
          aria-invalid={ramInvalid}
          className="w-48"
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="java">Java executable</Label>
        <div className="flex gap-2">
          <Input
            id="java"
            placeholder="Use global setting / auto-detect"
            value={java}
            onChange={(e) => setJava(e.target.value)}
            className="font-mono text-xs"
          />
          <Button variant="outline" size="icon" title="Browse" onClick={() => void browseJava()}>
            <FileSearch />
          </Button>
        </div>
      </div>
      <div className="space-y-2">
        <Label htmlFor="jvm-args">Extra JVM arguments (one per line)</Label>
        <Textarea
          id="jvm-args"
          className="font-mono text-xs"
          rows={4}
          value={jvmArgs}
          onChange={(e) => setJvmArgs(e.target.value)}
        />
      </div>
      <Button disabled={ramInvalid || save.isPending} onClick={() => save.mutate()}>
        {save.isPending && <Loader2 className="animate-spin" />}
        Save
      </Button>
    </div>
  );
}
