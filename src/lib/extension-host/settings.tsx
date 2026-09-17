"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import type { SettingsHandle } from "@/lib/extension-api/contract";
import { command } from "@/lib/command";
import { saveProjectSettings } from "@/lib/project/client";
import type { OpenProject } from "@/lib/project/types";
import type { Packages } from "@/lib/extension-host/packages";

/**
 * The host half of extension settings: reads the schema the manifest declared,
 * loads the values (portable from the project's memory, local from app
 * config), merges them, and provides the `SettingsHandle` the area reads.
 *
 * The extension half — `useSettings` — is in `extension-api/settings.tsx`.
 * This is where the handle is built; the extension reads it through the
 * `SettingsScope` the project window wraps around each area.
 */

/** One field in the settings schema, as the host reads it. */
export interface SettingsField {
  readonly type: "string" | "boolean" | "integer" | "number";
  readonly title: string;
  readonly description: string;
  readonly default?: unknown;
  readonly enum?: readonly string[];
  readonly minimum?: number;
  readonly maximum?: number;
  readonly portable: boolean;
}

/** The schema, parsed into the fields the host renders. */
export interface SettingsSchema {
  readonly properties: Readonly<Record<string, SettingsField>>;
}

/** Parse the raw JSON Schema into the fields the host renders. */
function parseSchema(raw: Record<string, unknown>): SettingsSchema | null {
  const properties = raw["properties"];
  if (typeof properties !== "object" || properties === null) return null;

  const fields: Record<string, SettingsField> = {};
  for (const [key, value] of Object.entries(properties)) {
    if (typeof value !== "object" || value === null) continue;
    const v = value as Record<string, unknown>;
    const type = v["type"];
    if (type !== "string" && type !== "boolean" && type !== "integer" && type !== "number") {
      continue;
    }
    const enumValues = Array.isArray(v["enum"]) ? (v["enum"] as readonly string[]) : undefined;
    fields[key] = {
      type,
      title: typeof v["title"] === "string" ? v["title"] : key,
      description: typeof v["description"] === "string" ? v["description"] : "",
      default: v["default"],
      enum: enumValues,
      minimum: typeof v["minimum"] === "number" ? v["minimum"] : undefined,
      maximum: typeof v["maximum"] === "number" ? v["maximum"] : undefined,
      portable: v["sync:portable"] === true,
    };
  }
  return { properties: fields };
}

/** Fill defaults from the schema for fields that have never been set. */
function withDefaults(
  values: Readonly<Record<string, unknown>>,
  schema: SettingsSchema,
): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...values };
  for (const [key, field] of Object.entries(schema.properties)) {
    if (!(key in merged) && field.default !== undefined) {
      merged[key] = field.default;
    }
  }
  return merged;
}

/** Whether a field is portable, from the schema. */
function isPortable(schema: SettingsSchema, key: string): boolean {
  return schema.properties[key]?.portable ?? false;
}

export interface ExtensionSettingsState {
  readonly handle: SettingsHandle;
  readonly schema: SettingsSchema | null;
  readonly extensionName: string;
  readonly sheetOpen: boolean;
  readonly setSheetOpen: (open: boolean) => void;
}

const NO_SETTINGS: ExtensionSettingsState = {
  handle: {
    values: null,
    set: () => {},
    open: () => {},
  },
  schema: null,
  extensionName: "",
  sheetOpen: false,
  setSheetOpen: () => {},
};

/**
 * Read, merge and write one extension's settings for one project.
 *
 * Returns a state that carries the `SettingsHandle` for the `SettingsScope`,
 * the parsed schema for the settings sheet, and the sheet's open state. When
 * the extension declares no settings, every field is inert — `values` is
 * `null`, `set` and `open` do nothing, the schema is `null`.
 */
export function useExtensionSettings(
  project: OpenProject,
  extensionId: string,
  packages: Packages,
): ExtensionSettingsState {
  const packaged = packages.byId(extensionId);
  const rawSchema = packaged?.settings ?? null;
  const extensionName = packaged?.manifest.name ?? extensionId;

  const schema = useMemo(() => (rawSchema === null ? null : parseSchema(rawSchema)), [rawSchema]);

  // Portable values come from the project's own record — the
  // `InstalledExtension.settings` field, which travels through memory. Read
  // once on mount: the component is keyed by area, which includes the
  // extension id, so a new extension mounts fresh and an uninstalled one
  // unmounts entirely.
  const [portable, setPortable] = useState<Record<string, unknown>>(() => {
    const entry = project.installed.find((e) => e.id === extensionId);
    return entry?.settings ? { ...entry.settings } : {};
  });
  const [local, setLocal] = useState<Record<string, unknown> | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);

  // Local values come from app config, through a Tauri command.
  useEffect(() => {
    let live = true;
    void command<Record<string, unknown>>("extension_settings_load", {
      project: project.path,
      extensionId,
    }).then(
      (values) => {
        if (live) setLocal(values ?? {});
      },
      () => {
        if (live) setLocal({});
      },
    );
    return () => {
      live = false;
    };
  }, [project.path, extensionId]);

  const values = useMemo<Record<string, unknown> | null>(() => {
    if (schema === null || local === null) return null;
    return withDefaults({ ...portable, ...local }, schema);
  }, [schema, portable, local]);

  const set = useCallback(
    (key: string, value: unknown) => {
      if (schema === null) return;

      if (isPortable(schema, key)) {
        const next = { ...portable, [key]: value };
        setPortable(next);
        const entry = project.installed.find((e) => e.id === extensionId);
        if (entry !== undefined) {
          const updatedInstalled = project.installed.map((e) =>
            e.id === extensionId ? { ...e, settings: next } : e,
          );
          void saveProjectSettings(project.path, {
            ...project,
            installed: updatedInstalled,
          }).catch(() => {});
        }
      } else {
        setLocal((prev) => ({ ...(prev ?? {}), [key]: value }));
        void command("extension_settings_set", {
          project: project.path,
          extensionId,
          key,
          value,
        }).catch(() => {});
      }
    },
    [schema, portable, project, extensionId],
  );

  const open = useCallback(() => setSheetOpen(true), []);

  const handle = useMemo<SettingsHandle>(() => ({ values, set, open }), [values, set, open]);

  if (schema === null) {
    return NO_SETTINGS;
  }

  return { handle, schema, extensionName, sheetOpen, setSheetOpen };
}
