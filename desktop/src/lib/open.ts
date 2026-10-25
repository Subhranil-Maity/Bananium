import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { errorMessage } from "@/lib/api";

/** Open a file or folder in the system's default app, toasting instead of failing silently. */
export function openPathSafe(path: string) {
  openPath(path).catch((err) => toast.error("Couldn't open", { description: errorMessage(err) }));
}
