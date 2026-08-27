"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  chooseVoice,
  FASTEST,
  languageNamed,
  loadVoice,
  SLOWEST,
  speak,
  stopSpeaking,
  type Voice,
  type VoiceSettings,
  type VoiceStatus,
} from "@/lib/settings/voice";
import { cn } from "@/lib/utils";

/**
 * What Sync says out loud, and in whose voice.
 *
 * Three choices about sound, one about permission, and a way to hear the
 * result. The engine is what turns text into
 * sound — today the system's own synthesiser, and later a model on this disk.
 * The voice is the one thing anybody actually comes here to change. The rate is
 * a multiplier over normal speech, because every synthesiser is too slow or too
 * fast for somebody.
 *
 * **The way to hear it is not a control, and the page would be useless without
 * it.** `Milena`, `Daniel` and `Yuri` are three names, and nobody knows which one they
 * want until they have heard it. So there is a sentence and a button that says
 * it, and the sentence is editable because the one somebody wants to test is
 * usually their own.
 *
 * Everything applies as it is chosen, like the appearance beside it: a settings
 * window with an Apply button asks a person to confirm something they can
 * already hear.
 *
 * There is no volume. The system has one, and a second one here would be an
 * application deciding it is louder than everything else on the Mac.
 *
 * **`Agents` is not about how Sync speaks but about who may ask it to**, and it
 * is the only choice here that starts off. Installing a package that can
 * speak was agreeing to it — the card says so, by the rule the clock's switch
 * already follows. Nobody agreed to anything when they connected an agent, so
 * that agreement is taken here or not at all.
 */
export function VoiceSection() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [sentence, setSentence] = useState("Sync speaks like this.");
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void loadVoice().then(
      (answer) => {
        if (live) setStatus(answer);
      },
      (error: unknown) => {
        if (live) setFailure(messageOf(error));
      },
    );
    return () => {
      live = false;
    };
  }, []);

  const choose = useCallback((settings: VoiceSettings) => {
    setBusy(true);
    setFailure(null);
    void chooseVoice(settings)
      .then(setStatus, (error: unknown) => setFailure(messageOf(error)))
      .finally(() => setBusy(false));
  }, []);

  const say = useCallback(() => {
    setFailure(null);
    // Interrupting, because pressing it twice means "say it again", not "say it
    // twice" — the second press is somebody who did not hear the first.
    void speak(sentence, true).catch((error: unknown) =>
      setFailure(messageOf(error)),
    );
  }, [sentence]);

  const settings = status?.settings;
  const grouped = useMemo(
    () => byLanguage(status?.voices ?? []),
    [status?.voices],
  );

  return (
    <section className="flex flex-col gap-5">
      <Choice
        label="Engine"
        detail="What turns the words into sound. The system's own synthesiser uses the voices macOS has, including the ones it downloads in System Settings."
      >
        <div role="radiogroup" aria-label="Engine" className="flex gap-1">
          {(status?.engines ?? []).map((engine) => (
            <button
              key={engine.id}
              type="button"
              role="radio"
              aria-checked={settings?.engine === engine.id}
              disabled={engine.absent !== null || busy}
              title={engine.absent ?? undefined}
              onClick={() =>
                settings && choose({ ...settings, engine: engine.id })
              }
              className={cn(
                "flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2.5 text-sm transition-colors duration-(--motion-duration-fast) ease-shell disabled:opacity-50",
                settings?.engine === engine.id
                  ? "border-separator-strong bg-selected font-medium text-fg"
                  : "text-fg-secondary hover:bg-hover hover:text-fg",
              )}
            >
              {settings?.engine === engine.id ? (
                <Check aria-hidden="true" className="size-3 shrink-0" />
              ) : null}
              {engine.label}
            </button>
          ))}
        </div>
      </Choice>

      <Choice
        label="Voice"
        detail="Grouped by language. Enhanced and Premium voices are the ones macOS downloads; the rest ship with it."
      >
        <select
          aria-label="Voice"
          disabled={busy || grouped.length === 0}
          value={settings?.voice ?? ""}
          onChange={(event) =>
            settings &&
            choose({ ...settings, voice: event.target.value || null })
          }
          className={cn(FIELD, "w-full max-w-[42ch]")}
        >
          {/* Not choosing is a real answer, and it is the one somebody who has
              never opened this page already has. */}
          <option value="">The system&apos;s own choice</option>
          {grouped.map(([language, voices]) => (
            <optgroup key={language} label={languageNamed(language)}>
              {voices.map((voice) => (
                <option key={voice.id} value={voice.id}>
                  {voice.name}
                  {voice.quality === "standard" ? "" : ` — ${voice.quality}`}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
        {status?.failure ? (
          <p className="text-sm text-warning">{status.failure}</p>
        ) : null}
      </Choice>

      <Choice
        label="Rate"
        detail="A multiplier over the engine's normal speed. One is normal."
      >
        <div className="flex items-center gap-2">
          <input
            type="number"
            aria-label="Rate"
            value={settings?.rate ?? 1}
            min={SLOWEST}
            max={FASTEST}
            step={0.1}
            disabled={busy || !settings}
            onChange={(event) => {
              const next = event.target.valueAsNumber;
              if (settings && Number.isFinite(next)) {
                choose({
                  ...settings,
                  rate: Math.min(FASTEST, Math.max(SLOWEST, next)),
                });
              }
            }}
            className={cn(FIELD, "w-24")}
          />
          <span className="text-sm text-fg-tertiary">×</span>
          <span className="text-xs text-fg-tertiary">
            {SLOWEST}–{FASTEST}
          </span>
        </div>
      </Choice>

      <Choice
        label="Agents"
        detail="An agent connected to Sync can say a sentence out loud — that a long job finished, or that something it was watching happened. It decides when; this decides whether."
      >
        <div role="radiogroup" aria-label="Agents" className="flex gap-1">
          {[
            { label: "Off", wanted: false },
            { label: "On", wanted: true },
          ].map((option) => (
            <button
              key={option.label}
              type="button"
              role="radio"
              aria-checked={settings?.agents === option.wanted}
              disabled={busy || !settings}
              onClick={() =>
                settings && choose({ ...settings, agents: option.wanted })
              }
              className={cn(
                "flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2.5 text-sm transition-colors duration-(--motion-duration-fast) ease-shell",
                settings?.agents === option.wanted
                  ? "border-separator-strong bg-selected font-medium text-fg"
                  : "text-fg-secondary hover:bg-hover hover:text-fg",
              )}
            >
              {settings?.agents === option.wanted ? (
                <Check aria-hidden="true" className="size-3 shrink-0" />
              ) : null}
              {option.label}
            </button>
          ))}
        </div>
        {/* Said where the switch is, because the consequence is invisible: the
            tool leaves the agent's catalogue entirely rather than staying in it
            and refusing, so an agent cannot tell it was ever there. */}
        <p className="max-w-[64ch] text-xs text-fg-tertiary">
          Off, an agent has no way to speak at all — Sync does not offer it one.
        </p>
      </Choice>

      <Choice
        label="Try it"
        detail="A voice cannot be chosen from a name. Say something in it."
      >
        <div className="flex flex-wrap items-center gap-2">
          <input
            aria-label="What to say"
            value={sentence}
            onChange={(event) => setSentence(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") say();
            }}
            className={cn(FIELD, "w-full max-w-[42ch] flex-1")}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={sentence.trim().length === 0}
            onClick={say}
          >
            Speak
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void stopSpeaking()}>
            Stop
          </Button>
        </div>
        {/* A command that did not happen says so, beside the control that was
            pressed rather than somewhere else in the window. */}
        {failure ? <p className="text-sm text-warning">{failure}</p> : null}
      </Choice>
    </section>
  );
}

/**
 * The voices, in the order somebody looks for one.
 *
 * Two rules, and both are about a list of nearly two hundred. Languages are
 * ordered by name, except this Mac's own, which goes first — the voice somebody
 * wants is overwhelmingly one that speaks their language. Within a language the
 * downloaded voices lead, because a Premium voice beside a compact one of the
 * same name is the whole reason quality is shown at all.
 */
function byLanguage(
  voices: readonly Voice[],
): readonly (readonly [string, readonly Voice[]])[] {
  const held = new Map<string, Voice[]>();
  for (const voice of voices) {
    const group = held.get(voice.language) ?? [];
    group.push(voice);
    held.set(voice.language, group);
  }

  const mine = typeof navigator === "undefined" ? "" : navigator.language;
  const first = (tag: string) =>
    tag === mine || tag.split("-")[0] === mine.split("-")[0] ? 0 : 1;
  const rank = { premium: 0, enhanced: 1, standard: 2 } as const;

  return [...held.entries()]
    .map(
      ([language, group]) =>
        [
          language,
          [...group].sort(
            (one, other) =>
              rank[one.quality] - rank[other.quality] ||
              one.name.localeCompare(other.name),
          ),
        ] as const,
    )
    .sort(
      ([one], [other]) =>
        first(one) - first(other) ||
        languageNamed(one).localeCompare(languageNamed(other)),
    );
}

function Choice({
  label,
  detail,
  children,
}: {
  label: string;
  detail: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="space-y-0.5">
        <h2 className="text-base font-medium text-fg">{label}</h2>
        <p className="max-w-[64ch] text-sm text-fg-tertiary">{detail}</p>
      </div>
      {children}
    </div>
  );
}

/** The height every control in this window shares. */
const FIELD =
  "h-(--control-height-lg) rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-sm text-fg";

function messageOf(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return typeof error === "string"
    ? error
    : "Sync could not reach a voice engine.";
}
