# Extension settings

A package may ask for settings. The host reads a schema the package declares,
renders a form from it, and stores the values — some travel with the project's
knowledge, and some stay on this machine. The extension reads the merged
result and asks for the form to open; it does not draw the form, because a
form drawn by every extension is a standard held by none.

Read [`extensions.md`](extensions.md) for what a package is and
[`extension-architecture.md`](extension-architecture.md) for the manifest field
by field. This document is about one field — `settings` — and everything it
opens.

---

## 1. Declaring settings

One field in the manifest, one file in the archive:

```json
{
  "settings": "settings/schema.json"
}
```

A path inside the package, to a JSON Schema file. Optional: a package that
declares none has no settings, and nothing about the extension API changes for
it. The file is hashed and signed like every other — see
[`extension-architecture.md`](extension-architecture.md) §4.

The schema is a standard JSON Schema (draft 2020-12) with one custom keyword:
`sync:portable`. A field that carries `"sync:portable": true` travels with the
project's memory; a field that does not is local to this machine. Everything
else — `type`, `title`, `description`, `default`, `enum`, `minimum`, `maximum`
— is read and rendered by the host.

```json
{
  "type": "object",
  "properties": {
    "apiEndpoint": {
      "type": "string",
      "title": "API endpoint",
      "description": "The URL the extension reaches for data.",
      "sync:portable": true
    },
    "refreshInterval": {
      "type": "integer",
      "title": "Refresh interval (minutes)",
      "description": "How often to check for updates.",
      "default": 30,
      "minimum": 1,
      "maximum": 1440
    },
    "theme": {
      "type": "string",
      "enum": ["light", "dark", "system"],
      "title": "Theme",
      "description": "Which appearance the section uses.",
      "default": "system",
      "sync:portable": true
    },
    "notifications": {
      "type": "boolean",
      "title": "Notifications",
      "description": "Show a count when there are new items.",
      "default": true
    }
  }
}
```

`apiEndpoint`, `theme` and the schema's `sync:portable` mark travel with the
project; `refreshInterval` and `notifications` are this machine's. A colleague
who clones the repository gets the first two and not the second two, which is
the point: an endpoint and a theme are decisions about the project, and a
refresh interval is a decision about the machine.

## 2. Portable and local

**Portable** values live in the project's memory, on the `InstalledExtension`
record — the same record that already carries `prompt` and `tools`. They
travel through `refs/memory/main` with everything else the project knows, and
a second machine that opens the same repository sees them without being asked.

**Local** values live in app config, keyed by project path and extension id.
They never reach a repository, and a colleague who clones gets nothing of
them — which is correct for a refresh interval, a window position, or anything
else that is this machine's answer to a question the project did not ask.

The split is the one the window already keeps — see
[`architecture.md`](architecture.md) §"Where a fact lives." A package's prompt
travels; a package's keychain entry does not. Settings follow the same line,
and `sync:portable` is the mark that draws it.

**A secret is never portable.** A field that stores a token, a key, or anything
that should not be committed does not belong in the settings schema at all —
it belongs in the vault, through `ExtensionVault` on the `ExtensionHost`. The
schema describes preferences, not credentials.

## 3. What the host renders

The host reads the schema, groups the fields by portability, and renders a
sheet — the same `Sheet` component the rest of the window uses for forms. Each
field is a `Setting` row: a label, a detail, and a control, the same shape
every setting in the window has. Portable fields come first, local second, so
a person who cares only about what travels reads the top and leaves.

The controls the host draws, from the schema's `type`:

| Schema type | Control |
| --- | --- |
| `string` | text input |
| `boolean` | segmented toggle (On / Off) |
| `integer`, `number` | number input, with `minimum`/`maximum` as bounds |
| `string` with `enum` | segmented control (few) or select (many) |

Everything applies as it is chosen, like the rest of the settings window: a
form with an Apply button asks a person to confirm something they can already
see. `default` fills a field that has never been set; the host does not write
a default into storage, so a package that changes its default in a new version
sees the new value take effect without a migration.

## 4. What the extension reads

```ts
import { useSettings, type SettingsHandle } from "@sync-buzz/extension-api";
```

`useSettings` returns a `SettingsHandle` — three things and no more:

- **`values`** — the merged settings (portable + local), or `null` while the
  host reads. An extension that asked for a setting and has not been answered
  is not the same as one that was answered with nothing, and `null` is the
  difference between them.
- **`set(key, value)`** — writes one field. The host routes it to portable or
  local storage from the schema, so the extension does not know which of its
  values travel and which stay. That is the point: the schema is the single
  source of what is portable, and the extension reading a value is not the
  extension that decides where it goes.
- **`open()`** — raises the settings sheet the host renders from the schema.
  The same form, reached from the area's own context.

An extension typically opens the sheet from a menu item — `useAppMenu` already
exists, and "Settings…" under the area's own heading is where macOS puts it.
The host does not add the item automatically: the extension knows where in its
own UI the gesture belongs, and a gear icon bolted onto every area is one that
some areas do not need.

## 5. What a settings schema may not do

- **Declare a field that is a secret.** The schema describes preferences; a
  token goes through the vault. A field marked `sync:portable` is written into
  the project's memory and reaches every clone, and a secret that travelled
  would be one that everybody holds.
- **Nest objects or arrays.** The first version draws flat fields — string,
  boolean, number, enum. A schema with nested `properties` is read but the
  host draws nothing for the children, because a form inside a form is a
  second standard the first one did not agree to.
- **Set `sync:portable` on a field whose value is machine-specific by
  nature.** A path, a port, a device name — these are this machine's, and
  marking them portable writes them into a repository where they are wrong on
  the next clone. The schema's author decides, and the decision is theirs to
  get wrong.

## 6. The lifecycle of a value

1. The package is installed. The manifest's `settings` path is read, the
   schema is extracted, and the host holds it.
2. The project opens. The host reads portable values from the
   `InstalledExtension` record and local values from app config, merges them,
   and hands the result to `useSettings`.
3. A field is set. The host writes it to the store its schema declares —
   portable to the project's memory, local to app config — and the merged
   values update.
4. The project is cloned elsewhere. Portable values arrive with the memory;
   local values do not. Defaults from the schema fill what is absent, and the
   extension reads the merged result as though it had always been there.
5. The package is updated. The schema may change — fields added, defaults
   moved, a field renamed. Values the new schema still names are kept; values
   it no longer names are not deleted, and a schema that names them again
   finds them where they were. A default that changed takes effect for any
   field that was never set, because the default was never written.
