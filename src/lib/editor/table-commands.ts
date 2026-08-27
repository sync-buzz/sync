"use client";

/**
 * What can be done to a table, and how the window gets told about it.
 *
 * A table drawn but not editable is a block the editor offers and then refuses
 * to help with: rows and columns are the whole of what a table *is*, and an
 * interface where the only way to get a fourth row is to write the Markdown by
 * hand is one that inserted a picture of a table.
 *
 * The commands live here rather than in the cell that draws them because they
 * are wanted in two places at once — the system's menu under the pointer, and
 * `Format ▸ Table` in the menu bar, which is what a keyboard can reach. Both
 * call the same seven functions, so there is one definition of what "insert a
 * row" means.
 *
 * They act on the caret rather than on a path: every one of them is a transform
 * over the current selection, which is why the cell puts the caret where the
 * menu was opened before showing it.
 *
 * **Column widths are not among them.** The store holds a body as Markdown, a
 * Markdown table has no widths, and a column dragged wider would be back where
 * it started the next time the record was opened — the same rule that decides
 * every other thing this editor does or does not offer.
 */

import { createContext, useContext, useEffect } from "react";

import {
  deleteColumn,
  deleteRow,
  deleteTable,
  insertTableColumn,
  insertTableRow,
} from "@platejs/table";
import type { PlateEditor } from "platejs/react";

/** The seven things a person can do to the table the caret is in. */
export interface TableCommands {
  insertRowAbove: () => void;
  insertRowBelow: () => void;
  insertColumnBefore: () => void;
  insertColumnAfter: () => void;
  deleteRow: () => void;
  deleteColumn: () => void;
  deleteTable: () => void;
}

/**
 * Bind the commands to one editor. Stable for the life of that editor, so the
 * menu is rebuilt when the caret enters or leaves a table and not once per
 * keystroke.
 */
export function tableCommands(editor: PlateEditor): TableCommands {
  return {
    insertRowAbove: () => insertTableRow(editor, { before: true, select: true }),
    insertRowBelow: () => insertTableRow(editor, { select: true }),
    insertColumnBefore: () =>
      insertTableColumn(editor, { before: true, select: true }),
    insertColumnAfter: () => insertTableColumn(editor, { select: true }),
    deleteRow: () => deleteRow(editor),
    deleteColumn: () => deleteColumn(editor),
    deleteTable: () => deleteTable(editor),
  };
}

/**
 * How the table under the caret reaches the menu bar.
 *
 * The menu belongs to the application and is built by the window; the caret is
 * known only to the editor, four components below it. Rather than the window
 * reaching down into the editor's selection, the table reports itself while it
 * holds the caret and withdraws when it stops.
 */
const Report = createContext<(commands: TableCommands | null) => void>(
  () => undefined,
);

export const TableCommandsProvider = Report.Provider;

/**
 * Report the commands while this table holds the caret.
 *
 * `commands` has to be stable — bound once per editor — or this would tell the
 * window something new on every render and rebuild the menu bar with it.
 */
export function useReportTableCommands(commands: TableCommands | null): void {
  const report = useContext(Report);

  useEffect(() => {
    report(commands);
    // Leaving takes the commands with it: `Format ▸ Table` acting on a table
    // the caret left would be the menu bar editing something off screen.
    return () => report(null);
  }, [report, commands]);
}
