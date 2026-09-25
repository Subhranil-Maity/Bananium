import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import {
  ArrowLeft,
  ArrowRight,
  ClipboardPaste,
  Copy,
  Plus,
  RotateCw,
  Scissors,
  Search,
  TextSelect,
  Undo2,
} from "lucide-react";
import { toast } from "sonner";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usePalette } from "@/components/command-palette";
import { useNewInstance } from "@/components/new-instance-dialog";

type Editable = HTMLInputElement | HTMLTextAreaElement;

interface MenuState {
  x: number;
  y: number;
  /** The text field right-clicked, with its selection at that moment. */
  field: { el: Editable; start: number; end: number } | null;
  /** Page text selected at right-click time (outside text fields). */
  selection: string;
}

const TEXT_INPUT_TYPES = new Set(["text", "search", "url", "email", "password", "number", "tel", ""]);

function editableFrom(target: EventTarget | null): Editable | null {
  if (target instanceof HTMLTextAreaElement) return target;
  if (target instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(target.type)) return target;
  return null;
}

/**
 * Replaces the webview's native right-click menu (Reload / Inspect / ...)
 * with a themed one everywhere a component hasn't supplied its own: text
 * fields get edit actions, selected text gets Copy, anywhere else gets
 * navigation. Component menus (Radix `ContextMenu`) call `preventDefault`
 * first, which is how this knows to stay out of their way.
 */
export function GlobalContextMenu() {
  const navigate = useNavigate();
  const openPalette = usePalette((s) => s.setOpen);
  const openNew = useNewInstance((s) => s.setOpen);
  const [menu, setMenu] = useState<MenuState | null>(null);

  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      if (e.defaultPrevented) return;
      e.preventDefault();
      const el = editableFrom(e.target);
      setMenu({
        x: e.clientX,
        y: e.clientY,
        field: el && !el.readOnly ? { el, start: el.selectionStart ?? 0, end: el.selectionEnd ?? 0 } : null,
        selection: el ? "" : (window.getSelection()?.toString() ?? ""),
      });
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);

  /** Put focus and selection back in the field the menu was opened on. */
  function refocus(): Editable | null {
    if (!menu?.field) return null;
    const { el, start, end } = menu.field;
    el.focus();
    try {
      el.setSelectionRange(start, end);
    } catch {
      // Some input types (number) don't support selection ranges.
    }
    return el;
  }

  const fieldText = menu?.field ? menu.field.el.value.slice(menu.field.start, menu.field.end) : "";

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      toast.error("Couldn't copy to the clipboard");
    }
  }

  // Edits go through execCommand("insertText"), which keeps the field's
  // undo history and fires the input events React listens to.
  function cut() {
    void copy(fieldText);
    if (refocus()) document.execCommand("insertText", false, "");
  }
  async function paste() {
    try {
      const text = await navigator.clipboard.readText();
      if (refocus()) document.execCommand("insertText", false, text);
    } catch {
      toast.error("Couldn't read the clipboard", { description: "Use Ctrl+V instead." });
    }
  }

  const selecting = menu?.field ? fieldText.length > 0 : (menu?.selection.length ?? 0) > 0;

  return (
    <DropdownMenu open={menu !== null} onOpenChange={(o) => !o && setMenu(null)}>
      <DropdownMenuTrigger asChild>
        <span aria-hidden className="pointer-events-none fixed size-0" style={{ left: menu?.x ?? 0, top: menu?.y ?? 0 }} />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        side="bottom"
        sideOffset={2}
        className="w-52"
        // Keep focus handling ours: refocus() restores it to the field.
        onCloseAutoFocus={(e) => e.preventDefault()}
      >
        {menu?.field ? (
          <>
            <DropdownMenuItem onSelect={() => refocus() && document.execCommand("undo")}>
              <Undo2 /> Undo <DropdownMenuShortcut>Ctrl+Z</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={!selecting} onSelect={cut}>
              <Scissors /> Cut <DropdownMenuShortcut>Ctrl+X</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!selecting} onSelect={() => void copy(fieldText)}>
              <Copy /> Copy <DropdownMenuShortcut>Ctrl+C</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => void paste()}>
              <ClipboardPaste /> Paste <DropdownMenuShortcut>Ctrl+V</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => refocus()?.select()}>
              <TextSelect /> Select all <DropdownMenuShortcut>Ctrl+A</DropdownMenuShortcut>
            </DropdownMenuItem>
          </>
        ) : (
          <>
            {selecting && (
              <>
                <DropdownMenuItem onSelect={() => void copy(menu!.selection)}>
                  <Copy /> Copy <DropdownMenuShortcut>Ctrl+C</DropdownMenuShortcut>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
              </>
            )}
            <DropdownMenuItem onSelect={() => navigate(-1)}>
              <ArrowLeft /> Back <DropdownMenuShortcut>Alt+←</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => navigate(1)}>
              <ArrowRight /> Forward <DropdownMenuShortcut>Alt+→</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => window.location.reload()}>
              <RotateCw /> Reload <DropdownMenuShortcut>Ctrl+R</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => openPalette(true)}>
              <Search /> Search… <DropdownMenuShortcut>Ctrl+K</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => openNew(true)}>
              <Plus /> Create instance <DropdownMenuShortcut>Ctrl+N</DropdownMenuShortcut>
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
