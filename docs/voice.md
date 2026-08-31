# Voice

What the application says out loud, who may ask it to, and where a person
chooses the voice.

*This describes what this version does: the crate, the system engine, the
settings page and `sync_speak`. §6 names what it deliberately does not.*

Read [`extensions.md`](extensions.md) for what a package is and
[`background.md`](background.md) for the half of an extension with no screen.
This document is about a capability of the **application**, which is the one
thing those two do not cover — and §1 is why it cannot be anything else.

---

## 1. Speech is the application's, not a package's

The request this began as was an extension called Voice: other extensions hand
it text, it speaks. The receiving half would be a package accepting a request
from another package, which nothing in this version does. The speaking half
cannot be a package at all, and that is a measured fact rather than a
preference.

A service module runs in a QuickJS isolate. `background.md` §3.2 states what is
in it, having run the probe: no `WebAssembly`, no `fetch`, no `setTimeout`, no
`TextDecoder`. No audio device, no filesystem, no thread that outlives the call
— a handler is gone in milliseconds and a sentence takes seconds to say. Every
part of speaking is therefore Rust's, and the only question a package could
still answer is *which voice* and *what to do with a queue*.

So a Voice extension would be a screen over a mechanism it does not own, in a
window that already has the place for exactly that: **the settings window**.
What it holds is true of the installation and of every project opened in it —
which speakers, which voice, which model on this disk — and that sentence is
`design-foundation.md`'s own test for what settings are.

**The shape is the one Chat already stands on.** ACP lives in
`crates/acp-client`, in the application, and Chat is a package that drives it
through a capability. Speech lives in `crates/sync-voice`, in the application,
and any package may drive it through a capability. What differs is that talking
to an agent needs a screen and speaking does not, so this one has no package at
the front of it.

**Consequence, stated rather than left implicit:** `background.md` step 6 —
extension to extension — is *not* built by this work and is not needed by it.
A package that wants a sentence said asks the host, exactly as it asks for a
record. There is no `speech.speak` request kind to resolve, no `accepts` in a
manifest, and the second package that would have justified that machinery has
not arrived after all.

---

## 2. The vocabulary

One word for one mechanism, in every place it appears: the settings section is
**Voice**, the crate is `sync-voice`, the file is `voice.json`, the capability
is `voice` and the functions are `voice.*`.

| Term | What it means | Not to be called |
| --- | --- | --- |
| **Engine** | What turns text into sound: the system's synthesiser, or a model on this disk. | backend, provider, driver |
| **Voice** | One speaker an engine offers, with a language and a name a person recognises. | speaker, persona |
| **Utterance** | One request to say one piece of text. | message, job |

*Voice* is the word in the window because it is what a person is choosing — the
voice that will speak. *Engine* is the word for what produces it, and the two
are separate controls in §4 because the same person changes one far more often
than the other.

**Only speaking.** Nothing here listens. If dictation ever arrives it belongs in
this section, which is why the section is called Voice rather than Speech — a
name that already covers both halves does not have to be renamed to gain the
second one. Until then the page says one thing.

---

## 3. One interface, and the system's engine behind it

```rust
trait Engine {
    fn voices(&self) -> Vec<Voice>;
    fn speak(&self, utterance: Utterance) -> Result<(), VoiceError>;
    fn stop(&self);
}
```

`AVSpeechSynthesizer`, through `objc2-avf-audio` — `objc2`, `objc2-foundation`
and `objc2-app-kit` are already in the tree for the Dock menu, so this is one
crate and no new dependency graph.

Measured on this machine before the decision, because "the system has voices" is
not an argument until somebody counts them: **184 voices**, including `ru_RU`
Milena, and macOS downloads its own Enhanced and Premium voices in System
Settings without Sync being involved. The system plays the sound, so there is no
audio device to open, no sample format to negotiate and no `rodio` in the
dependency tree.

It is the platform's own, so there is nothing to download, nothing to choose
before a person can hear anything, and no audio stack of ours to get wrong. That
`Engine` is a trait with one implementation is deliberate: what a build offers
is answered by `engines()` rather than by a list somebody has to keep true.

**It owns a thread, and that replaces what this section first said.** The design
assumed speaking had to hop to the main thread through `app.run_on_main_thread`,
which would have made the crate know it was inside Tauri. It does not: measured
2026-08-25, `AVSpeechSynthesizer` speaks from an ordinary thread with no run
loop, so the engine keeps a thread of its own for the life of the process and
every caller reaches it over a channel.

The thread is not a convenience, it is the requirement. `speakUtterance` queues
and returns, so the synthesiser must outlive the call that asked — a sentence
takes seconds and the caller is gone in milliseconds. It cannot be a `static`
either: `Retained` is not `Send`, and the three callers are three different
threads (the window's, the clock's, and a handler's blocking one). One thread
owning the object answers all of that, and it means nothing here asserts
anything about `AVFoundation`'s own thread safety: the object is made on that
thread and never leaves it.

## 4. Where a person chooses, and what changes in the window

The settings window gains a **fifth section**, after `Agents`:

```
Appearance
Text
Server
Agents
Voice          ← new
```

`sections.ts` states the rule the addition has to pass — *a section is one with
a screen behind it* — and this one has a screen. The headline under the name, in
the window's own voice: **"What Sync says out loud, and in whose voice."**

What is on the page, top to bottom:

| Control | What it is | Why it is a control at all |
| --- | --- | --- |
| **Engine** | `System`, and whatever else this build and platform offer | An engine differs from another in what it costs and what it can say, not in taste |
| **Voice** | The voices *that engine* offers, grouped by language | The one thing a person actually wants to change |
| **Rate** | How fast, as a plain number | Every synthesiser is too slow or too fast for somebody |
| **Try it** | A field with a sentence in it, and a button that says it | A voice cannot be chosen from a name |

Four, and the fourth is the one this page would be useless without: `Milena`,
`Yuri` and `Katya` are three names, and nobody knows which of them they want
until they have heard one. The field starts with a sentence in the window's
language and is editable, because the sentence somebody wants to test is usually
their own.

**Volume is not here.** The system has a volume control and this application is
not going to grow a second one; a sentence Sync says is as loud as everything
else the Mac says.

**Nothing on this page is per project.** That is what makes it a settings
window's page and not an area's, and it is the same test that keeps extensions
out of this window: a voice belongs to these speakers, not to a repository that
travels to a colleague with a different set.

---

## 5. Who may speak, and how they ask

Two callers, one implementation, and each reaches it by the door already open
to them. A package's screen is not one of them — §6.

**The window.** Tauri commands — `voice_status`, `voice_choose`, `voice_speak`,
`voice_stop`. The settings page is their only caller today. `voice_status` is
one command rather than the two this first said, because the voices and the
choice are read together and neither means anything alone: a stored identifier
with no list to find it in is not a voice, it is a string.

**An agent.** The door the first real case needed. `sync_speak` is a tool on Sync's own MCP surface, beside
`sync_apply`: one string to say, an optional `interrupt`, and no `project` —
speakers belong to a machine rather than to a repository, so it is the second
tool after `sync_projects` that takes none.

Three things about it are decisions rather than details:

- **It lives in the sidecar because the channel runs one way.** `host.rs` is the
  *window's* channel — the window calls `sync-mcp`, and there is no route back.
  An agent is connected to that process, so the sentence has to be said there,
  which meant linking `sync-voice` into it. The consolation is that the sidecar
  outlives every window, so an agent can speak with nothing open.
- **Off until somebody says otherwise, and off means absent.** A package that
  can speak was installed from a card that said so — the consent rule
  `background.md` §4.1 settled for the clock. Nobody agreed to anything when
  they connected an agent, so `Preference::agents` starts `false`, and while it
  is false the tool is **left out of the catalogue** rather than published and
  refusing. A tool a model can see is a turn it will spend.
- **The refusal is still written**, for the model calling a name it remembers
  from a catalogue read before the switch moved, and it says where the switch is
  so that trying again is not the next thing it does.

```ts
await speak("The nightly review is done.", false)
```

**What is asked for, and what is not.** `text` and nothing else is required.
Neither caller names a voice: what is said is said in the person's own choice,
which is the case that matters — a caller should not have to know what is
installed on this Mac, and somebody who chose a voice chose it for everything.
`interrupt` decides between the two things a second sentence can mean while one
is still being said, and it is the only other member either door takes.

**One queue for the machine.** Utterances are said in the order they were asked
for; `interrupt: true` clears what is waiting and stops what is speaking.
`stop` is the same without a sentence to follow it. There is no per-caller queue
and no priority — a product where two packages compete for the speakers is a
product where the important sentence is the one that happened to be second.

**A refusal is a sentence, not silence.** No engine on this platform, no voice
for that language, a voice that has been removed — each answers with a reason. Speech that quietly
does not happen is indistinguishable from speech nobody heard.

---

## 6. Deliberately absent

Named so they are not re-proposed.

- **A Voice extension.** §1. The package would own nothing; the settings window
  owns the choice and the crate owns the mechanism.
- **A second engine.** The system's synthesiser is the only one there is.
  Nothing about it assumes it is alone — `Engine` is a trait and `engines()`
  answers with what this build and platform actually offer — but a build with
  one implementation is what this is.
- **One extension handing another text to speak.** The host is the only
  speaker, and no package reaches it, so there is not even a first half for a
  package-to-package path to be the second of.
- **Speech as a notification channel.** A banner is gated by three conditions
  (`extensions.md` §9a) and a sentence said aloud in an open-plan office is
  louder than a banner, not quieter. Whoever asks for a sentence is responsible
  for having a reason; the host does not start speaking on its own.
- **Reading a record aloud from the window.** A control on a record that reads
  it out is a feature of the reader, not of the speaker, and it is a separate
  decision about the record page. The mechanism does not prejudge it.
- **A package speaking at all**, from a screen or from a handler. The surface
  publishes no `voice` function and `SYNC_CAPABILITIES` names no `voice`, so
  there is nothing a card could say and nothing a manifest could ask for. What
  the shape would be is settled — a function behind a capability, so a card says
  *this extension can speak* before it is installed — and the extension that
  wants it has not arrived. A handler is the harder half of the two: it runs
  with no screen mounted, so the capability would have to be enforced at the
  call rather than read off the manifest.
- **Listening.** §2.
- **A volume control.** §4.
- **Per-project voices.** §4.
An agent speaking is **not** on this list, and was nearly put there. What the
absence would have guarded against — a machine that talks at three in the
morning — is answered better by the switch in §5, which starts off: an agent
that can speak is one somebody turned on, and the same page turns it off.

---
