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
| **Model** | An engine's downloaded weights, when it has any. The system engine has none, and says so. | checkpoint, weights |
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

## 3. Two engines behind one interface, and the system's goes first

```rust
trait Engine {
    fn voices(&self) -> Vec<Voice>;
    fn speak(&self, utterance: Utterance) -> Result<(), VoiceError>;
    fn stop(&self);
}
```

### 3.1 The system engine

`AVSpeechSynthesizer`, through `objc2-avf-audio` — `objc2`, `objc2-foundation`
and `objc2-app-kit` are already in the tree for the Dock menu, so this is one
crate and no new dependency graph.

Measured on this machine before the decision, because "the system has voices" is
not an argument until somebody counts them: **184 voices**, including `ru_RU`
Milena, and macOS downloads its own Enhanced and Premium voices in System
Settings without Sync being involved. The system plays the sound, so there is no
audio device to open, no sample format to negotiate and no `rodio` in the
dependency tree.

It goes first for a reason that has nothing to do with quality: it is the engine
that can be **heard the same day**, which is what tells us whether the shape in
§4 and §5 is right before anybody pays for an ONNX runtime.

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

### 3.2 The model engine

The second slot, and it is what the request meant by *choose which model*. A
neural voice — Kokoro, or Piper where a language needs it — run through ONNX,
with `rodio` for the output the system engine did not need.

**There is no model engine in this version, and no model to download.** What
exists is the shape that would hold one: `Engine` is a trait with one
implementation, and `engines()` answers with what this build and this platform
can actually offer rather than with a list somebody has to keep true. The point
of writing the second slot down is that its absence is a missing implementation
rather than a redesign — see §6.

### 3.3 A model arrives the way an extension does

**No new library, and this is the whole of why the download is cheap.**
`sync-extensions` already fetches a remote artefact, verifies its `sha256`,
unpacks it into a content-addressed directory under the app data directory and
resolves an id to whichever artefact serves it now (`store.rs`, `registry.rs`).
A model is that, minus the archive: fetch, hash, keep under `voices/<sha256>/`.

One difference is stated rather than glossed. An extension we publish carries a
**minisign** signature of ours; a third-party model does not and never will. So
the guarantee is the `sha256` in a catalogue **compiled into the build**, next
to the hosts that catalogue may name — the same rule the registry already
follows, where what can be reached is a property of the binary rather than of a
file somebody edited.

**A model is the machine's, never the project's.** It sits beside the extension
artefacts for the same reason they do: two projects wanting the same voice cost
one copy, and removing a project takes nothing away from another.

---

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

Three callers, one implementation, and each reaches it by the door already open
to them.

**The window.** Tauri commands — `voice_status`, `voice_choose`, `voice_speak`,
`voice_stop`. The settings page is their only caller today. `voice_status` is
one command rather than the two this first said, because the voices and the
choice are read together and neither means anything alone: a stored identifier
with no list to find it in is not a voice, it is a string.

**A package's screen.** A function on `@sync-buzz/extension-api`, behind the
`voice` capability. A card says *this extension can speak*, before it is
installed, which is the whole purpose of the capability list.

**A package's handler.** `voice.speak` joins `OFFERED` in `handlers.rs`, behind
the same capability, enforced **at the call** the way `work.agent` is: whether
JavaScript calls a function is not visible in a manifest, and the refusal has to
name what was missing so an author can catch it. *Unbuilt.*

**An agent.** *Built 2026-08-25*, and it turned out to be the door the first
real case needed. `sync_speak` is a tool on Sync's own MCP surface, beside
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
await voice.speak({ text: "The nightly review is done.", interrupt: false })
```

**What is asked for, and what is not.** `text` and nothing else is required. A
caller may name a `voice`, and one that does not gets the person's own choice —
which is the case that matters: a package should not have to know what is
installed on this Mac, and a person who chose a voice chose it for everything.
`interrupt` decides between the two things a second sentence can mean while one
is still being said.

**One queue for the machine.** Utterances are said in the order they were asked
for; `interrupt: true` clears what is waiting and stops what is speaking.
`stop` is the same without a sentence to follow it. There is no per-caller queue
and no priority — a product where two packages compete for the speakers is a
product where the important sentence is the one that happened to be second.

**A refusal is a sentence, not silence.** No engine, no voice for that language,
a model that is not downloaded — each answers with a reason. Speech that quietly
does not happen is indistinguishable from speech nobody heard.

---

## 6. Deliberately absent

Named so they are not re-proposed.

- **A Voice extension.** §1. The package would own nothing; the settings window
  owns the choice and the crate owns the mechanism.
- **A model engine.** §3.2. The system's synthesiser is the only one, and a
  neural voice run through ONNX is a second implementation of `Engine` that
  nobody has written. Nothing about the first one assumes it is alone.
- **One extension handing another text to speak.** A package asks the host, and
  the host is the only speaker. Nothing here needs a package-to-package path.
- **Speech as a notification channel.** A banner is gated by three conditions
  (`extensions.md` §9a) and a sentence said aloud in an open-plan office is
  louder than a banner, not quieter. Whoever asks for a sentence is responsible
  for having a reason; the host does not start speaking on its own.
- **Reading a record aloud from the window.** A control on a record that reads
  it out is a feature of the reader, not of the speaker, and it is a separate
  decision about the record page. The mechanism does not prejudge it.
- **Listening.** §2.
- **A volume control.** §4.
- **Per-project voices.** §4.
- ~~**Speaking as an agent tool over MCP.**~~ **Built 2026-08-25**, on the
  owner's decision, and §5 is what it became. What this line was guarding
  against is answered by the switch rather than by the absence: an agent that
  can make the machine talk at three in the morning is one somebody switched on,
  and the same page switches it off.

---

## 7. The order it gets built in

Each step is usable before the next exists, and each is refutable on its own.

1. **The crate and the system engine.** *Built.* `sync-voice` with the `Engine`
   trait, the macOS implementation, `voice.json` in the configuration directory
   beside `mcp-server.json`, and four commands — `voice_status`, `voice_choose`,
   `voice_speak`, `voice_stop`. Five was the estimate; reading the preference
   and reading the voices are one question a page asks once, so `voice_status`
   answers both, the way `server_status` does.

   **Heard, not just tested**, which for this step is the only proof there is:
   `cargo test -p sync-voice -- --ignored` speaks a Russian sentence in Milena
   and the machine said it. That settled the one thing the design could not
   settle by argument — whether `AVSpeechSynthesizer` speaks from a thread that
   is not the main one and has no run loop. It does, which is what lets the
   engine own a thread of its own and be reachable from the clock's.
2. **The settings page.** *Built.* The fifth section, three controls and the
   try field.
3. **The capability, and the agent's tool.** *Half built.* The agent's half is
   `sync_speak` (§5), which is what the first real case — a routine that reads a
   mailbox on a clock — actually needed: the package orders the work and the
   agent, which is the only one of the three that knows whether there was
   anything worth saying, decides whether to say it. The package's own half is
   `voice` in `SYNC_CAPABILITIES`, `voice.speak` in
   `OFFERED` and on the service surface, the surface bumped to **2.7.0** — a
   minor, an addition, and every package stating `^2.0` goes on installing. A
   capability arrives with the machinery that honours it, which is the rule
   steps 2 and 3 of `background.md` were both held to.
4. **The model engine.** The catalogue, the download over the existing store,
   the ONNX implementation, and the engine control on the page gaining its
   second entry.

---

## 8. Open

1. **Which model, and which catalogue.** Kokoro has the best voices per
   megabyte and no Russian; Piper has Russian and more files per voice. The
   answer is a list in the build, and it is chosen when step 4 starts rather
   than now.
2. ~~**Whether an agent may speak.**~~ **Answered 2026-08-25: yes, behind a
   switch that starts off.** §5.
3. **What happens to a queue when the application quits.** Today's answer is
   that it stops, which is the only one that does not surprise anybody.
4. **Whether a sentence is ever repeated.** Nothing here re-says anything, and
   whether a person who missed it can ask for it again is a window question with
   no window to ask it in.
