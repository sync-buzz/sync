"use client";

import { createContext, useContext, useMemo, type ReactNode } from "react";

import type { SettingsHandle } from "@/lib/extension-api/contract";

/**
 * What an area reads and writes of its own settings, and how it reaches the
 * host.
 *
 * Here rather than beside the host's storage, and for the reason `badge.tsx`
 * is here: this is the surface, and the surface may not depend on the loader.
 * The host imports [`SettingsScope`] from it; an extension imports
 * [`useSettings`], and the context both use has to be the same object, so it
 * is neither side's to hold.
 */

/**
 * Absent outside a window, which is the ordinary case for a component under
 * test or in a story. `useSettings` then returns a handle whose `values` is
 * `null` and whose `set` and `open` do nothing, rather than throwing: an area
 * that cannot reach its settings is still an area.
 */
const Channel = createContext<SettingsHandle | null>(null);

/**
 * Publishes the settings handle for one area's subtree.
 *
 * Wrapped around the whole of a layer — the provider and the columns —
 * because an area is as likely to hold a setting in its provider as in a
 * column. The layers nest, so this also encloses every area visited after
 * this one; it is not a leak, because each of those opens a scope of its own
 * before it renders anything of its own, and the nearer one wins.
 */
export function SettingsScope({
  handle,
  children,
}: {
  handle: SettingsHandle;
  children: ReactNode;
}) {
  const channel = useMemo(() => handle, [handle]);
  return <Channel.Provider value={channel}>{children}</Channel.Provider>;
}

/**
 * Read and write this area's own settings, from inside the area.
 *
 * Returns a [`SettingsHandle`] — `values` for the merged settings (portable
 * and local), `set` to write one field, `open` to raise the sheet the host
 * renders from the schema. Outside a window the handle is inert: `values` is
 * `null`, `set` and `open` do nothing.
 */
export function useSettings(): SettingsHandle {
  const channel = useContext(Channel);
  if (channel === null) {
    return NO_SETTINGS;
  }
  return channel;
}

const NO_SETTINGS: SettingsHandle = {
  values: null,
  set: () => {},
  open: () => {},
};
