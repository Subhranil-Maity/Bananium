import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDown,
  ChevronDown,
  ChevronUp,
  Copy,
  ExternalLink,
  FolderOpen,
  Search,
  TextWrap,
  X,
} from "lucide-react";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import type { LogFile } from "@/bindings/LogFile";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { errorMessage, run } from "@/lib/api";
import { cn, formatBytes, formatRelative } from "@/lib/utils";

/** Cap on text kept in memory; older lines fall off the top. */
const MAX_CHARS = 400_000;
const LATEST = "__latest__";
/** Fixed row height (px) for unwrapped lines; wrapped rows are measured. */
const ROW = 20;

export type Level = "debug" | "info" | "warn" | "error";

export interface Line {
  /** 1-based line number in the loaded text. */
  n: number;
  raw: string;
  level: Level;
  /** Parsed header parts; absent for continuation/raw lines. */
  time?: string;
  thread?: string;
  tag?: string;
  logger?: string;
  message: string;
  /** Continuation of the previous entry (stack frames, wrapped output). */
  cont: boolean;
  chat: boolean;
  /** A fatal error / crash line, highlighted beyond a plain error. */
  fatal?: boolean;
}

/** Text loaded so far from one log, as produced by a tail hook. */
export interface LogTail {
  text: string;
  /** Name of the file being shown; `null` when there is none. */
  current: string | null;
  loaded: boolean;
  error: string | null;
  /** Loads the chunk before what's shown; absent when nothing is older. */
  loadOlder?: () => void;
  loadingOlder?: boolean;
  /** Bumped each time `loadOlder` prepends text; `lines` is how many. */
  prepended?: { seq: number; lines: number };
}

// Vanilla: `[12:34:56] [Render thread/INFO]: msg`
// Fabric:  `[12:34:56] [main/INFO] (FabricLoader) msg`
const HEADER =
  /^\[(\d{2}:\d{2}:\d{2}(?:\.\d+)?)\] \[([^\]]*?)\/(TRACE|DEBUG|INFO|WARN|WARNING|ERROR|FATAL|SEVERE)\]:?\s?(?:\(([^)]{1,60})\)\s)?(.*)$/;

function levelOf(tag: string): Level {
  if (tag === "WARN" || tag === "WARNING") return "warn";
  if (tag === "ERROR" || tag === "FATAL" || tag === "SEVERE") return "error";
  return "info";
}

/** Parses a Minecraft (log4j) game log. */
function parseGameLog(text: string): Line[] {
  const lines = text.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  let prev: Level = "info";
  return lines.map((raw, i) => {
    const m = HEADER.exec(raw);
    if (m) {
      const level = levelOf(m[3]);
      prev = level;
      const message = m[5];
      return {
        n: i + 1,
        raw,
        level,
        time: m[1],
        thread: m[2],
        tag: m[3],
        logger: m[4],
        message,
        cont: false,
        chat: message.includes("[CHAT]"),
        fatal: m[3] === "FATAL",
      };
    }
    // Anything else continues the previous entry: stack traces inherit its
    // level so filtering to errors keeps the whole trace together.
    const trace = /^\s+at |^Caused by:|^\s*\.\.\. \d+ more|^[\w.$]+(Exception|Error)\b/.test(raw);
    const level: Level = trace && prev === "info" ? "error" : prev;
    return { n: i + 1, raw, level, message: raw, cont: true, chat: false };
  });
}

const LEVEL_TAG: Record<string, string> = {
  debug: "text-sky-300/50",
  info: "text-white/40",
  warn: "text-amber-300",
  error: "text-red-400",
};
const LEVEL_TEXT: Record<Level, string> = {
  debug: "text-console-foreground/55",
  info: "text-console-foreground",
  warn: "text-amber-100/95",
  error: "text-red-200",
};
const LEVEL_ROW: Record<Level, string> = {
  debug: "",
  info: "",
  warn: "bg-amber-400/[0.06] shadow-[inset_2px_0_0_0_rgb(252_211_77/0.7)]",
  error: "bg-red-500/[0.09] shadow-[inset_2px_0_0_0_rgb(248_113_113/0.85)]",
};

/** `text` with every case-insensitive occurrence of `q` highlighted. */
function highlight(text: string, q: string, strong: boolean): ReactNode {
  if (!q) return text;
  const lower = text.toLowerCase();
  const parts: ReactNode[] = [];
  let from = 0;
  for (let at = lower.indexOf(q); at !== -1; at = lower.indexOf(q, at + q.length)) {
    parts.push(text.slice(from, at));
    parts.push(
      <mark
        key={at}
        className={cn("rounded-[2px] text-black", strong ? "bg-primary" : "bg-primary/55")}
      >
        {text.slice(at, at + q.length)}
      </mark>,
    );
    from = at + q.length;
  }
  parts.push(text.slice(from));
  return parts;
}

const LogRow = memo(function LogRow({
  line,
  query,
  current,
  wrap,
}: {
  line: Line;
  query: string;
  current: boolean;
  wrap: boolean;
}) {
  return (
    <div
      className={cn(
        "flex min-h-5 px-0 leading-5 hover:bg-white/[0.035]",
        LEVEL_ROW[line.level],
        line.fatal && "bg-red-500/20",
        current && "bg-primary/15 hover:bg-primary/15",
      )}
    >
      <span className="w-14 shrink-0 pr-3 text-right text-white/20 tabular-nums select-none">{line.n}</span>
      <span className={cn("min-w-0 flex-1 pr-4", wrap ? "break-all whitespace-pre-wrap" : "whitespace-pre")}>
        {line.cont ? (
          <span className={cn(LEVEL_TEXT[line.level], line.level === "info" && "text-console-foreground/70")}>
            {highlight(line.raw, query, current)}
          </span>
        ) : (
          <>
            <span className="text-white/30">{line.time} </span>
            {line.thread !== undefined ? (
              <>
                <span className="text-sky-300/55">{line.thread}</span>
                <span className="text-white/25">/</span>
                <span className={cn("font-bold", LEVEL_TAG[line.level])}>{line.tag}</span>
                <span className="text-white/25"> </span>
                {line.logger && <span className="text-violet-300/70">({line.logger}) </span>}
              </>
            ) : (
              <>
                <span className={cn("font-bold", LEVEL_TAG[line.level])}>[{line.tag}]</span>{" "}
                {line.logger && <span className="text-violet-300/70">[{line.logger}] </span>}
              </>
            )}
            <span className={cn(LEVEL_TEXT[line.level], line.chat && "text-emerald-300")}>
              {highlight(line.message, query, current)}
            </span>
          </>
        )}
      </span>
    </div>
  );
});

/**
 * Polls `log_read` from the offset the previous read returned. The caller
 * remounts (via `key`) whenever the selected log changes, so it always
 * starts clean. `file === null` follows whichever log is newest, so a fresh
 * launch is picked up without reselecting.
 */
function useLogTail(slug: string, file: string | null, follow: boolean): LogTail {
  const [text, setText] = useState("");
  const [current, setCurrent] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cursor = useRef<{ file: string | null; offset: number }>({ file: null, offset: 0 });

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const out = await run(
          { command: "log_read", instance: slug, file, offset: cursor.current.offset },
          "log_chunk",
        );
        if (cancelled) return;
        setLoaded(true);
        setError(null);
        if (out.file !== cursor.current.file) {
          // First read, or "latest" moved to a newer launch: start over.
          cursor.current = { file: out.file, offset: out.next_offset };
          setCurrent(out.file);
          setText(out.text.slice(-MAX_CHARS));
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

  return { text, current, loaded, error };
}

function ToolButton({
  label,
  onClick,
  active,
  children,
  disabled,
}: {
  label: string;
  onClick: () => void;
  active?: boolean;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          aria-pressed={active}
          disabled={disabled}
          onClick={onClick}
          className={cn(
            "flex size-7 items-center justify-center rounded-md text-white/55 transition-colors hover:bg-white/10 hover:text-white disabled:opacity-40 [&_svg]:size-3.5",
            active && "bg-white/10 text-white",
          )}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

const LEVEL_FILTERS: { level: Level; label: string; on: string }[] = [
  { level: "debug", label: "Debug", on: "border-sky-300/30 bg-sky-300/10 text-sky-200" },
  { level: "info", label: "Info", on: "border-white/20 bg-white/10 text-white" },
  { level: "warn", label: "Warn", on: "border-amber-300/40 bg-amber-300/15 text-amber-200" },
  { level: "error", label: "Error", on: "border-red-400/50 bg-red-500/20 text-red-200" },
];

/**
 * A searchable, level-filtered, virtualized console over `tail`'s text.
 * The data source is the caller's: an instance's game log or the
 * launcher's own log, each with its own parser.
 */
export function LogConsole({
  tail,
  parse,
  live,
  following,
  picker,
  path,
  empty,
  banner,
  levels,
  setLevels,
  wrap,
  setWrap,
}: {
  tail: LogTail;
  parse: (text: string) => Line[];
  live: boolean;
  /** Whether new lines keep arriving (changes the jump button's label). */
  following: boolean;
  picker: ReactNode;
  /** The shown file on disk, for "open" / "show in folder". */
  path: string | undefined;
  /** Shown when there is no file at all. */
  empty: ReactNode;
  /** Optional strip above the lines (e.g. how an old run ended). */
  banner?: ReactNode;
  levels: Set<Level>;
  setLevels: (l: Set<Level>) => void;
  wrap: boolean;
  setWrap: (w: boolean) => void;
}) {
  const { text, current, loaded, error } = tail;
  const lines = useMemo(() => parse(text), [parse, text]);
  const counts = useMemo(() => {
    const c = { debug: 0, info: 0, warn: 0, error: 0 };
    for (const l of lines) if (!l.cont) c[l.level]++;
    return c;
  }, [lines]);
  const visible = useMemo(() => lines.filter((l) => levels.has(l.level)), [lines, levels]);

  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const matches = useMemo(
    () => (q ? visible.flatMap((l, i) => (l.raw.toLowerCase().includes(q) ? [i] : [])) : []),
    [visible, q],
  );
  const [matchAt, setMatchAt] = useState(0);
  const currentRow = matches.length ? matches[Math.min(matchAt, matches.length - 1)] : -1;

  const scroller = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  // The React Compiler skips memoizing this component because of the
  // virtualizer's mutable API; that's expected and harmless here.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 30,
  });

  // After older text is prepended, keep the lines that were on screen in
  // place instead of jumping to the new top.
  const prepended = tail.prepended;
  useEffect(() => {
    if (!prepended?.seq) return;
    const at = visible.findIndex((l) => l.n > prepended.lines);
    if (at > 0) virtualizer.scrollToIndex(at, { align: "start" });
    // Only when a new chunk lands, not on every filter change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prepended?.seq]);

  // Follow the tail while pinned to the bottom; scrolling up unpins.
  useEffect(() => {
    if (atBottom && visible.length > 0 && !q) virtualizer.scrollToIndex(visible.length - 1, { align: "end" });
  }, [visible.length, atBottom, q, virtualizer, wrap]);

  function jump(delta: number) {
    if (!matches.length) return;
    const next = (Math.min(matchAt, matches.length - 1) + delta + matches.length) % matches.length;
    setMatchAt(next);
    setAtBottom(false);
    virtualizer.scrollToIndex(matches[next], { align: "center" });
  }

  function toBottom() {
    setAtBottom(true);
    virtualizer.scrollToIndex(visible.length - 1, { align: "end" });
  }

  // Ctrl+F focuses search while the console is on screen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggleLevel = (level: Level) => {
    const next = new Set(levels);
    if (next.has(level)) next.delete(level);
    else next.add(level);
    setLevels(next.size ? next : new Set([level]));
  };

  return (
    <div className="dark flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-white/10 bg-console font-mono text-[12.5px] font-medium text-console-foreground">
      <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-white/[0.07] bg-white/[0.02] px-2 py-1.5 font-sans text-xs">
        {picker}
        <span
          className={cn(
            "flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold tracking-wide uppercase",
            live ? "text-success" : "text-white/35",
          )}
        >
          <span className={cn("size-1.5 rounded-full", live ? "animate-pulse bg-success" : "bg-white/25")} />
          {live ? "Live" : "Stopped"}
        </span>

        <div className="mx-1 h-4 w-px bg-white/10" />
        {LEVEL_FILTERS.filter((f) => f.level !== "debug" || counts.debug > 0).map(({ level, label, on }) => (
          <button
            key={level}
            onClick={() => toggleLevel(level)}
            className={cn(
              "flex h-6 items-center gap-1.5 rounded-md border border-transparent px-2 text-[11px] font-semibold text-white/40 transition-colors hover:text-white/80",
              levels.has(level) && on,
            )}
          >
            {label}
            <span className="font-mono font-normal tabular-nums opacity-80">{counts[level]}</span>
          </button>
        ))}

        <div className="flex-1" />

        <div className="flex h-7 w-64 items-center gap-1 rounded-md border border-white/10 bg-black/25 pr-1 pl-2 focus-within:border-primary/60">
          <Search className="size-3.5 shrink-0 text-white/35" />
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setMatchAt(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                jump(e.shiftKey ? -1 : 1);
              } else if (e.key === "Escape") {
                setQuery("");
              }
            }}
            placeholder="Find in log (Ctrl+F)"
            className="min-w-0 flex-1 bg-transparent text-xs text-white outline-none placeholder:text-white/30"
          />
          {q && (
            <>
              <span className="shrink-0 font-mono text-[11px] text-white/45 tabular-nums">
                {matches.length ? `${Math.min(matchAt, matches.length - 1) + 1}/${matches.length}` : "0/0"}
              </span>
              <button className="rounded p-0.5 text-white/50 hover:text-white" onClick={() => jump(-1)} aria-label="Previous match">
                <ChevronUp className="size-3.5" />
              </button>
              <button className="rounded p-0.5 text-white/50 hover:text-white" onClick={() => jump(1)} aria-label="Next match">
                <ChevronDown className="size-3.5" />
              </button>
              <button className="rounded p-0.5 text-white/50 hover:text-white" onClick={() => setQuery("")} aria-label="Clear search">
                <X className="size-3.5" />
              </button>
            </>
          )}
        </div>

        <ToolButton label={wrap ? "Don't wrap lines" : "Wrap lines"} active={wrap} onClick={() => setWrap(!wrap)}>
          <TextWrap />
        </ToolButton>
        <ToolButton
          label="Copy visible lines"
          disabled={!visible.length}
          onClick={() =>
            void navigator.clipboard
              .writeText(visible.map((l) => l.raw).join("\n"))
              .then(() => toast.success(`Copied ${visible.length} lines`))
          }
        >
          <Copy />
        </ToolButton>
        <ToolButton label="Open in editor" disabled={!path} onClick={() => path && void openPath(path)}>
          <ExternalLink />
        </ToolButton>
        <ToolButton label="Show in folder" disabled={!path} onClick={() => path && void revealItemInDir(path)}>
          <FolderOpen />
        </ToolButton>
      </div>

      <div className="relative min-h-0 flex-1">
        <div
          ref={scroller}
          className="absolute inset-0 overflow-auto py-1 select-text"
          onScroll={(e) => {
            const el = e.currentTarget;
            const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < ROW * 1.5;
            if (bottom !== atBottom) setAtBottom(bottom);
          }}
        >
          {banner}
          {error && <p className="px-4 py-2 font-sans text-sm text-red-300">{error}</p>}
          {loaded && !current && <p className="px-4 py-6 font-sans text-sm text-white/40">{empty}</p>}
          {tail.loadOlder && (
            <button
              onClick={tail.loadOlder}
              disabled={tail.loadingOlder}
              className="mx-auto my-1 flex items-center gap-1.5 rounded-full border border-white/15 px-3 py-1 font-sans text-xs text-white/70 transition-colors hover:bg-white/10 hover:text-white disabled:opacity-50"
            >
              <ChevronUp className="size-3.5" /> {tail.loadingOlder ? "Loading…" : "Load older lines"}
            </button>
          )}
          <div className={cn("relative", !wrap && "min-w-max")} style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((row) => (
              <div
                key={row.key}
                data-index={row.index}
                ref={wrap ? virtualizer.measureElement : undefined}
                className="absolute left-0 w-full"
                style={{ transform: `translateY(${row.start}px)`, height: wrap ? undefined : ROW }}
              >
                <LogRow line={visible[row.index]} query={q} current={row.index === currentRow} wrap={wrap} />
              </div>
            ))}
          </div>
        </div>

        {!atBottom && visible.length > 0 && (
          <button
            onClick={toBottom}
            className="absolute right-5 bottom-4 flex items-center gap-1.5 rounded-full border border-white/15 bg-neutral-800/95 px-3 py-1.5 font-sans text-xs font-medium text-white shadow-lg transition-colors hover:bg-neutral-700"
          >
            <ArrowDown className="size-3.5" /> {following ? "Jump to latest" : "Jump to end"}
          </button>
        )}
      </div>

      <div className="flex h-6 shrink-0 items-center gap-3 border-t border-white/[0.07] px-3 font-sans text-[11px] text-white/35 tabular-nums">
        <span>{lines.length.toLocaleString()} lines</span>
        {visible.length !== lines.length && <span>{visible.length.toLocaleString()} shown</span>}
        {counts.warn > 0 && <span className="text-amber-300/80">{counts.warn} warnings</span>}
        {counts.error > 0 && <span className="text-red-400/90">{counts.error} errors</span>}
        <span className="ml-auto truncate">{current ?? ""}</span>
      </div>
    </div>
  );
}

/** Launch-log picker plus a live, searchable, level-filtered console. */
export function LogViewer({ slug, live }: { slug: string; live: boolean }) {
  const [selected, setSelected] = useState(LATEST);
  const [levels, setLevels] = useState<Set<Level>>(() => new Set(["info", "warn", "error"]));
  const [wrap, setWrap] = useState(false);
  const { data: logs } = useQuery({
    queryKey: ["logs", slug],
    queryFn: async () => (await run({ command: "log_list", instance: slug }, "log_listed")).logs,
    refetchInterval: live ? 3000 : false,
  });
  const file = selected === LATEST ? null : selected;
  const following = live || file === null;

  const picker = (
    <Select value={selected} onValueChange={setSelected}>
      <SelectTrigger
        size="sm"
        className="h-7 w-60 border-white/10 bg-black/25 text-xs text-white hover:bg-black/40 dark:bg-black/25 dark:hover:bg-black/40"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={LATEST}>Latest launch</SelectItem>
        {logs?.map((l) => (
          <SelectItem key={l.name} value={l.name}>
            <span>{new Date(l.modified_unix * 1000).toLocaleString()}</span>
            <span className="text-muted-foreground">
              {" "}
              · {formatBytes(l.size)} · {formatRelative(l.modified_unix)}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );

  return (
    <GameLogConsole
      key={`${slug}:${selected}`}
      slug={slug}
      file={file}
      live={live}
      following={following}
      picker={picker}
      logs={logs}
      levels={levels}
      setLevels={setLevels}
      wrap={wrap}
      setWrap={setWrap}
    />
  );
}

/** Tails one game log; remounted (via `key`) when the selection changes. */
function GameLogConsole({
  slug,
  file,
  following,
  logs,
  ...rest
}: {
  slug: string;
  file: string | null;
  live: boolean;
  following: boolean;
  picker: ReactNode;
  logs: LogFile[] | undefined;
  levels: Set<Level>;
  setLevels: (l: Set<Level>) => void;
  wrap: boolean;
  setWrap: (w: boolean) => void;
}) {
  const tail = useLogTail(slug, file, following);
  return (
    <LogConsole
      {...rest}
      tail={tail}
      parse={parseGameLog}
      following={following}
      path={logs?.find((l) => l.name === tail.current)?.path}
      empty="No launches yet. Logs appear here as soon as the game starts."
    />
  );
}
