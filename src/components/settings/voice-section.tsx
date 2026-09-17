"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { Button } from "@/components/ui/button";
import { FIELD, messageOf, Segment, SegmentedToggle, Setting } from "@/components/settings/shared";
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
        if (live) setFailure(messageOf(error, "Sync could not reach a voice engine."));
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
      .then(setStatus, (error: unknown) =>
        setFailure(messageOf(error, "Sync could not reach a voice engine.")),
      )
      .finally(() => setBusy(false));
  }, []);

  const say = useCallback(() => {
    setFailure(null);
    void speak(sentence, true).catch((error: unknown) =>
      setFailure(messageOf(error, "Sync could not reach a voice engine.")),
    );
  }, [sentence]);

  const settings = status?.settings;
  const grouped = useMemo(() => byLanguage(status?.voices ?? []), [status?.voices]);
  const chosen = settings?.voice ?? null;
  const stranded = useMemo(
    () => (chosen && !(status?.voices ?? []).some((voice) => voice.id === chosen) ? chosen : null),
    [chosen, status?.voices],
  );

  return (
    <section className="flex flex-col gap-5">
      <Setting
        label="Engine"
        detail="What turns the words into sound. The system's own synthesiser uses the voices macOS has, including the ones it downloads in System Settings."
      >
        <div role="radiogroup" aria-label="Engine" className="flex gap-1">
          {(status?.engines ?? []).map((engine) => (
            <Segment
              key={engine.id}
              label={engine.label}
              isSelected={settings?.engine === engine.id}
              disabled={engine.absent !== null || busy}
              title={engine.absent ?? undefined}
              onSelect={() => settings && choose({ ...settings, engine: engine.id })}
            />
          ))}
        </div>
      </Setting>

      <Setting
        label="Voice"
        detail="The voices macOS reads text in, grouped by language. Enhanced and Premium ones are the downloads it offers in System Settings; the rest ship with it."
      >
        <select
          aria-label="Voice"
          disabled={busy || grouped.length === 0}
          value={settings?.voice ?? ""}
          onChange={(event) =>
            settings && choose({ ...settings, voice: event.target.value || null })
          }
          className={cn(FIELD, "w-full max-w-[42ch]")}
        >
          <option value="">The system&apos;s own choice</option>
          {stranded ? <option value={stranded}>{stranded} — no longer offered</option> : null}
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
        {status?.failure ? <p className="text-sm text-warning">{status.failure}</p> : null}
      </Setting>

      <Setting label="Rate" detail="A multiplier over the engine's normal speed. One is normal.">
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
      </Setting>

      <Setting
        label="Agents"
        detail="An agent connected to Sync can say a sentence out loud — that a long job finished, or that something it was watching happened. It decides when; this decides whether."
      >
        <SegmentedToggle
          isOn={settings?.agents === true}
          disabled={busy || !settings}
          onChange={(wanted) => settings && choose({ ...settings, agents: wanted })}
          label="Agents"
        />
        <p className="max-w-[64ch] text-xs text-fg-tertiary">
          Off, an agent has no way to speak at all — Sync does not offer it one.
        </p>
      </Setting>

      <Setting label="Try it" detail="A voice cannot be chosen from a name. Say something in it.">
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
          <Button variant="outline" size="sm" disabled={sentence.trim().length === 0} onClick={say}>
            Speak
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void stopSpeaking()}>
            Stop
          </Button>
        </div>
        {failure ? <p className="text-sm text-warning">{failure}</p> : null}
      </Setting>
    </section>
  );
}

/**
 * The voices, in the order somebody looks for one.
 *
 * Two rules, and both are about a list with one voice for every language macOS
 * supports — forty-nine of them on the machine this was written on. Languages
 * are ordered by name, except this Mac's own, which goes first: the voice
 * somebody wants is overwhelmingly one that speaks their language. Within a
 * language the downloaded voices lead, because a Premium voice beside a compact
 * one of the same name is the whole reason quality is shown at all.
 */
function byLanguage(voices: readonly Voice[]): readonly (readonly [string, readonly Voice[]])[] {
  const held = new Map<string, Voice[]>();
  for (const voice of voices) {
    const group = held.get(voice.language) ?? [];
    group.push(voice);
    held.set(voice.language, group);
  }

  const mine = typeof navigator === "undefined" ? "" : navigator.language;
  const first = (tag: string) => (tag === mine || tag.split("-")[0] === mine.split("-")[0] ? 0 : 1);
  const rank = { premium: 0, enhanced: 1, standard: 2 } as const;

  return [...held.entries()]
    .map(
      ([language, group]) =>
        [
          language,
          [...group].sort(
            (one, other) =>
              rank[one.quality] - rank[other.quality] || one.name.localeCompare(other.name),
          ),
        ] as const,
    )
    .sort(
      ([one], [other]) =>
        first(one) - first(other) || languageNamed(one).localeCompare(languageNamed(other)),
    );
}
