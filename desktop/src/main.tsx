import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createHashRouter, RouterProvider } from "react-router";

import "./index.css";
import { AppShell } from "@/components/app-shell";
import { AccountsPage } from "@/routes/accounts";
import { BrowsePage } from "@/routes/browse";
import { InstancePage } from "@/routes/instance";
import { LibraryPage } from "@/routes/library";
import { PresetsPage } from "@/routes/presets";
import { ScreenshotsPage } from "@/routes/screenshots";
import { SettingsPage } from "@/routes/settings";
import { INSTANCES_KEY } from "@/hooks/use-instances";
import { onEvent } from "@/lib/api";
import { useTasks } from "@/stores/tasks";

const queryClient = new QueryClient({
  defaultOptions: {
    // Everything is local IPC; there's no network flakiness to retry past,
    // and a failed command should surface immediately.
    queries: { retry: false, refetchOnWindowFocus: false },
  },
});

// One global subscription: task events feed the tray; a game exiting
// refreshes the instance list so Play/Stop flips immediately.
void onEvent((event) => {
  if (event.event === "instance_exited") {
    void queryClient.invalidateQueries({ queryKey: INSTANCES_KEY });
    return;
  }
  useTasks.getState().apply(event);
});

// Hash routing: the production build is served from Tauri's custom
// protocol, where there's no server to rewrite deep links to index.html.
const router = createHashRouter([
  {
    element: <AppShell />,
    children: [
      { index: true, element: <LibraryPage /> },
      { path: "instance/:slug", element: <InstancePage /> },
      { path: "browse", element: <BrowsePage /> },
      { path: "presets", element: <PresetsPage /> },
      { path: "screenshots", element: <ScreenshotsPage /> },
      { path: "accounts", element: <AccountsPage /> },
      { path: "settings", element: <SettingsPage /> },
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
