import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useQuery } from "@tanstack/react-query";

import type { LauncherLogFile } from "@/bindings/LauncherLogFile";
import type { LogStatus } from "@/bindings/LogStatus";
import { LogConsole, type Level, type Line, type LogTail } from "@/components/log-viewer";
import { Page, PageHeader } from "@/components/page";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { errorMessage, run } from "@/lib/api";
import { usePresenceView } from "@/lib/presence";
import { cn, formatBytes } from "@/lib/utils";

const CURRENT = "__current__";
/** Cap on text kept in memory. Following trims the oldest lines; paging back stops here. */
const MAX_CHARS = 2_000_000;

export const LAUNCHER_LOGS_KEY = ["launcher-logs"] as const;

// `2026-09-26 14:30:05.123 [INFO] [bananium_api::session] message`
const HEADER = /^\d{4}-\d{2}-\d{2} (\d{2}:\d{2}:\d{2}\.\d{3}) \[(TRACE|DEBUG|INFO|WARN|ERROR)\] \[([^\]]+)\] (.*)$/;

function levelOf(tag: string): Level {
  if (tag === "TRACE" || tag === "DEBUG") return "debug";
  if (tag === "WARN") return "warn";
  if (tag === "ERROR") return "error";
  return "info";
}

/** Parses the launcher's own log format (see `bananium_core::logging`). */
function parseLauncherLog(text: string): Line[] {
  const lines = text.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  let prev: Level = "info";
  let prevFatal = false;
  return lines.map((raw, i) => {
    const m = HEADER.exec(raw);
    if (m) {
      prev = levelOf(m[2]);
      prevFatal = m[3] === "bananium::panic";
      return {
        n: i + 1,
        raw,
        level: prev,
        time: m[1],
        tag: m[2],
        logger: m[3],
        message: m[4],
        cont: false,
        chat: false,
        fatal: prevFatal,
      };
    }
    // Backtrace frames and multi-line messages belong to the entry above.
    return { n: i + 1, raw, level: prev, message: raw, cont: true, chat: false, fatal: prevFatal };
  });
}

const utf8 = new TextEncoder();

/**
 * Reads the launcher log in bounded chunks: the tail first, then new lines
 * every second while `follow` is on, and older chunks on request. The whole
 * file never crosses IPC at once. `file === null` is this run's log.
 */
function useLauncherLogTail(file: string | null, follow: boolean): LogTail {
  const [text, setText] = useState("");
  const [current, setCurrent] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [start, setStart] = useState(0);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [prepended, setPrepended] = useState({ seq: 0, lines: 0 });
  // Byte offsets of what's loaded; `end === null` until the first read.
  const range = useRef<{ start: number; end: number | null }>({ start: 0, end: null });
  const textRef = useRef("");

  const show = useCallback((next: string, nextStart: number) => {
    // Over the cap: drop whole lines off the top and move `start` past
    // their bytes, so paging back later resumes from the right place.
    if (next.length > MAX_CHARS) {
      const cut = next.indexOf("\n", next.length - MAX_CHARS);
      const dropped = cut === -1 ? next : next.slice(0, cut + 1);
      nextStart += utf8.encode(dropped).length;
      next = next.slice(dropped.length);
    }
    textRef.current = next;
    range.current.start = nextStart;
    setText(next);
    setStart(nextStart);
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const end = range.current.end;
        const out = await run(
          { command: "launcher_log_read", file, offset: end ?? 0, before: null },
          "launcher_log_chunk",
        );
        if (cancelled) return;
        setLoaded(true);
        setError(null);
        setCurrent(out.file);
        if (end !== null && out.start === end) {
          if (out.text) show(textRef.current + out.text, range.current.start);
        } else {
          // First read, or the file shrank and was read afresh.
          show(out.text, out.start);
        }
        range.current.end = out.end;
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
  }, [file, follow, show]);

  const loadOlder = useCallback(async () => {
    setLoadingOlder(true);
    try {
      const out = await run(
        { command: "launcher_log_read", file: current, offset: 0, before: range.current.start },
        "launcher_log_chunk",
      );
      const lines = out.text.split("\n").length - 1;
      textRef.current = out.text + textRef.current;
      range.current.start = out.start;
      setText(textRef.current);
      setStart(out.start);
      setPrepended((p) => ({ seq: p.seq + 1, lines }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoadingOlder(false);
    }
  }, [current]);

  const canLoadOlder = loaded && start > 0 && text.length < MAX_CHARS;
  return {
    text,
    current,
    loaded,
    error,
    loadOlder: canLoadOlder ? () => void loadOlder() : undefined,
    loadingOlder,
    prepended,
  };
}

const STATUS_LABEL: Record<LogStatus, string> = {
  running: "Running",
  closed: "Closed",
  crashed: "Crashed",
  unclean: "Didn't close properly",
};

const STATUS_CLASS: Record<LogStatus, string> = {
  running: "text-success",
  closed: "text-muted-foreground",
  crashed: "text-destructive",
  unclean: "text-amber-500",
};

function logDate(log: LauncherLogFile) {
  return new Date(log.modified_unix * 1000).toLocaleString();
}

function LauncherConsole({
  file,
  log,
  picker,
  levels,
  setLevels,
  wrap,
  setWrap,
}: {
  file: string | null;
  log: LauncherLogFile | undefined;
  picker: React.ReactNode;
  levels: Set<Level>;
  setLevels: (l: Set<Level>) => void;
  wrap: boolean;
  setWrap: (w: boolean) => void;
}) {
  const live = file === null;
  const tail = useLauncherLogTail(file, live);
  const bad = log && (log.status === "crashed" || log.status === "unclean");
  const banner = bad ? (
    <div className="mx-2 mb-1 rounded-md border border-red-400/30 bg-red-500/10 px-3 py-2 font-sans text-xs text-red-200">
      <span className="font-semibold">{STATUS_LABEL[log.status]}:</span> {log.reason}
    </div>
  ) : undefined;
  return (
    <LogConsole
      tail={tail}
      parse={parseLauncherLog}
      live={live}
      following={live}
      picker={picker}
      path={log?.path}
      empty="This run has no log file (the logs folder couldn't be written)."
      banner={banner}
      levels={levels}
      setLevels={setLevels}
      wrap={wrap}
      setWrap={setWrap}
    />
  );
}

/** The launcher's own log: this run live, or any earlier run's. */
export function ConsolePage() {
  usePresenceView({ view: "settings" });
  const [params, setParams] = useSearchParams();
  const selected = params.get("file") ?? CURRENT;
  const [levels, setLevels] = useState<Set<Level>>(() => new Set(["debug", "info", "warn", "error"]));
  const [wrap, setWrap] = useState(false);
  const { data, refetch } = useQuery({
    queryKey: LAUNCHER_LOGS_KEY,
    queryFn: () => run({ command: "launcher_log_list" }, "launcher_log_listed"),
  });
  const currentName = data?.current ?? null;
  const file = selected === CURRENT || selected === currentName ? null : selected;
  const log = data?.logs.find((l) => l.name === (file ?? currentName));
  const older = data?.logs.filter((l) => l.name !== currentName) ?? [];

  const picker = (
    <Select
      value={file ?? CURRENT}
      onValueChange={(v) => setParams(v === CURRENT ? {} : { file: v }, { replace: true })}
      onOpenChange={(open) => open && void refetch()}
    >
      <SelectTrigger
        size="sm"
        className="h-7 w-72 border-white/10 bg-black/25 text-xs text-white hover:bg-black/40 dark:bg-black/25 dark:hover:bg-black/40"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={CURRENT}>Current session</SelectItem>
        {older.map((l) => (
          <SelectItem key={l.name} value={l.name}>
            <span>{logDate(l)}</span>
            <span className="text-muted-foreground"> · {formatBytes(l.size)} · </span>
            <span className={cn(STATUS_CLASS[l.status])}>{STATUS_LABEL[l.status]}</span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );

  return (
    <Page className="flex h-full flex-col">
      <PageHeader title="Console" meta="Bananium's own log — attach it when reporting a bug" />
      <div className="min-h-0 flex-1">
        <LauncherConsole
          key={selected}
          file={file}
          log={log}
          picker={picker}
          levels={levels}
          setLevels={setLevels}
          wrap={wrap}
          setWrap={setWrap}
        />
      </div>
    </Page>
  );
}
