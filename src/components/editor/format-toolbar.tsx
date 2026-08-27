"use client";

/**
 * What appears over a selection.
 *
 * It exists because the alternative is a permanent band of controls above the
 * text, and this window has one band per column already. A toolbar that is there
 * only while something is selected is the version that costs the workspace
 * nothing for as long as nobody is formatting anything.
 *
 * Every command on it is also a keyboard shortcut and a Markdown spelling, so
 * nothing here is the only way to reach anything — the rule the shell's native
 * menus follow, for the same reason. The controls are icon-only, which means each
 * one carries an accessible name, a tooltip, and its state in `aria-pressed`.
 */

import type { ComponentType } from "react";

import {
  flip,
  offset,
  shift,
  useFloatingToolbar,
  useFloatingToolbarState,
} from "@platejs/floating";
import { unwrapLink } from "@platejs/link";
import { triggerFloatingLink, useLinkToolbarButtonState } from "@platejs/link/react";
import { someList, toggleList } from "@platejs/list";
import {
  Bold,
  Code,
  Heading1,
  Heading2,
  Italic,
  Link as LinkIcon,
  List,
  ListOrdered,
  Strikethrough,
  TextQuote,
} from "lucide-react";
import { KEYS, type SlateEditor } from "platejs";
import {
  useEditorId,
  useEditorRef,
  useEditorSelector,
  useEventEditorValue,
} from "platejs/react";

import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface Command {
  label: string;
  icon: ComponentType;
  /** Whether the selection already carries it. */
  active: (editor: SlateEditor) => boolean;
  run: (editor: SlateEditor) => void;
}

const MARKS: Command[] = [
  mark("Bold", Bold, KEYS.bold),
  mark("Italic", Italic, KEYS.italic),
  mark("Strikethrough", Strikethrough, KEYS.strikethrough),
  mark("Code", Code, KEYS.code),
];

const BLOCKS: Command[] = [
  block("Heading 1", Heading1, KEYS.h1),
  block("Heading 2", Heading2, KEYS.h2),
  block("Quote", TextQuote, KEYS.blockquote),
  {
    label: "Bulleted list",
    icon: List,
    active: (editor) => someList(editor, "disc"),
    run: (editor) => toggleList(editor, { listStyleType: "disc" }),
  },
  {
    label: "Numbered list",
    icon: ListOrdered,
    active: (editor) => someList(editor, "decimal"),
    run: (editor) => toggleList(editor, { listStyleType: "decimal" }),
  },
];

function mark(label: string, icon: ComponentType, key: string): Command {
  return {
    label,
    icon,
    active: (editor) => editor.api.marks()?.[key] === true,
    run: (editor) => editor.tf.toggleMark(key),
  };
}

function block(label: string, icon: ComponentType, key: string): Command {
  return {
    label,
    icon,
    active: (editor) => editor.api.block()?.[0]?.type === editor.getType(key),
    run: (editor) => editor.tf.toggleBlock(key),
  };
}

export function FormatToolbar() {
  const editorId = useEditorId();
  const focusedEditorId = useEventEditorValue("focus");
  const state = useFloatingToolbarState({
    editorId,
    focusedEditorId,
    floatingOptions: {
      middleware: [
        offset(8),
        flip({ fallbackPlacements: ["bottom-start"], padding: 8 }),
        shift({ padding: 8 }),
      ],
      placement: "top-start",
    },
  });
  const {
    clickOutsideRef,
    hidden,
    props: rootProps,
    ref,
  } = useFloatingToolbar(state);

  if (hidden) return null;

  return (
    <div ref={clickOutsideRef}>
      <div
        ref={ref}
        {...rootProps}
        role="toolbar"
        aria-label="Formatting"
        className="absolute z-50 flex items-center gap-0.5 rounded-(--radius-control) border border-separator-strong bg-raised p-1 shadow-(--shadow-content)"
      >
        {MARKS.map((command) => (
          <CommandButton key={command.label} command={command} />
        ))}
        <LinkButton />
        <Separator orientation="vertical" className="mx-0.5 h-5" />
        {BLOCKS.map((command) => (
          <CommandButton key={command.label} command={command} />
        ))}
      </div>
    </div>
  );
}

/**
 * Make a link out of the selection, or take one off it.
 *
 * One button with two meanings, because to a person they are one question
 * asked of the same words — and `aria-pressed` is what says which of the two is
 * on offer, the same way every other control on this toolbar does.
 *
 * Whether the selection is a link is the plugin's answer rather than ours, and
 * so are both transforms. What is not the plugin's own is the pairing: its own
 * toolbar button moves the caret to the end of the link instead of unmaking it,
 * which is a reasonable second click and not what a pressed button means here.
 */
function LinkButton() {
  const editor = useEditorRef();
  const { pressed } = useLinkToolbarButtonState();

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={pressed ? "Remove link" : "Link"}
          aria-pressed={pressed}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            if (pressed) unwrapLink(editor);
            else triggerFloatingLink(editor, { focused: true });
          }}
          className="text-fg-secondary aria-pressed:bg-selected aria-pressed:text-fg"
        >
          <LinkIcon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{pressed ? "Remove link" : "Link"}</TooltipContent>
    </Tooltip>
  );
}

function CommandButton({ command }: { command: Command }) {
  const editor = useEditorRef();
  const active = useEditorSelector(
    (current) => command.active(current),
    [command],
  );
  const Icon = command.icon;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={command.label}
          aria-pressed={active}
          // The selection is the subject of every command here, and pressing a
          // button would take it away before the command ran.
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => command.run(editor)}
          className="text-fg-secondary aria-pressed:bg-selected aria-pressed:text-fg"
        >
          <Icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{command.label}</TooltipContent>
    </Tooltip>
  );
}
