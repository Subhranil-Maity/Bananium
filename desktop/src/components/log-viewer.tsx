import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { errorMessage, run } from "@/lib/api";
import { formatBytes } from "@/lib/utils";

/** Cap on text kept in memory; older lines fall off the top. */
const MAX_CHARS = 400_000;
const LATEST = "__latest__";

/**
 * Polls `log_read` from the offset the previous read returned. Remounted
 * (via `key`) whenever the selected log changes, so it always starts clean.
 * `file === null` follows whichever log is newest, so a fresh launch is
 * picked up without reselecting.
 */
function LogTail({ slug, file, follow }: { slug: string; file: string | null; follow: boolean }) {
  const [text, setText] = useState("");
  const [current, setCurrent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const cursor = useRef<{ file: string | null; offset: number }>({ file: null, offset: 0 });
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const out = await run(
          { command: "log_read", instance: slug, file, offset: cursor.current.offset },
          "log_chunk",
        );
        if (cancelled) return;
        if (out.file !== cursor.current.file) {
          // First read, or "latest" moved to a newer launch: start over.
          cursor.current = { file: out.file, offset: out.next_offset };
          setCurrent(out.file);
          setText(out.text);
        } else {
          cursor.current.offset = out.next_offset;
          if (out.text) setText((t) => (t + out.text).slice(-MAX_CHARS));
        }
      } catch (err) {
        if (!cancelled) setError(errorMessage(err));
      }
    }
    void poll();
    const timer = follow ? setInterval(poll, 1000) : undefined;
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [slug, file, follow]);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [text]);

  return (
    <>
      {error && <p className="text-sm text-destructive">{error}</p>}
      <div className="min-h-0 flex-1 overflow-auto rounded-md border bg-muted p-3">
        <pre className="font-mono text-xs break-all whitespace-pre-wrap select-text">
          {text || (current ? "" : "No launches yet.")}
        </pre>
        <div ref={bottom} />
      </div>
    </>
  );
}

/** Launch-log picker plus a live tail of the chosen log. */
export function LogViewer({ slug, live }: { slug: string; live: boolean }) {
  const [selected, setSelected] = useState(LATEST);
  const { data: logs } = useQuery({
    queryKey: ["logs", slug],
    queryFn: async () => (await run({ command: "log_list", instance: slug }, "log_listed")).logs,
    refetchInterval: live ? 3000 : false,
  });
  const file = selected === LATEST ? null : selected;

  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <Select value={selected} onValueChange={setSelected}>
        <SelectTrigger className="w-80">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={LATEST}>Latest launch</SelectItem>
          {logs?.map((l) => (
            <SelectItem key={l.name} value={l.name}>
              {new Date(l.modified_unix * 1000).toLocaleString()} · {formatBytes(l.size)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <LogTail key={`${slug}:${selected}`} slug={slug} file={file} follow={live || file === null} />
    </div>
  );
}
