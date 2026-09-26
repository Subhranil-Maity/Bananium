import { useEffect } from "react";
import { Link, useRouteError } from "react-router";

import { Button } from "@/components/ui/button";
import { describeError, logToBackend } from "@/lib/api";

/**
 * Shown when rendering a screen throws. The error is written to the
 * launcher log so a UI crash can be debugged from the log file alone.
 */
export function RouteError() {
  const error = useRouteError();
  const text = describeError(error);
  useEffect(() => {
    logToBackend("error", `screen crashed: ${text}`);
  }, [text]);

  return (
    <div className="mx-auto flex h-screen max-w-xl flex-col justify-center gap-3 p-8">
      <h1 className="text-base font-semibold">Something went wrong on this screen</h1>
      <p className="text-sm text-muted-foreground">
        The error was saved to the launcher log. If it keeps happening, please share that log file with the
        developer.
      </p>
      <pre className="max-h-60 overflow-auto rounded-md border bg-muted/50 p-3 font-mono text-xs whitespace-pre-wrap select-text">
        {text}
      </pre>
      <div className="flex gap-2">
        <Button asChild>
          <Link to="/">Back to Library</Link>
        </Button>
        <Button variant="outline" asChild>
          <Link to="/console">Open console</Link>
        </Button>
      </div>
    </div>
  );
}
