"use client";

/**
 * Everything about the open record, and where it is changed.
 *
 * The centre column is the claim; this is what is true *of* it. Which of those
 * things a person may change is not this build's decision but the store's, and it
 * comes out as three rules:
 *
 * - **Identity is not editable.** The key is what every link and every agent
 *   refer to and the store has no rename; the kind is what validates the record.
 *   Changing either would be a rewrite of the corpus disguised as a form.
 * - **Freshness is not editable.** The engine derives it by reconciling code
 *   history against the record's scope. It is an answer, not a field.
 * - **Everything else is, on the schema's terms.**
 *
 * ## A control is chosen by what the value *is*
 *
 * The schema says what each field is — `string`, `text`, `integer`, `number`,
 * `boolean`, `enum`, `array` of one of those, `object` — and every one of those
 * words means a different thing to type into. An earlier pass drew a text box
 * for nearly all of them, which is the interface refusing to read a schema it
 * had already been handed: a list of tags typed as `a, b, c` into one box, a
 * set of paths as lines in a textarea, and anything that was not a plain string
 * shown as unreadable JSON and not editable at all.
 *
 * So each declaration is answered by the control that matches it, and macOS has
 * one for most of them:
 *
 * - A **short repeated value** — tags, an array of strings — is a token field.
 *   That is what `NSTokenField` is, and it is why a tag is a thing you can
 *   delete with one Backspace rather than a substring of a line you have to
 *   find the commas in.
 * - A **path** is chosen with the system's open panel, not typed. It is a file
 *   in this repository, the panel opens at the project, and what is stored is
 *   the path relative to it. Typing one by hand is still offered, because a
 *   path can name something that is not there yet.
 * - A **choice** is a pop-up over exactly the values the schema allows; a set of
 *   choices is those values as checkboxes.
 * - A **flag** is a checkbox, a **number** is a number field, a **string** is one
 *   line, and `text` — which the schema means as prose — is the several lines it
 *   says it is.
 * - A **shape this build cannot generate a control for** (an object, an array of
 *   objects) is shown as the store spells it and left alone. That is the honest
 *   answer, and it is the only place this file falls back to one.
 *
 * Discrete choices are written at once; text is written on the same pause as the
 * body, because typing a tag is typing.
 */

import { useId, useRef, useState } from "react";

import { ChevronDown, Plus, X } from "lucide-react";

import { StateMark } from "@/components/shell/entity-marks";
import type {
  DocumentPatch,
  FieldDeclaration,
  MemoryDocument,
  MemoryType,
} from "@/lib/memory/types";
import type { DocumentDraft } from "@/lib/memory/use-document";
import { typeName } from "@/lib/memory/use-corpus";
import { chooseProjectFiles } from "@/lib/project/client";
import { cn } from "@/lib/utils";

export function RecordMetadata({
  document,
  draft,
  type,
  types,
  projectPath,
  onEdit,
  onWrite,
}: {
  /** The record as stored: what the read-only facts describe. */
  document: MemoryDocument;
  /** The record with unwritten edits on top: what every control shows. */
  draft: DocumentDraft;
  /** The record's own type, when the project still holds it. */
  type: MemoryType | undefined;
  /** The corpus, because a link names a kind and a kind has a name. */
  types: readonly MemoryType[];
  /** Where the project is, so the open panel opens inside it. */
  projectPath: string;
  onEdit: (patch: DocumentPatch) => void;
  /** Write now: what a single choice does, rather than waiting for a pause. */
  onWrite: () => void;
}) {
  const choose = (patch: DocumentPatch) => {
    onEdit(patch);
    onWrite();
  };

  return (
    <>
      <section className="space-y-2">
        <Fact label="Type">{typeName(types, document.kind)}</Fact>
        <Fact label="Key">
          <span className="font-mono text-xs">{document.key}</span>
        </Fact>
        <Fact label="State">
          <StateMark freshness={document.freshness} />
        </Fact>
      </section>

      <section className="space-y-2">
        <Toggle
          label="Archived"
          checked={draft.archived}
          onChange={(archived) => choose({ archived })}
        />
        <p className="text-xs text-fg-tertiary">
          An archived record keeps everything that links to it and leaves the
          lists. It is the reversible half of removing something.
        </p>
      </section>

      <section className="space-y-2">
        <Label>Tags</Label>
        <TokenField
          label="Tags"
          values={draft.tags}
          placeholder="Add a tag"
          onChange={(tags) => choose({ tags })}
        />
      </section>

      <PathField
        title="Scope"
        paths={draft.scope}
        projectPath={projectPath}
        empty="Not scoped to any path, so nothing in the code can make it stale."
        onChange={(scope) => choose({ scope })}
      />
      <PathField
        title="Written against"
        paths={draft.observed}
        projectPath={projectPath}
        empty="No files recorded as evidence."
        onChange={(observed) => choose({ observed })}
      />

      <Links draft={draft} type={type} types={types} onEdit={onEdit} onWrite={onWrite} />

      <Fields draft={draft} type={type} onEdit={onEdit} onWrite={onWrite} />
    </>
  );
}

/**
 * One control, at the density of the column it sits in.
 *
 * The height is a token and not a consequence of padding, so a text field, a
 * pop-up and a token field are the same height standing next to each other —
 * which is the only reason a column of controls reads as one column. Nothing
 * here suppresses the focus ring: it is defined once for the whole application
 * in `globals.css`, and a control that turned it off would be the one place
 * the keyboard goes invisible.
 */
const FIELD =
  "h-(--control-height-sm) w-full rounded-(--radius-control) border border-separator-strong bg-raised px-2 text-xs text-fg placeholder:text-fg-tertiary";

/**
 * A pop-up, at that same height.
 *
 * `appearance-none` and a chevron of our own, the way the project sheet already
 * draws one: a select left to itself is drawn by the engine at whatever height
 * it likes, with its own arrow, and it was the one control in the panel that
 * did not line up with the fields above and below it. The menu it opens is
 * still the system's, which is the half worth keeping.
 */
const SELECT = "cursor-default appearance-none pr-7";

/** A control that grows: the height token becomes a floor rather than a size. */
const GROWS = "h-auto min-h-(--control-height-sm) py-1";

function Picker({
  value,
  label,
  onChange,
  children,
}: {
  value: string;
  label: string;
  onChange: (value: string) => void;
  children: React.ReactNode;
}) {
  return (
    <div className="relative">
      <select
        value={value}
        aria-label={label}
        onChange={(event) => onChange(event.target.value)}
        className={cn(FIELD, SELECT)}
      >
        {children}
      </select>
      <ChevronDown
        aria-hidden="true"
        className="pointer-events-none absolute top-1/2 right-2 size-3 -translate-y-1/2 text-fg-tertiary"
      />
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="text-xs font-semibold text-fg-tertiary">{children}</h3>
  );
}

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="min-w-0 flex-1 text-xs text-fg-tertiary">{label}</span>
      <span className="max-w-[65%] truncate text-right text-xs text-fg-secondary">
        {children}
      </span>
    </div>
  );
}

/**
 * A checkbox the system draws.
 *
 * `color-scheme` already follows the appearance, and a hand-drawn box would be
 * the interface imitating something the platform ships.
 */
function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="flex items-center gap-2">
      <input
        id={id}
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="size-3.5 accent-fg"
      />
      <label htmlFor={id} className="min-w-0 flex-1 text-xs text-fg-secondary">
        {label}
      </label>
    </div>
  );
}

/**
 * A set of short values, each one a thing rather than a substring.
 *
 * This is the token field macOS has had since `NSTokenField`, and it exists
 * because the alternative was tried here and does not work: a single box
 * holding `a, b, c` cannot be a controlled field over a parsed list. Every
 * keystroke re-parsed the text and handed back the normalised version, so the
 * comma that starts a second value was deleted as it was typed and a second
 * value could not be entered at all.
 *
 * What is typed lives here, and only a completed value reaches the record. A
 * value is completed by Return, by a comma, or by leaving the field — which is
 * also why nothing is lost by clicking away mid-word.
 */
function TokenField({
  label,
  values,
  placeholder,
  onChange,
}: {
  label: string;
  values: readonly string[];
  placeholder: string;
  onChange: (values: string[]) => void;
}) {
  const [typing, setTyping] = useState("");
  const field = useRef<HTMLInputElement>(null);

  const commit = (text: string) => {
    const parts = text
      .split(",")
      .map((part) => part.trim())
      .filter(Boolean);
    setTyping("");
    if (parts.length === 0) return;
    // A value already held is not added twice: a set is what this is, and two
    // identical tags say nothing the one says.
    const next = [...values];
    for (const part of parts) if (!next.includes(part)) next.push(part);
    if (next.length !== values.length) onChange(next);
  };

  return (
    // One field, with the values inside it. A row of tokens above a box would be
    // two controls describing one value, and the box would go on saying "Add a
    // tag" about something that already has four. This is the same border, the
    // same radius and the same minimum height as every other control in the
    // column; it grows down a line at a time as the values fill it.
    //
    // The ring is on the field rather than on the text cursor inside it, because
    // what the keyboard is in is the field.
    <div
      onMouseDown={(event) => {
        // Clicking the padding puts the caret in the field, the way clicking a
        // text box anywhere puts it in the text. A click on a token's own remove
        // button is not that, and says so by handling itself.
        if (event.target === event.currentTarget) {
          event.preventDefault();
          field.current?.focus();
        }
      }}
      className={cn(
        FIELD,
        GROWS,
        "flex cursor-text flex-wrap items-center gap-1 focus-within:outline-2 focus-within:outline-offset-1 focus-within:outline-focus",
      )}
    >
      {values.map((value) => (
        <span
          key={value}
          className="flex max-w-full shrink-0 items-center gap-0.5 rounded-(--radius-control) bg-selected py-px pr-0.5 pl-1.5 text-xs text-fg"
        >
          <span className="truncate">{value}</span>
          <button
            type="button"
            aria-label={`Remove ${value}`}
            onClick={() => onChange(values.filter((held) => held !== value))}
            className="shrink-0 rounded-(--radius-control) p-0.5 text-fg-tertiary hover:text-danger"
          >
            <X className="size-2.5" />
          </button>
        </span>
      ))}

      <input
        ref={field}
        value={typing}
        spellCheck={false}
        aria-label={label}
        // The prompt is for an empty field. With values in it the field is
        // already saying what it holds, and a placeholder beside them would be
        // instructions competing with content.
        placeholder={values.length === 0 ? placeholder : ""}
        onChange={(event) => setTyping(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === ",") {
            event.preventDefault();
            commit(typing);
            return;
          }
          // Backspace on an empty field takes back the last value, which is what
          // a token field on this system does.
          if (event.key === "Backspace" && typing === "" && values.length > 0) {
            event.preventDefault();
            onChange(values.slice(0, -1));
          }
        }}
        onBlur={() => commit(typing)}
        className="min-w-16 flex-1 bg-transparent text-xs text-fg outline-none placeholder:text-fg-tertiary"
      />
    </div>
  );
}

/**
 * The paths a claim is scoped to, or was written against.
 *
 * A path is not free text: it names a file in this repository, and the system
 * has a panel for choosing one. It opens at the project and what is stored is
 * relative to it, so a record scoped in one checkout means the same thing in
 * another. Typing is still offered beside it, because a claim may be scoped to
 * a file that does not exist yet — and because a path pasted from a terminal is
 * faster to paste than to find.
 *
 * Freshness is derived from these, which is why they are a list of exact paths
 * rather than a sentence: the engine matches them against what the code did.
 */
function PathField({
  title,
  paths,
  projectPath,
  empty,
  onChange,
}: {
  title: string;
  paths: readonly string[];
  projectPath: string;
  empty: string;
  onChange: (paths: string[]) => void;
}) {
  const [typing, setTyping] = useState("");

  const add = (added: readonly string[]) => {
    const next = [...paths];
    for (const path of added) {
      const trimmed = path.trim();
      if (trimmed !== "" && !next.includes(trimmed)) next.push(trimmed);
    }
    if (next.length !== paths.length) onChange(next);
  };

  return (
    <section className="space-y-2">
      <div className="flex items-center gap-1">
        <Label>{title}</Label>
        <div className="min-w-0 flex-1" />
        <button
          type="button"
          // The panel is the system's, so this says what it opens rather than
          // describing a gesture: no ellipsis, for the reason nothing in this
          // window carries one.
          onClick={() => {
            void chooseProjectFiles(projectPath).then(add);
          }}
          className="flex shrink-0 items-center gap-1 rounded-(--radius-control) px-1.5 py-0.5 text-xs text-fg-tertiary hover:bg-hover hover:text-fg"
        >
          <Plus className="size-3" />
          Choose
        </button>
      </div>

      {paths.length > 0 ? (
        <ul className="space-y-0.5">
          {paths.map((path) => (
            <li key={path} className="flex items-center gap-1">
              {/* A path is read from its end — the file — so it is the start
                  that is allowed to run out of the column. */}
              <span
                dir="rtl"
                title={path}
                className="min-w-0 flex-1 truncate text-left font-mono text-xs text-fg-secondary"
              >
                {path}
              </span>
              <button
                type="button"
                aria-label={`Remove ${path}`}
                onClick={() => onChange(paths.filter((held) => held !== path))}
                className="shrink-0 rounded-(--radius-control) text-fg-tertiary hover:text-danger"
              >
                <X className="size-3" />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs text-fg-tertiary">{empty}</p>
      )}

      <input
        value={typing}
        spellCheck={false}
        aria-label={`Add a path to ${title}`}
        placeholder="Or type a path"
        onChange={(event) => setTyping(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return;
          event.preventDefault();
          add([typing]);
          setTyping("");
        }}
        onBlur={() => {
          add([typing]);
          setTyping("");
        }}
        className={cn(FIELD, "font-mono")}
      />
    </section>
  );
}

/**
 * What this record links to, and under which relation.
 *
 * The engine validates every link against the relations the type declares and
 * rejects any other, so the relation is a pop-up over exactly those. A type that
 * declares none says so instead of offering a field the store would refuse.
 */
function Links({
  draft,
  type,
  types,
  onEdit,
  onWrite,
}: {
  draft: DocumentDraft;
  type: MemoryType | undefined;
  types: readonly MemoryType[];
  onEdit: (patch: DocumentPatch) => void;
  onWrite: () => void;
}) {
  const relations = Object.entries(type?.relationships ?? {});
  const links = draft.links;

  const replace = (index: number, key: string, relation: string) => {
    const next = links.map((link, at) =>
      at === index ? { key, relation } : link,
    );
    onEdit({ links: next });
  };

  // A link with no key is a link to nothing, and the store refuses it. It stays
  // on screen as the row somebody is filling in and is left out of the write,
  // so adding a link and thinking about it does not produce a refusal.
  const write = () => {
    const named = links.filter((link) => link.key.trim() !== "");
    if (named.length !== links.length) onEdit({ links: named });
    onWrite();
  };

  return (
    <section className="space-y-2">
      <Label>Links</Label>

      {links.map((link, index) => (
        <div key={index} className="space-y-1">
          <div className="flex items-center gap-1">
            <div className="min-w-0 flex-1">
              <Picker
                value={link.relation}
                label="Relation"
                onChange={(relation) => {
                  replace(index, link.key, relation);
                  if (link.key.trim() !== "") onWrite();
                }}
              >
                {/* The stored relation is offered even when the type no longer
                    declares it: the record says what it says, and a picker that
                    silently swapped it would rewrite a link nobody touched. */}
                {relations.some(([name]) => name === link.relation) ? null : (
                  <option value={link.relation}>{link.relation}</option>
                )}
                {relations.map(([name, declaration]) => (
                  <option key={name} value={name}>
                    {name}
                    {declaration.target && declaration.target !== "any"
                      ? ` → ${typeName(types, declaration.target)}`
                      : ""}
                  </option>
                ))}
              </Picker>
            </div>
            <button
              type="button"
              aria-label={`Remove the link to ${link.key || "nothing"}`}
              onClick={() => {
                onEdit({ links: links.filter((_, at) => at !== index) });
                onWrite();
              }}
              className="flex h-(--control-height-sm) shrink-0 items-center rounded-(--radius-control) px-1.5 text-xs text-fg-tertiary hover:bg-hover hover:text-danger"
            >
              Remove
            </button>
          </div>
          <input
            value={link.key}
            spellCheck={false}
            aria-label="Linked record"
            placeholder="Key"
            onChange={(event) =>
              replace(index, event.target.value, link.relation)
            }
            onBlur={write}
            className={cn(FIELD, "font-mono")}
          />
        </div>
      ))}

      {relations.length === 0 ? (
        <p className="text-xs text-fg-tertiary">
          {type
            ? `${type.title} declares no relations, so the store would refuse a link on it. Relations are part of a type's definition.`
            : "The project no longer holds this record's type, so nothing can say which links it may hold."}
        </p>
      ) : (
        <button
          type="button"
          onClick={() =>
            onEdit({
              links: [...links, { key: "", relation: relations[0][0] }],
            })
          }
          className="w-full rounded-(--radius-control) border border-separator-strong px-2 py-1 text-xs text-fg-secondary hover:bg-hover hover:text-fg"
        >
          Add link
        </button>
      )}
    </section>
  );
}

/**
 * The product fields the record's type declares.
 *
 * Every control is generated from the declaration, and nothing here knows what
 * any field means: `validation_state` and `horizon` are enumerations to this
 * file and nothing else. What it does know is what each *shape* is worth
 * offering, which is the whole of the difference between a form and a text box
 * with a label.
 *
 * A field the record carries and the type no longer declares is shown as stored
 * and never rewritten: the project said it once, and a window that dropped it
 * on the next save would hide part of what the project said.
 */
function Fields({
  draft,
  type,
  onEdit,
  onWrite,
}: {
  draft: DocumentDraft;
  type: MemoryType | undefined;
  onEdit: (patch: DocumentPatch) => void;
  onWrite: () => void;
}) {
  const declared = Object.entries(type?.fields ?? {});
  const undeclared = Object.keys(draft.fields).filter(
    (name) => !declared.some(([declaredName]) => declaredName === name),
  );
  if (declared.length === 0 && undeclared.length === 0) return null;

  const set = (name: string, value: unknown, immediate: boolean) => {
    onEdit({ fields: { [name]: value } });
    if (immediate) onWrite();
  };

  return (
    <section className="space-y-3">
      <Label>Fields</Label>

      {declared.map(([name, declaration]) => (
        <Field
          key={name}
          name={name}
          declaration={declaration}
          value={draft.fields[name]}
          onSet={set}
        />
      ))}

      {undeclared.map((name) => (
        <div key={name} className="space-y-1">
          <FieldName name={name} note="not declared by this type" />
          <p className="truncate text-xs text-fg-secondary">
            {asText(draft.fields[name])}
          </p>
        </div>
      ))}
    </section>
  );
}

function Field({
  name,
  declaration,
  value,
  onSet,
}: {
  name: string;
  declaration: FieldDeclaration;
  value: unknown;
  onSet: (name: string, value: unknown, immediate: boolean) => void;
}) {
  const required = declaration.required === true;
  const shape = declaration.type ?? (declaration.values ? "enum" : "string");

  if (declaration.values && declaration.values.length > 0) {
    const current = typeof value === "string" ? value : "";
    return (
      <div className="space-y-1">
        <FieldName name={name} note={declaration.description} />
        <Picker
          value={current}
          label={name}
          onChange={(chosen) => onSet(name, chosen === "" ? null : chosen, true)}
        >
          {/* An optional field can be nothing, and "nothing" has to be a choice
              a person can make rather than a value they have to guess at. */}
          {required ? null : <option value="">Not set</option>}
          {declaration.values.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
          {current !== "" && !declaration.values.includes(current) ? (
            <option value={current}>{current}</option>
          ) : null}
        </Picker>
      </div>
    );
  }

  if (shape === "boolean") {
    return (
      <Toggle
        label={name}
        checked={value === true}
        onChange={(checked) => onSet(name, checked, true)}
      />
    );
  }

  if (shape === "number" || shape === "integer") {
    return (
      <div className="space-y-1">
        <FieldName name={name} note={declaration.description} />
        <NumberField
          name={name}
          value={value}
          integer={shape === "integer"}
          required={required}
          onSet={onSet}
        />
      </div>
    );
  }

  if (shape === "array") {
    return (
      <ArrayField
        name={name}
        declaration={declaration}
        value={value}
        onSet={onSet}
      />
    );
  }

  // `text` is the schema's word for prose. A single line for something declared
  // as several is the interface ignoring what it was told.
  if (shape === "text") {
    const current = typeof value === "string" ? value : "";
    return (
      <div className="space-y-1">
        <FieldName name={name} note={declaration.description} />
        <textarea
          value={current}
          rows={Math.min(Math.max(current.split("\n").length, 2), 8)}
          spellCheck={false}
          aria-label={name}
          placeholder={required ? "Required" : "Not set"}
          onChange={(event) =>
            onSet(
              name,
              event.target.value === "" && !required ? null : event.target.value,
              false,
            )
          }
          onBlur={() => onSet(name, value, true)}
          className={cn(FIELD, GROWS, "resize-y leading-5")}
        />
      </div>
    );
  }

  if (shape !== "string") {
    // An object: shown as the store spells it, because a control for it is a
    // screen of its own and a wrong guess would rewrite the value.
    return (
      <div className="space-y-1">
        <FieldName name={name} note={`${shape}, shown as stored`} />
        <p className="truncate text-xs text-fg-secondary">{asText(value)}</p>
      </div>
    );
  }

  return (
    <div className="space-y-1">
      <FieldName name={name} note={declaration.description} />
      <input
        value={typeof value === "string" ? value : ""}
        spellCheck={false}
        aria-label={name}
        placeholder={required ? "Required" : "Not set"}
        onChange={(event) =>
          onSet(name, event.target.value === "" && !required ? null : event.target.value, false)
        }
        onBlur={() => onSet(name, value, true)}
        className={FIELD}
      />
    </div>
  );
}

/**
 * A number, typed the way numbers are typed.
 *
 * What is on screen is held here as text, because the halfway states of typing
 * a number are not numbers: `-` on its own, `1.` before the decimals, an empty
 * field. Parsing every keystroke and handing back the parsed value — which is
 * what this did — deleted the minus sign and the decimal point as they were
 * typed, so nothing negative and nothing fractional could be entered at all.
 *
 * The record is told only about text that is a number, and about an empty field
 * when the schema allows one.
 */
function NumberField({
  name,
  value,
  integer,
  required,
  onSet,
}: {
  name: string;
  value: unknown;
  integer: boolean;
  required: boolean;
  onSet: (name: string, value: unknown, immediate: boolean) => void;
}) {
  const stored = typeof value === "number" ? String(value) : "";
  const [typing, setTyping] = useState<string | null>(null);
  const shown = typing ?? stored;

  const report = (text: string, immediate: boolean) => {
    if (text.trim() === "") {
      if (!required) onSet(name, null, immediate);
      return;
    }
    const parsed = Number(text);
    if (!Number.isFinite(parsed)) return;
    if (integer && !Number.isInteger(parsed)) return;
    onSet(name, parsed, immediate);
  };

  return (
    <input
      type="text"
      inputMode={integer ? "numeric" : "decimal"}
      value={shown}
      spellCheck={false}
      aria-label={name}
      placeholder={required ? "0" : "Not set"}
      onChange={(event) => {
        setTyping(event.target.value);
        report(event.target.value, false);
      }}
      onBlur={() => {
        report(shown, true);
        // Back to what the record holds: a half-typed number that was never
        // reported would otherwise stay on screen as though it had been.
        setTyping(null);
      }}
      className={FIELD}
    />
  );
}

/**
 * A list of values, as the declaration says they are.
 *
 * An array of strings is a token field, for the reason tags are. An array over
 * an enumeration is that enumeration as checkboxes, because the values are
 * known and a list of known values is a set to tick rather than a string to
 * spell correctly. Anything else — an array of objects, which is what a
 * checklist is — is shown as stored: a control for it is a screen of its own,
 * and inventing one would rewrite what the project wrote.
 */
function ArrayField({
  name,
  declaration,
  value,
  onSet,
}: {
  name: string;
  declaration: FieldDeclaration;
  value: unknown;
  onSet: (name: string, value: unknown, immediate: boolean) => void;
}) {
  const items = declaration.items;
  const held = Array.isArray(value) ? value : [];
  const strings = held.filter((entry): entry is string => typeof entry === "string");
  const editable =
    items !== undefined &&
    (items.type === "string" || items.type === "text" || items.values !== undefined) &&
    strings.length === held.length;

  if (!editable) {
    return (
      <div className="space-y-1">
        <FieldName name={name} note={`list, shown as stored`} />
        <p className="truncate text-xs text-fg-secondary">{asText(value)}</p>
      </div>
    );
  }

  if (items.values && items.values.length > 0) {
    return (
      <div className="space-y-1">
        <FieldName name={name} note={declaration.description} />
        {items.values.map((option) => (
          <Toggle
            key={option}
            label={option}
            checked={strings.includes(option)}
            onChange={(checked) =>
              onSet(
                name,
                checked
                  ? [...strings, option]
                  : strings.filter((entry) => entry !== option),
                true,
              )
            }
          />
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-1">
      <FieldName name={name} note={declaration.description} />
      <TokenField
        label={name}
        values={strings}
        placeholder="Add a value"
        onChange={(next) => onSet(name, next, true)}
      />
    </div>
  );
}

/** A field is named as the store spells it. Nothing here invents a label. */
function FieldName({ name, note }: { name: string; note?: string }) {
  return (
    <p className="flex items-baseline gap-1.5">
      <span className="min-w-0 truncate font-mono text-xs text-fg-tertiary">
        {name}
      </span>
      {note ? (
        <span className="min-w-0 flex-1 truncate text-xs text-fg-tertiary">
          {note}
        </span>
      ) : null}
    </p>
  );
}

/** A value as one line, without pretending to know what it means. */
function asText(value: unknown): string {
  if (value === null || value === undefined) return "—";
  if (Array.isArray(value)) return `${value.length} items`;
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}
