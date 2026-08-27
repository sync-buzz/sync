"use client";

/**
 * The menu bar, which is the system's and has to exist.
 *
 * A Mac application without one is not a Mac application: `⌘,`, `⌘W`, `⌘Q`,
 * Hide, Services and the editing commands are things a person expects to find
 * by looking, and the ones the system routes for free — Undo, Cut, Copy, Paste,
 * Select All — do not work fully in a webview until a menu claims them. Sync
 * carried none of that and answered two of the shortcuts with its own
 * `keydown` handlers, which fired in text fields and appeared nowhere.
 *
 * So the menu is built from Tauri's own menu API — no plugin, nothing granted
 * beyond `core:default`, which already covers `core:menu`, exactly as the
 * context menu is. Everything in it is a predefined item except the commands
 * that are ours, and predefined items are the system's implementations rather
 * than ours wearing its labels.
 *
 * **File is where a thing is made.** Writing a record is not what this window
 * exists for — Sync is not a text editor — but it is a command, and a command
 * on macOS is in the menu bar whether or not it is the one an application is
 * opened for. `⌘N` is where a person looks for it, and the `+` beside the list
 * it writes into is where they find it by looking.
 *
 * A record has a kind, and the kind is the navigator's selection rather than a
 * question this menu asks: `⌘N` writes one of what the workspace is showing and
 * names it, and with a view showing it is disabled. Sync's own type is never
 * what it names — the record naming the project is the one record the window
 * neither creates nor removes.
 *
 * No command carries an ellipsis, for the reason nothing else in the shell
 * does: the shell has too few commands for the distinction to earn its
 * punctuation, and half of them marked would read as an inconsistency.
 */

import { useEffect, useRef } from "react";
import type { TableCommands } from "@/lib/editor/table-commands";
import { nativeMenusAvailable, queueMenuWork } from "@/lib/menu-queue";
import { openSettings } from "@/lib/settings/window";
import { openNewWindow } from "@/lib/window-open";

/** A kind a record can be written as, as the navigator lists it. */
export interface MenuRecordType {
  kind: string;
  title: string;
}

/**
 * What File can do at this moment. Every field is read at the moment a command
 * is chosen rather than when the menu was built, so a window that has since
 * moved on answers with what is true then.
 */
export interface WindowCommands {
  /**
   * The kind the workspace is showing, which is the one thing `⌘N` writes.
   * `null` where it is showing a view rather than a kind, and the command has
   * nothing to act on.
   */
  selected: MenuRecordType | null;
  /** Write a record of one kind, or `null` where the window cannot. */
  createRecord: ((kind: string) => void) | null;
  /** Name a new type, or `null` where the window cannot. */
  createType: (() => void) | null;
  /**
   * What can be done to the table the caret is in, or `null` when it is not in
   * one. A table is the one block whose editing is more than typing into it,
   * and this is the half of that a keyboard can reach — the other half is the
   * system's menu on the cell itself.
   */
  table: TableCommands | null;
}

/**
 * The menu the system is currently wearing, held so that nothing frees it while
 * the menu bar still has it: menus live in the Rust process and the object here
 * is a handle, so a handle that is collected takes the menu with it.
 *
 * Beside it, the number of the last installation asked for. Building a menu is
 * asynchronous and a selection can change while one is being built, so only the
 * one asked for last is allowed to reach the bar.
 */
let installedMenu: { close: () => Promise<void> } | null = null;
let generation = 0;

/**
 * The window's own commands, and the reason they are not in
 * [`WindowCommands`].
 *
 * Everything in that interface is an *area's*: the kind `⌘N` writes, the table
 * the caret is in. Synchronisation is not — it is true of the whole project at
 * once, whichever area is showing, and threading it through every area so each
 * could hand back something none of them owns would have made a window-level
 * fact into an area's paperwork.
 *
 * So it lives beside the menu instead, registered by the window and read when
 * the menu is built and again when an item is chosen.
 */
let readMemory: () => MemoryCommands | null = () => null;

/**
 * How the last menu read its area's commands, kept so that a change to the
 * window's own can rebuild without an area asking for it.
 */
let readArea: () => WindowCommands | null = () => null;

/** Moving a project's memory to and from its remote. */
export interface MemoryCommands {
  /** Bring what is on the remote here, or `null` with no remote configured. */
  fetch: (() => void) | null;
  /** Put what is here on the remote, or `null` with no remote configured. */
  publish: (() => void) | null;
  /** True while one of them is already in flight. */
  busy: boolean;
}

/**
 * Keep `Memory` in step with what the project's remote allows.
 *
 * Called by the window rather than by an area, which is the whole point — see
 * [`readMemory`]. Like `File`, the items are always drawn and disabled when
 * there is nothing for them to do: a menu whose items come and go teaches
 * nobody where a command lives.
 */
export function useMemoryMenu(memory: MemoryCommands | null): void {
  const signature = JSON.stringify(
    memory === null
      ? null
      : [memory.fetch !== null, memory.publish !== null, memory.busy],
  );

  // **A getter, the way the area's commands are read**, rather than a value
  // copied into the module when the menu is built. What the menu *looks* like
  // depends only on the signature, so rebuilding it on every render would be a
  // native menu rebuilt for nothing. What the items *do* is a pair of closures
  // over the open project, and those change without the signature moving:
  // switching to another project whose remote is configured the same way leaves
  // both flags and `busy` exactly as they were. Copying the value in only when
  // the menu was rebuilt therefore left the slot holding the *previous*
  // project's commands, so `Fetch` reached into the project somebody had just
  // navigated away from. Read late, and the question is always about now.
  const latest = useRef(memory);
  useEffect(() => {
    latest.current = memory;
  });

  useEffect(() => {
    readMemory = () => latest.current;
    // Nothing to reach for once the window holding it is gone. The menu is the
    // application's and outlives this window.
    return () => {
      readMemory = () => null;
    };
  }, []);

  useEffect(() => {
    installAppMenu(readArea);
  }, [signature]);
}

/**
 * Keep the application menu in step with what the window can do.
 *
 * The menu belongs to the application rather than to a window, so it is one
 * menu whoever calls this: the shell installs it with nothing to create, and
 * the open project replaces it with its own kinds. It is rebuilt only when what
 * it *says* changes — a re-render that hands over new closures is not a new
 * menu, because the commands are read through a ref.
 */
/**
 * @param enabled False for an area that is mounted but not selected. Such an
 *   area keeps its state and its DOM, and must keep its hands off the menu:
 *   the window has one menu bar, and the area a person is looking at is the one
 *   that fills it.
 */
export function useAppMenu(
  file: WindowCommands | null,
  enabled = true,
): void {
  const latest = useRef(file);
  // Declared before the installation below so that a rebuild triggered by the
  // same render reads the commands of that render rather than the last one's.
  useEffect(() => {
    latest.current = file;
  });

  const signature = JSON.stringify(
    file === null
      ? null
      : [
          file.selected?.kind ?? null,
          file.selected?.title ?? null,
          file.createRecord !== null,
          file.createType !== null,
          // Whether the caret is in a table, not which one: the commands read
          // the selection, so moving from one cell to the next says nothing new.
          file.table !== null,
        ],
  );

  useEffect(() => {
    if (!enabled) return;
    installAppMenu(() => latest.current);
  }, [signature, enabled]);
}

/**
 * Install the application menu, reading its commands from the window that asked
 * for it whenever one is chosen.
 *
 * Asked for here, built when the menus of this window are free — see
 * [`@/lib/menu-queue`]. Selecting a row with the secondary button is a menu bar
 * that wants rebuilding *and* a context menu going up in the same breath, and
 * building one while the other is on screen is a window that never comes back.
 */
function installAppMenu(read: () => WindowCommands | null): void {
  // Kept so that a change to the window's own commands can rebuild the menu
  // without an area asking for one.
  readArea = read;
  if (!nativeMenusAvailable()) return;

  watchTheFocus();

  const asked = ++generation;

  void queueMenuWork(() => buildAppMenu(asked, read));
}

/** Whether this window has already asked to hear about its own focus. */
let watchingTheFocus = false;

/**
 * Put this window's menu back on the bar whenever it becomes the front one.
 *
 * There is one menu bar and any number of windows, so the last window to build
 * a menu owns it — which, with two windows open, is whichever one was touched
 * least recently rather than the one a person is looking at. `⌘N` would then
 * write a record into the window behind, which is the worst kind of wrong: it
 * works, silently, somewhere else.
 *
 * Rebuilt from `readArea` rather than from anything passed in, because that is
 * this window's own last word about what it can do — every document has its
 * own copy of this module.
 */
function watchTheFocus(): void {
  if (watchingTheFocus) return;
  watchingTheFocus = true;

  void (async () => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        if (focused) installAppMenu(readArea);
      });
    } catch (error) {
      // The menu still works; it is the handover between windows that does not,
      // so this is reported rather than escalated.
      console.error("The window could not follow the focus.", error);
    }
  })();
}

/** Build the menu this installation asked for, and put it on the bar. */
async function buildAppMenu(
  asked: number,
  read: () => WindowCommands | null,
): Promise<void> {
  // Its turn can come after a context menu has been and gone, by which time
  // later selections may have asked for menus of their own. Only the last of
  // them describes the window as it stands, so the rest are dropped before
  // anything is built rather than built and thrown away.
  if (asked !== generation) return;

  try {
    const { Menu } = await import("@tauri-apps/api/menu");

    const menu = await Menu.new({
      items: [
        {
          // On macOS the first submenu is the application menu, and the system
          // titles it with the application's name whatever this text says.
          text: "Sync",
          items: [
            { item: { About: null } },
            { item: "Separator" },
            {
              text: "Settings",
              accelerator: "CmdOrCtrl+,",
              action: () => void openSettings(),
            },
            { item: "Separator" },
            { item: "Services" },
            { item: "Separator" },
            { item: "Hide" },
            { item: "HideOthers" },
            { item: "ShowAll" },
            { item: "Separator" },
            { item: "Quit" },
          ],
        },
        {
          text: "File",
          items: fileItems(read),
        },
        {
          // Its own submenu rather than an entry under File, because nothing
          // here makes anything: this is where a project's memory is moved
          // between this machine and the remote it shares with everyone else.
          text: "Memory",
          items: memoryItems(),
        },
        {
          text: "Edit",
          items: [
            { item: "Undo" },
            { item: "Redo" },
            { item: "Separator" },
            { item: "Cut" },
            { item: "Copy" },
            { item: "Paste" },
            { item: "SelectAll" },
          ],
        },
        {
          // Where this system keeps what is done *to* content rather than with
          // the clipboard. It holds one thing, because a table is the one block
          // whose editing is more than typing into it.
          text: "Format",
          items: [{ text: "Table", items: tableItems(read) }],
        },
        {
          text: "Window",
          items: [
            { item: "Minimize" },
            { item: "Fullscreen" },
            { item: "Separator" },
            { item: "CloseWindow" },
            { item: "BringAllToFront" },
          ],
        },
      ],
    });

    // Something asked for a menu of its own while this one was being built, and
    // that one describes the window as it stands now. This one was never on the
    // bar, so it is freed here rather than left in the Rust process for nothing.
    if (asked !== generation) {
      await menu.close().catch(() => undefined);
      return;
    }

    await menu.setAsAppMenu();

    // The one it replaced is released only now, with the new one already on the
    // bar: released any earlier it would be a menu the system is still drawing.
    const previous = installedMenu;
    installedMenu = menu;
    await previous?.close().catch(() => undefined);
  } catch (error) {
    // The window still works without it, and the header still opens settings
    // and still writes records, so this is reported rather than escalated — but
    // it is reported, because an application silently missing its menu bar is a
    // broken one.
    console.error("The application menu could not be installed.", error);
  }
}

/**
 * What File offers, which is whatever the window can currently make.
 *
 * One command, not a list of them: the kind is chosen in the navigator, and the
 * menu writes the kind the workspace is showing — the same act the `+` beside
 * that list performs, said in the place a keyboard can reach. A submenu of
 * every kind was the earlier draft and it was wrong twice over: it asked a
 * question the window had already answered, and it made `⌘N` mean a different
 * command depending on which row of a list it landed on.
 *
 * The command names the kind it would write, so the menu says what is about to
 * happen before it happens. Where nothing is selected it keeps its place and is
 * disabled, because a menu that loses an item between one moment and the next
 * teaches nobody where the command lives.
 */
function fileItems(read: () => WindowCommands | null) {
  const file = read();
  const selected = file?.selected ?? null;

  return [
    {
      // First, above everything the window writes *into* itself, and never
      // disabled: a window can always be opened, including from the settings
      // window, which can make nothing else.
      //
      // `⌥⌘N` rather than the `⌘N` this command has in a browser, because `⌘N`
      // is already the record — Sync is opened to write into a project far more
      // often than to open a second one, and moving it would break the reflex
      // that matters for the one that does not.
      text: "New Window",
      accelerator: "CmdOrCtrl+Alt+N",
      action: () => void openNewWindow(),
    },
    { item: "Separator" as const },
    {
      text: selected ? `New ${selected.title}` : "New Record",
      accelerator: "CmdOrCtrl+N",
      enabled: selected !== null && file?.createRecord != null,
      action: () => {
        const now = read();
        if (now?.selected) now.createRecord?.(now.selected.kind);
      },
    },
    {
      text: "New Type",
      accelerator: "CmdOrCtrl+Shift+N",
      enabled: file?.createType != null,
      action: () => read()?.createType?.(),
    },
  ];
}

/**
 * The two directions memory moves in.
 *
 * Both are also in the sheet the header's indicator opens, which is where a
 * pointer will look for them; this is where a keyboard does, and it is the
 * shell's rule that nothing is reachable only one of those ways.
 *
 * No accelerators. `⌘R` and `⌘S` are the obvious guesses and both are wrong
 * here — this application does not reload and does not have a save command, and
 * binding either to something that reaches the network would be teaching a
 * reflex that costs a push.
 */
function memoryItems() {
  const memory = readMemory();

  return [
    {
      text: "Fetch",
      enabled: memory?.fetch != null && !memory.busy,
      action: () => readMemory()?.fetch?.(),
    },
    {
      text: "Publish",
      enabled: memory?.publish != null && !memory.busy,
      action: () => readMemory()?.publish?.(),
    },
  ];
}

/**
 * What can be done to the table the caret is in.
 *
 * Every one of them is drawn whether or not there is a table, and disabled when
 * there is not — the way `Format` behaves in every application on this system
 * that has one. A submenu that appeared with the caret would be a menu bar that
 * changed shape as somebody typed.
 *
 * The same seven are under the secondary button on a cell, which is where a
 * pointer will look for them; this is where a keyboard does.
 */
function tableItems(read: () => WindowCommands | null) {
  const enabled = read()?.table != null;

  const command = (text: string, run: (table: TableCommands) => void) => ({
    text,
    enabled,
    action: () => {
      const table = read()?.table;
      if (table) run(table);
    },
  });

  return [
    command("Insert Row Above", (table) => table.insertRowAbove()),
    command("Insert Row Below", (table) => table.insertRowBelow()),
    { item: "Separator" as const },
    command("Insert Column Before", (table) => table.insertColumnBefore()),
    command("Insert Column After", (table) => table.insertColumnAfter()),
    { item: "Separator" as const },
    command("Delete Row", (table) => table.deleteRow()),
    command("Delete Column", (table) => table.deleteColumn()),
    command("Delete Table", (table) => table.deleteTable()),
  ];
}
