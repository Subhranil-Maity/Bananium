import { useEffect, type ReactNode } from "react";
import { Link, Outlet, useLocation, useNavigate } from "react-router";
import { Plus, Search } from "lucide-react";

import { AccountSwitcher } from "@/components/account-switcher";
import { BrandMark } from "@/components/brand-mark";
import { CommandPalette, usePalette } from "@/components/command-palette";
import { GlobalContextMenu } from "@/components/global-context-menu";
import { InstanceContextMenuContent, InstanceDialogsHost } from "@/components/instance-actions";
import { InstanceIcon } from "@/components/instance-icon";
import { NAV } from "@/components/nav";
import { NewInstanceDialog, useNewInstance } from "@/components/new-instance-dialog";
import { TaskTray } from "@/components/task-tray";
import { ContextMenu, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Kbd } from "@/components/ui/kbd";
import { Toaster } from "@/components/ui/sonner";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { useInstances } from "@/hooks/use-instances";
import { sortInstances } from "@/lib/instances";
import { cn } from "@/lib/utils";
import { useTheme } from "@/stores/theme";

/** How many recently played instances get a quick-launch slot in the rail. */
const QUICK_LAUNCH = 5;

function RailTip({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="right" sideOffset={8}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

/** Rail button look: the active page gets a yellow edge marker. */
function railClass(active: boolean) {
  return cn(
    "relative flex size-11 shrink-0 items-center justify-center rounded-xl text-sidebar-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
    "before:absolute before:top-2.5 before:bottom-2.5 before:-left-2.5 before:w-[3px] before:rounded-r before:bg-primary before:opacity-0 before:transition-opacity",
    active && "bg-sidebar-accent text-sidebar-accent-foreground before:opacity-100",
  );
}

/**
 * Whether `to` is the current page. Computed here rather than with
 * `NavLink`'s function-valued `className`: the tooltip trigger's Radix
 * `Slot` merges class names as strings and would stringify the function,
 * silently dropping every rail style.
 */
function isActive(pathname: string, to: string, end: boolean) {
  return end ? pathname === to : pathname === to || pathname.startsWith(`${to}/`);
}

function Rail() {
  const { pathname } = useLocation();
  const { data: instances } = useInstances();
  const openNew = useNewInstance((s) => s.setOpen);
  const pages = NAV.filter((n) => n.to !== "/settings");
  const settings = NAV.find((n) => n.to === "/settings")!;
  const recent = sortInstances(instances ?? [], "played")
    .filter((i) => i.last_played_unix || i.running)
    .slice(0, QUICK_LAUNCH);

  return (
    <aside className="flex w-[68px] shrink-0 flex-col items-center gap-1.5 overflow-y-auto border-r border-sidebar-border bg-sidebar py-3">
      <Link to="/" className="mb-3 flex size-11 items-center justify-center rounded-xl" aria-label="Library">
        <BrandMark className="size-8" />
      </Link>

      {pages.map(({ to, label, icon: Icon, end }) => (
        <RailTip key={to} label={label}>
          <Link to={to} className={railClass(isActive(pathname, to, end))} aria-label={label}>
            <Icon className="size-5" strokeWidth={1.85} />
          </Link>
        </RailTip>
      ))}

      {recent.length > 0 && <div className="my-2 h-px w-8 shrink-0 bg-sidebar-border" />}
      {recent.map((i) => (
        <ContextMenu key={i.slug}>
          <RailTip
            label={
              <span>
                {i.name}
                {i.running && <span className="ml-1.5 text-success">● running</span>}
              </span>
            }
          >
            <ContextMenuTrigger asChild>
              <Link
                to={`/instance/${i.slug}`}
                className={cn(railClass(pathname === `/instance/${i.slug}`), "p-1")}
                aria-label={i.name}
              >
                <InstanceIcon instance={i} className="size-9" />
              </Link>
            </ContextMenuTrigger>
          </RailTip>
          <InstanceContextMenuContent instance={i} />
        </ContextMenu>
      ))}

      <RailTip label="Create instance">
        <button
          className={cn(railClass(false), "mt-1 border border-dashed border-sidebar-border")}
          onClick={() => openNew(true)}
          aria-label="Create instance"
        >
          <Plus className="size-5" strokeWidth={1.85} />
        </button>
      </RailTip>

      <div className="flex-1" />
      <RailTip label={settings.label}>
        <Link
          to={settings.to}
          className={railClass(isActive(pathname, settings.to, settings.end))}
          aria-label={settings.label}
        >
          <settings.icon className="size-5" strokeWidth={1.85} />
        </Link>
      </RailTip>
    </aside>
  );
}

/** Ctrl+1…5 jump between pages; Ctrl+, opens settings; Ctrl+N creates an instance. */
function useGlobalShortcuts() {
  const navigate = useNavigate();
  const openNew = useNewInstance((s) => s.setOpen);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      if (e.key === ",") {
        e.preventDefault();
        navigate("/settings");
      } else if (e.key.toLowerCase() === "n") {
        e.preventDefault();
        openNew(true);
      } else if (/^[1-5]$/.test(e.key)) {
        e.preventDefault();
        navigate(NAV[Number(e.key) - 1].to);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate, openNew]);
}

/** Icon rail + top bar frame every screen renders inside. */
export function AppShell() {
  const theme = useTheme((s) => s.theme);
  const openPalette = usePalette((s) => s.setOpen);
  useGlobalShortcuts();

  return (
    <TooltipProvider delayDuration={250}>
      <div className="flex h-screen overflow-hidden">
        <Rail />
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
            <button
              className="flex h-7 w-72 items-center gap-2 rounded-md border bg-muted/40 px-2.5 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              onClick={() => openPalette(true)}
            >
              <Search className="size-3.5" />
              Search instances, pages…
              <Kbd className="ml-auto">Ctrl K</Kbd>
            </button>
            <div className="flex-1" />
            <TaskTray />
            <div className="h-5 w-px bg-border" />
            <AccountSwitcher />
          </header>
          <main className="min-h-0 flex-1 overflow-y-auto">
            <Outlet />
          </main>
        </div>
      </div>
      <CommandPalette />
      <GlobalContextMenu />
      <NewInstanceDialog />
      <InstanceDialogsHost />
      <Toaster theme={theme} position="bottom-right" />
    </TooltipProvider>
  );
}
