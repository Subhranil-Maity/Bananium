import type { ReactNode } from "react";
import { FileSearch, RefreshCw } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useJavaList } from "@/hooks/use-java";

const DEFAULT = "__default__";
const BROWSE = "__browse__";

/**
 * Choose a Java executable. `value === ""` means the default (Mojang's
 * official runtime), which is also what's selected until the user picks
 * something else. Lists downloaded Mojang runtimes and every JVM detected
 * on the machine, plus "Browse…" for anything else.
 */
export function JavaPicker({
  value,
  onChange,
  defaultLabel,
}: {
  value: string;
  onChange: (path: string) => void;
  /** What the default option shows, e.g. "Mojang official · java-runtime-delta (Java 21)". */
  defaultLabel: ReactNode;
}) {
  const javas = useJavaList();
  const mojang = javas.data?.filter((j) => j.source === "mojang") ?? [];
  const system = javas.data?.filter((j) => j.source === "system") ?? [];
  const known = javas.data?.some((j) => j.path === value) ?? false;

  async function browse() {
    const picked = await open({ multiple: false, directory: false, title: "Choose a Java executable" });
    if (typeof picked === "string") onChange(picked);
  }

  return (
    <div className="flex gap-2">
      <Select
        value={value === "" ? DEFAULT : value}
        onValueChange={(v) => {
          if (v === BROWSE) void browse();
          else onChange(v === DEFAULT ? "" : v);
        }}
      >
        <SelectTrigger className="min-w-0 flex-1">
          <SelectValue />
        </SelectTrigger>
        <SelectContent className="max-h-96">
          <SelectItem value={DEFAULT}>{defaultLabel}</SelectItem>

          {mojang.length > 0 && (
            <SelectGroup>
              <SelectSeparator />
              <SelectLabel className="text-[11px] tracking-wide uppercase">Mojang runtimes (downloaded)</SelectLabel>
              {mojang.map((j) => (
                <SelectItem key={j.path} value={j.path}>
                  <span className="font-medium">Java {j.major_version}</span>
                  <span className="text-muted-foreground">
                    {j.component} · {j.version}
                  </span>
                </SelectItem>
              ))}
            </SelectGroup>
          )}

          <SelectGroup>
            <SelectSeparator />
            <SelectLabel className="text-[11px] tracking-wide uppercase">
              {javas.isLoading ? "Detecting Java…" : "Detected on this computer"}
            </SelectLabel>
            {system.map((j) => (
              <SelectItem key={j.path} value={j.path}>
                <span className="font-medium">Java {j.major_version}</span>
                <span className="max-w-96 truncate font-mono text-[11px] text-muted-foreground">{j.path}</span>
              </SelectItem>
            ))}
            {!javas.isLoading && system.length === 0 && (
              <div className="px-2 py-1.5 text-xs text-muted-foreground">No other Java installations found.</div>
            )}
          </SelectGroup>

          {value !== "" && !known && (
            <SelectGroup>
              <SelectSeparator />
              <SelectLabel className="text-[11px] tracking-wide uppercase">Custom</SelectLabel>
              <SelectItem value={value}>
                <span className="max-w-96 truncate font-mono text-[11px]">{value}</span>
              </SelectItem>
            </SelectGroup>
          )}

          <SelectSeparator />
          <SelectItem value={BROWSE}>
            <FileSearch /> Browse for java executable…
          </SelectItem>
        </SelectContent>
      </Select>
      <Button
        variant="outline"
        size="icon"
        title="Scan for Java again"
        disabled={javas.isFetching}
        onClick={() => void javas.refetch()}
      >
        <RefreshCw className={javas.isFetching ? "animate-spin" : ""} />
      </Button>
    </div>
  );
}
