"use client";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Plus, X } from "lucide-react";

import { EDGE_HEIGHT, STRIP_HEIGHT } from "@/lib/console/height";
import { tabIsBusy, tabLabel, type Tab } from "@/lib/console/types";
import { cn } from "@/lib/utils";

/**
 * The tab strip, at the very bottom of the shade.
 *
 * At the bottom rather than the top, and three things decided it. The top edge
 * of the shade is the one that flies in and out, so navigation placed there
 * moves further than anything else on screen every time the console is called.
 * After a command the eye returns downwards, to the line it types into, and the
 * tabs fall inside that same zone of attention. And the foot of the shade
 * becomes its frame rather than a second ceiling over the work.
 *
 * **The bottom six pixels are not the strip's.** They belong to the edge the
 * shade is resized by, which moving the tabs down here put underneath them. A
 * tab whose hit area reached the floor would swallow every grab that missed the
 * edge by a pixel, so the strip keeps its height and stops short of it.
 *
 * **One slot on the right of each name, and it is never empty.** The close
 * cross lives there at all times, dimmed to the point where it is a shape the
 * eye skips rather than a control competing with the name; it comes up to full
 * strength under the pointer and takes a background of its own under a pointer
 * aimed at it. A tab that is busy shows a dot in that same place instead, and
 * the pointer swaps it back for the cross — so the slot always holds exactly
 * one mark. A cross that disappears leaves a hole where it was, and a hole is
 * more distracting than the thing it replaced.
 */

/** The mark slot beside a name: wide enough for the cross, and fixed. */
const SLOT = 16;

export function ConsoleTabs({
  tabs,
  activeId,
  onSelect,
  onClose,
  onRename,
  onOpen,
}: {
  readonly tabs: readonly Tab[];
  readonly activeId: string | null;
  readonly onSelect: (id: string) => void;
  readonly onClose: (id: string) => void;
  readonly onRename: (id: string, name: string | null) => void;
  readonly onOpen: () => void;
}) {
  /** The tab being renamed, and what has been typed for it so far. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  /**
   * A tab has been asked for and is to be named as soon as it exists.
   *
   * A flag rather than a name handed back by whatever made the tab: the strip
   * asks for one and the console decides what it is, so the identity does not
   * exist yet at the moment of asking. The effect below picks it up on the
   * render that has it.
   */
  const [naming, setNaming] = useState(false);

  // Answered during the render that has the tab rather than in an effect after
  // it: an effect would draw one frame of a new tab with its name settled, and
  // the field would open over the top of it. The same reading `project-window`
  // gives an intent arriving from outside, and for the same reason.
  if (naming && tabs.length > 0) {
    const made = tabs[tabs.length - 1]!;
    setNaming(false);
    setTyped(tabLabel(made));
    setRenaming(made.id);
  }

  const commit = (id: string) => {
    onRename(id, typed);
    setRenaming(null);
  };

  return (
    <nav
      aria-label="Console tabs"
      // A double click on the strip itself makes a tab and opens its name for
      // typing — the gesture the Finder, a browser and every tab strip with
      // room to spare have taught, and the one place on the shade where there
      // is empty space to spend on it. Only on the strip's own background:
      // `currentTarget` is this element, and a double click that landed on a
      // tab is that tab being renamed.
      onDoubleClick={(event) => {
        if (event.target !== event.currentTarget) return;
        onOpen();
        setNaming(true);
      }}
      className="flex shrink-0 items-stretch gap-0.5 overflow-x-auto px-2"
      style={{
        height: `${STRIP_HEIGHT}px`,
        paddingBottom: `${EDGE_HEIGHT}px`,
      }}
    >
      {tabs.map((tab) => {
        const selected = tab.id === activeId;
        const label = tabLabel(tab);

        if (tab.id === renaming) {
          return (
            <RenameField
              key={tab.id}
              value={typed}
              onChange={setTyped}
              onCommit={() => commit(tab.id)}
              onCancel={() => setRenaming(null)}
            />
          );
        }

        const busy = tabIsBusy(tab);

        return (
          <div
            key={tab.id}
            className={cn(
              // A capsule rather than a rounded rectangle, because at this
              // height a corner radius reads as an almost-capsule that
              // somebody got wrong. Fully round is a decision; seven pixels of
              // radius on a twenty-four pixel box is a compromise.
              "group/tab flex h-6 shrink-0 items-center gap-1 rounded-full pr-1 pl-2.5 transition-colors duration-(--motion-duration-fast) ease-shell",
              selected ? "bg-selected" : "hover:bg-hover",
            )}
          >
            <button
              type="button"
              onClick={() => onSelect(tab.id)}
              // Naming a tab is renaming it, which is what a double click means
              // on every label a person is allowed to change — in the Finder,
              // in a tab strip, on a layer. Nothing else here wants the
              // gesture.
              onDoubleClick={() => {
                setTyped(label);
                setRenaming(tab.id);
              }}
              aria-current={selected ? "true" : undefined}
              className={cn(
                "max-w-40 truncate font-mono text-xs tracking-tight transition-colors duration-(--motion-duration-fast) ease-shell",
                selected
                  ? "text-fg"
                  : "text-fg-tertiary group-hover/tab:text-fg-secondary",
              )}
            >
              {label}
            </button>

            <span
              className="flex shrink-0 items-center justify-center"
              style={{ width: `${SLOT}px` }}
            >
              {/* The dot is what a busy tab shows while nobody is pointing at
                  it, and the pointer trades it for the cross. Both stand in
                  this one slot, so the name beside them never moves. */}
              {busy ? (
                <span
                  role="img"
                  aria-label="Working"
                  className="size-1 rounded-full bg-fg-secondary group-hover/tab:hidden"
                />
              ) : null}

              <button
                type="button"
                onClick={() => onClose(tab.id)}
                className={cn(
                  "flex size-4 items-center justify-center rounded-full text-fg-tertiary transition-[opacity,background-color,color] duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg hover:opacity-100 focus-visible:opacity-100",
                  // Dimmed to a shape rather than hidden. It is there to be
                  // found without being read: at this weight the eye passes
                  // over it while scanning names and lands on it the moment it
                  // is looked for.
                  "opacity-35 group-hover/tab:opacity-70",
                  busy && "hidden group-hover/tab:flex",
                )}
              >
                <X aria-hidden className="size-3" />
                <span className="sr-only">Close {label}</span>
              </button>
            </span>
          </div>
        );
      })}

      {/* The same capsule as a tab, at the size of the mark inside one, so it
          sits on the strip as a member of the row rather than as a control
          parked beside it. */}
      <button
        type="button"
        onClick={onOpen}
        title="New tab"
        className="my-auto flex size-6 shrink-0 items-center justify-center rounded-full text-fg-tertiary opacity-60 transition-[opacity,background-color,color] duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg hover:opacity-100"
      >
        <Plus aria-hidden className="size-3" />
        <span className="sr-only">New tab</span>
      </button>
    </nav>
  );
}

/**
 * The name of a tab while it is being typed.
 *
 * It takes the tab's place rather than opening anywhere else: renaming in
 * position is what makes it obvious which tab is being renamed, and it is the
 * only place on the strip with room for the answer anyway.
 *
 * Committed by Return and by losing the caret both, which is the behaviour
 * every label of this kind has — clicking away is how most people finish, and
 * a field that threw the name away for it would be a field that punishes the
 * ordinary case. Escape is the way out that keeps the old name.
 */
function RenameField({
  value,
  onChange,
  onCommit,
  onCancel,
}: {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly onCommit: () => void;
  readonly onCancel: () => void;
}) {
  const field = useRef<HTMLInputElement>(null);
  /** Escape has to leave without the blur that follows it committing. */
  const cancelled = useRef(false);

  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // The shade answers Escape by closing and `⌘1`…`⌘9` by switching tabs.
    // Neither is what those keys mean inside a field somebody is typing in, so
    // the field says it has dealt with them.
    event.stopPropagation();

    if (event.key === "Enter") {
      event.preventDefault();
      onCommit();
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      cancelled.current = true;
      onCancel();
    }
  };

  return (
    <input
      ref={field}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={onKeyDown}
      onBlur={() => {
        if (cancelled.current) return;
        onCommit();
      }}
      aria-label="Tab name"
      spellCheck={false}
      className="h-6 shrink-0 rounded-full bg-selected px-2.5 font-mono text-xs text-fg outline-none"
      // Sized to what is in it, because the strip is a row of names and a field
      // of some standing width would push every tab after it aside while
      // somebody typed.
      //
      // The padding has to be in the sum. The box is measured border-box, so a
      // width of the text alone is a field narrower than its own text by both
      // insets — the line scrolls inside it and what a person sees is the
      // *end* of the name they are trying to read. The extra quarter of an em
      // past the two insets is the caret's, which sits after the last glyph
      // and would otherwise be the thing that overflows. No tightened tracking
      // either: `ch` is the width of one character in this face, and a
      // letter-spacing this does not know about makes every character wider
      // than the unit it is being counted in.
      style={{ width: `calc(${Math.max(value.length, 3)}ch + 1.5rem)` }}
    />
  );
}
