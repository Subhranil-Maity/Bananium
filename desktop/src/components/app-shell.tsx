import { NavLink, Outlet } from "react-router";
import { Compass, Image, Layers, Library, Settings, Users } from "lucide-react";

import { AccountSwitcher } from "@/components/account-switcher";

import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { TaskTray } from "@/components/task-tray";
import { cn } from "@/lib/utils";
import { useTheme } from "@/stores/theme";

const NAV = [
  { to: "/", label: "Library", icon: Library, end: true },
  { to: "/browse", label: "Browse", icon: Compass, end: false },
  { to: "/presets", label: "Presets", icon: Layers, end: false },
  { to: "/screenshots", label: "Screenshots", icon: Image, end: false },
  { to: "/accounts", label: "Accounts", icon: Users, end: false },
  { to: "/settings", label: "Settings", icon: Settings, end: false },
];

/** Sidebar + top bar frame every screen renders inside. */
export function AppShell() {
  const theme = useTheme((s) => s.theme);
  return (
    <TooltipProvider>
      <div className="flex h-screen overflow-hidden">
        <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
          <div className="flex h-14 items-center gap-2 px-4 font-semibold">
            <span className="text-xl">🍌</span> Bananium
          </div>
          <nav className="flex flex-col gap-1 px-2">
            {NAV.map(({ to, label, icon: Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors hover:bg-sidebar-accent",
                    isActive && "bg-sidebar-accent font-medium",
                  )
                }
              >
                <Icon className="size-4" />
                {label}
              </NavLink>
            ))}
          </nav>
        </aside>
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-14 shrink-0 items-center justify-end gap-2 border-b px-4">
            <TaskTray />
            <AccountSwitcher />
          </header>
          <main className="min-h-0 flex-1 overflow-y-auto p-6">
            <Outlet />
          </main>
        </div>
      </div>
      <Toaster theme={theme === "dark" ? "dark" : "light"} richColors />
    </TooltipProvider>
  );
}
