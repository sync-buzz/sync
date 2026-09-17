"use client";

import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { FIELD, Segment, SegmentedToggle, Setting } from "@/components/settings/shared";
import type { SettingsSchema, SettingsField } from "@/lib/extension-host/settings";
import { cn } from "@/lib/utils";

/**
 * The settings sheet the host renders from the extension's schema.
 *
 * Opened by `open()` on the `SettingsHandle` the area holds. Portable fields
 * come first, local second, so a person who cares only about what travels
 * reads the top and leaves. Everything applies as it is chosen, like the rest
 * of the settings window: a form with an Apply button asks a person to confirm
 * something they can already see.
 */
export function ExtensionSettingsSheet({
  open,
  onOpenChange,
  schema,
  values,
  extensionName,
  onSet,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  schema: SettingsSchema;
  values: Readonly<Record<string, unknown>> | null;
  extensionName: string;
  onSet: (key: string, value: unknown) => void;
}) {
  const fields = Object.entries(schema.properties);
  const portable = fields.filter(([, field]) => field.portable);
  const local = fields.filter(([, field]) => !field.portable);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>{extensionName} settings</SheetTitle>
        </SheetHeader>
        <SheetDescription className="sr-only">
          Settings for {extensionName}. Changes apply immediately.
        </SheetDescription>
        <ScrollArea className="min-h-0 flex-1">
          <div className="flex flex-col gap-5 px-4 py-5">
            {portable.length > 0 ? (
              <>
                <p className="text-xs text-fg-tertiary">Travel with this project</p>
                <div className="flex flex-col gap-5">
                  {portable.map(([key, field]) => (
                    <FieldRow
                      key={key}
                      fieldKey={key}
                      field={field}
                      value={values?.[key]}
                      onSet={onSet}
                    />
                  ))}
                </div>
              </>
            ) : null}
            {local.length > 0 ? (
              <>
                <p className="text-xs text-fg-tertiary">This machine only</p>
                <div className="flex flex-col gap-5">
                  {local.map(([key, field]) => (
                    <FieldRow
                      key={key}
                      fieldKey={key}
                      field={field}
                      value={values?.[key]}
                      onSet={onSet}
                    />
                  ))}
                </div>
              </>
            ) : null}
          </div>
        </ScrollArea>
        <SheetFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function FieldRow({
  fieldKey,
  field,
  value,
  onSet,
}: {
  fieldKey: string;
  field: SettingsField;
  value: unknown;
  onSet: (key: string, value: unknown) => void;
}) {
  return (
    <Setting label={field.title} detail={field.description}>
      <FieldControl fieldKey={fieldKey} field={field} value={value} onSet={onSet} />
    </Setting>
  );
}

function FieldControl({
  fieldKey,
  field,
  value,
  onSet,
}: {
  fieldKey: string;
  field: SettingsField;
  value: unknown;
  onSet: (key: string, value: unknown) => void;
}) {
  if (field.type === "boolean") {
    return (
      <SegmentedToggle
        isOn={value === true}
        onChange={(wanted) => onSet(fieldKey, wanted)}
        label={field.title}
      />
    );
  }

  if (field.enum !== undefined && field.enum.length > 0) {
    if (field.enum.length <= 4) {
      return (
        <div role="radiogroup" aria-label={field.title} className="flex flex-wrap gap-1">
          {field.enum.map((option) => (
            <Segment
              key={option}
              label={option}
              isSelected={value === option}
              onSelect={() => onSet(fieldKey, option)}
            />
          ))}
        </div>
      );
    }
    return (
      <select
        aria-label={field.title}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onSet(fieldKey, event.target.value)}
        className={cn(FIELD, "w-full max-w-[42ch]")}
      >
        <option value="">Choose…</option>
        {field.enum.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }

  if (field.type === "integer" || field.type === "number") {
    return (
      <div className="flex items-center gap-2">
        <input
          type="number"
          aria-label={field.title}
          value={typeof value === "number" ? value : ""}
          min={field.minimum}
          max={field.maximum}
          step={field.type === "integer" ? 1 : 0.1}
          onChange={(event) => {
            const next = event.target.valueAsNumber;
            if (Number.isFinite(next)) {
              onSet(fieldKey, next);
            }
          }}
          className={cn(FIELD, "w-24")}
        />
        {field.minimum !== undefined && field.maximum !== undefined ? (
          <span className="text-xs text-fg-tertiary">
            {field.minimum}–{field.maximum}
          </span>
        ) : null}
      </div>
    );
  }

  return (
    <input
      type="text"
      aria-label={field.title}
      value={typeof value === "string" ? value : ""}
      onChange={(event) => onSet(fieldKey, event.target.value)}
      className={cn(FIELD, "w-full max-w-[42ch]")}
    />
  );
}
