// The webview's only way into Bananium: every action is a serialized
// `Command` sent through the single `dispatch` Tauri command, and every
// progress/status update arrives as an `Event` on `EVENT_CHANNEL`. The
// types in `@/bindings` are generated from the Rust definitions in
// `crates/bananium-api` by ts-rs — never hand-edit them.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { Command } from "@/bindings/Command";
import type { CommandOutput } from "@/bindings/CommandOutput";
import type { Event } from "@/bindings/Event";

/** Mirrors `EVENT_CHANNEL` in `desktop/src-tauri/src/main.rs`. */
export const EVENT_CHANNEL = "bananium://event";

type ResultTag = CommandOutput["result"];

/** The `CommandOutput` variant whose `result` tag is `R`. */
export type OutputOf<R extends ResultTag> = Extract<CommandOutput, { result: R }>;

/** Run one command; rejects with the backend's error message as a string. */
export function dispatch(cmd: Command): Promise<CommandOutput> {
  return invoke<CommandOutput>("dispatch", { cmd });
}

/**
 * Run `cmd` and narrow its output to the variant tagged `result`. Every
 * `Command` has exactly one success variant, so a mismatch means the
 * bindings are stale relative to the Rust side.
 */
export async function run<R extends ResultTag>(cmd: Command, result: R): Promise<OutputOf<R>> {
  const out = await dispatch(cmd);
  if (out.result !== result) {
    throw new Error(`expected '${result}' from '${cmd.command}', got '${out.result}'`);
  }
  return out as OutputOf<R>;
}

/** Subscribe to every backend `Event`; resolves to an unsubscribe function. */
export function onEvent(handler: (event: Event) => void): Promise<UnlistenFn> {
  return listen<Event>(EVENT_CHANNEL, (e) => handler(e.payload));
}

/** Error thrown by `dispatch` (a plain string from Rust) as a displayable message. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
