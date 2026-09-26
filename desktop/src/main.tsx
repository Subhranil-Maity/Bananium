import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createHashRouter, RouterProvider } from "react-router";

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { AppShell } from "@/components/app-shell";
import { AccountsPage } from "@/routes/accounts";
import { BrowsePage } from "@/routes/browse";
import { InstancePage } from "@/routes/instance";
import { LibraryPage } from "@/routes/library";
import { PresetsPage } from "@/routes/presets";
import { ScreenshotsPage } from "@/routes/screenshots";
import { SettingsPage } from "@/routes/settings";
import { AboutPage } from "@/routes/about";
import { ConsolePage } from "@/routes/console";
import { RouteError } from "@/components/route-error";
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { describeError, logToBackend, onEvent } from "@/lib/api";
import { PRESENCE_STATUS_KEY } from "@/lib/presence";
import { useTasks } from "@/stores/tasks";

const queryClient = new QueryClient({
  defaultOptions: {
    // Everything is local IPC; there's no network flakiness to retry past,
    // and a failed command should surface immediately.
    queries: { retry: false, refetchOnWindowFocus: false },
  },
});

// Uncaught webview errors go into the launcher log, next to the backend's.
window.addEventListener("error", (e) => {
  logToBackend("error", `uncaught error: ${describeError(e.error ?? e.message)} (${e.filename}:${e.lineno})`);
});
window.addEventListener("unhandledrejection", (e) => {
  logToBackend("error", `unhandled promise rejection: ${describeError(e.reason)}`);
});

// One global subscription: task events feed the tray; a game exiting
// refreshes the instance list so Play/Stop flips immediately.
void onEvent((event) => {
  if (event.event === "instance_exited" || event.event === "instance_launched") {
    void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
    return;
  }
  if (event.event === "presence_status_changed") {
    queryClient.setQueryData(PRESENCE_STATUS_KEY, event.status);
    return;
  }
  useTasks.getState().apply(event);
  // Installs and launches may have just downloaded a Mojang Java runtime.
  if (event.event === "task_completed") {
    void queryClient.invalidateQueries({ queryKey: ["java-list"] });
    void queryClient.invalidateQueries({ queryKey: ["instance-java"] });
  }
});

// Hash routing: the production build is served from Tauri's custom
// protocol, where there's no server to rewrite deep links to index.html.
const router = createHashRouter([
  {
    element: <AppShell />,
    errorElement: <RouteError />,
    children: [
      { index: true, element: <LibraryPage /> },
      { path: "instance/:slug", element: <InstancePage /> },
      { path: "browse", element: <BrowsePage /> },
      { path: "presets", element: <PresetsPage /> },
      { path: "screenshots", element: <ScreenshotsPage /> },
      { path: "accounts", element: <AccountsPage /> },
      { path: "about", element: <AboutPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "console", element: <ConsolePage /> },
    ],
  },
]);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
