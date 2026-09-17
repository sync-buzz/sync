# The shell that knows nothing

*A desktop app where every feature is a package the application did not build —
what that cost us, and what it bought.*

---

There is a directory that does not exist in our repository, and a job in CI
whose only purpose is to keep it from existing.

It would be called `src/extensions/`. Every plugin system I have worked on had
one: the extensions we wrote, compiled into the application, sitting beside the
loader that loads everybody else's. It is such a reasonable place to put them.
It is also the thing that quietly guarantees your extension system only ever
works for extensions you wrote.

So we deleted it before we had one. Every feature of our app — the record
browser, the agent conversations, the vocabulary a project reasons in — is a
signed zip file built in a different repository, downloaded, verified, unpacked
and loaded at runtime. The shell that hosts them names no language, no file
type, no section and no package. It cannot: there is nowhere left to write a
name down.

This is what that actually looks like, and the four things it cost.

---

## The rule, stated as a property of the code

> **The core cannot name an extension.** Not in a constant, not in a type, not
> in a conditional.

That sentence is easy to agree with and hard to hold, because breaking it is
never a decision anybody makes. It happens like this: a helper gets lifted into
`lib/` because two sections wanted it. An `if (id === "chat")` appears in a
loader. A type union in the shell happens to list exactly the sections this
build contains. Each of those is a reasonable afternoon's work, and together
they are the thing the design existed to prevent.

The only defence that survives contact with a real team is one where the
alternative does not compile. So the union of section identifiers became
`string`. The constants naming every built-in area were deleted rather than
emptied. What survived that deletion is a lookup over data — a manifest
declared an area, a module returned it, the window drew a row it had never heard
of. What did not survive was the leak.

Two places in the shell still named a package when we started: one held *which
section opens the project's own types*, the other *which section needs something
from npm before it works*. Both became manifest fields. Neither was a special
case once it was written down by the package that had the property.

---

## What a package is

A zip, and a schema everybody reads.

```
manifest.json           id, version, the API range, capabilities, areas, types
types/*.json            the vocabulary it publishes into the project
ui/index.js             the section — React, built against a published contract
ui/index.css            the rules its own markup uses
service/index.js        the handlers, for what happens with no window open
prompt/instructions.md  what a connected agent is told
META/hashes.json        path -> sha256, for every file above
META/signature          minisign over the canonical hashes file
```

Four different programs read that manifest — the packer, CI, the Rust loader and
the window — so it is JSON with one schema rather than TOML with a second parser
and a specification written in prose.

Everything else follows from one number. The application has a version, and it
is the wrong number for extensions: a release can redraw every panel without
moving anything a package can see, and a patch release can remove an export. So
the surface carries a version of its own, on its own clock, and a manifest
states a range against it.

The interesting part is not the number. It is that **nobody verifies a promise
they only wrote down.** The surface is extracted by a tool into a committed
report, and CI fails on two different things: the surface moved and the report
did not, *and* the report moved and the version did not. The second is the quiet
one. That commit has a report matching its own build perfectly, and every
package that stated a range goes on believing a number that no longer describes
anything.

---

## Semver cannot answer the question people actually ask

*Is this surface compatible* is a version question. *Can this build do the
thing* is not.

A platform without a bundled agent sidecar publishes exactly the same types and
cannot raise an agent behind them. A build with nowhere to keep a secret
publishes exactly the same keychain interface and refuses every call. Expressing
that as a version number means a different version per platform, which is a lie
in the other direction.

So a build publishes named capabilities and a manifest asks for the ones it
needs. Thirteen of them today. Four are about code running with no screen
mounted, and they are four separate agreements because they are four different
questions:

| | What a person is agreeing to |
| --- | --- |
| `background` | this package runs code with no screen mounted |
| `schedule` | it runs while nobody is there |
| `work.agent` | it may raise an agent, which **spends money while they sleep** |
| `agent.tools` | an agent is told it is there, and may act through it |

Somebody shown only the first has agreed to something considerably narrower
than the fourth.

The same split runs through the network door. *May this package dial out* and
*where to* are two questions, so a package that asks to reach the network also
lists the exact hosts — no scheme, no port, no path, and no wildcard, because
`*.example.com` is a family nobody enumerated and it is the shape every
allow-list is eventually widened by. And reading somebody else's page is a
different agreement from filing something in it, so the write half is a
capability of its own, split on the verb, because the protocol already defines
`GET` and `HEAD` as safe and everything else as allowed to have an effect. A
line drawn anywhere else — *the verbs we think are usually harmless* — is not a
category you can ask a person to agree to.

I want to be precise about what that buys, because it is easy to oversell. A
package that can reach a host at all can put whatever it likes in a query
string, and can cause an effect with a `GET` wherever the other end is built
that way. The write capability stops a package that did not mean to change
anything, and does not stop one that did. What it buys is the sentence somebody
reads before installing — *this one acts, that one watches* — and a refusal for
an honest mistake. It is an agreement, not a boundary, and a document that let
it read as one would be lying in the exact place a person is deciding what to
trust.

---

## The one rule no check can hold

A package can keep secrets. Its own corner of the system keychain, the owner
half of every entry composed in Rust so a call can only ever name its own.

The recommended path never hands the value over at all: the manifest says *this
secret, in this header, to this host*, and Rust reads it out of the keychain and
puts it in the request there. It never crosses into JavaScript. A package that
never holds a value has nothing to leak.

But a package can read one, and then this rule applies:

> **A secret is never handed to an agent.** Not in the prompt of an order, not
> in the environment a process is raised with, not as a tool that answers with
> one. What an agent is given is a method that *does the work* — sign this
> request, fetch this page, post this comment. The password does the work and
> stays in the package; the agent gets the outcome.

We do not check that, and we say so in the documentation rather than implying
otherwise. A value that has crossed into a package's JavaScript is that
package's to pass on, and the call that would pass it on is invisible to
anything reading a manifest. A check that pretended otherwise would cost an
author the one sentence they need and buy nothing.

The reason it matters more here than in ordinary credential hygiene: what an
agent does with a token is worse than a leak. A transcript is kept, read back,
and sent to a model again. A token that reaches one has been *published* rather
than mislaid.

---

## Four things that cost us an afternoon each

**Tailwind generates the classes it finds in the source it is told to read.**
The window's build reads the window's own source; a package is not in it. So
every utility an extension used that the shell did not happen to use as well
produced no rule at all — no error, no warning, nothing in any file to open. A
section mounted, held its state, answered the keyboard, and drew without one of
its own margins. One section looked deliberately redesigned for a fortnight: its
close button positioned `-top-1.5 -right-1.5` sat at the bottom of its chip,
because `position: absolute` existed and the two offsets did not. Twenty-six
utilities were absent across two extensions, and every file anybody thought to
open was correct.

The fix is that a package compiles its own stylesheet and the host puts it on
the document *before* it fetches the module — before, so the first frame is
already styled. Rules travel with the package; values do not. Every token name
resolves to a variable the window defines, so retinting the window retints every
extension in it, with nothing rebuilt and nothing republished.

**A module loaded from another origin is a CORS problem, and the error says so
in no recognisable way.** Our artefacts are served over a URI scheme of our own,
and the first attempt failed with *"Cross-origin script load denied by
Cross-Origin Resource Sharing policy"* — a refusal that names neither the policy
nor the scheme and reads exactly like a network error. Fine, we added the
header. We measured it against a packaged build, wrote it up, and moved on.

The development build serves the window from a dev server on a different origin
entirely. Every extension loaded in development was refused with that same
sentence. The one loop an extension author works in was the one loop the
mechanism had never been run in — and installing from a folder is what makes the
system usable by anybody outside our repository, so until somebody actually
opened a window and looked, it made it usable by nobody.

**There must be exactly one React.** The dispatcher is per copy, so two copies
means hooks that throw in ways that look like your bug. A module cannot be
handed anything during its own evaluation — its imports are resolved before a
single line of yours runs — so the host publishes its objects on the global
*before* fetching the module, and the package's build points its shims there.
What the entry point receives is only what it could not have known: its own
identity, and the doors built for it.

The corollary is a rule we now apply everywhere: **one copy of anything with
identity or a design in it; its own copy of anything that is a pure rule.** React
because of the dispatcher. The component library because of portals and focus
traps. The design tokens because they *are* the design. But an icon is a pure SVG
component with neither, and a utility class means the same thing wherever it is
compiled — so those are bundled per package, and two packages carrying the same
forty bytes cannot come to disagree.

**We nearly built a WebAssembly runtime.** It was in the design as deferred, and
the research recommended it. Then a real package needed an action rather than a
screen, and we measured: a JavaScript engine adds 528 KB to a release binary,
643 KB with async support, against 4.03 MB for a minimal wasm runtime. But the
size was not the argument that ended it. Every extension that exists is written
in TypeScript, and a wasm runtime would have required their authors to learn a
second language to poll an API. The same author, the same package, the same
toolchain — one file builds the screen, another builds the handlers.

---

## What it is like to write one

A package that watches what your dependencies publish is about a hundred and
fifty lines. It declares two types, a section, a host it may reach, a secret it
writes and never reads, a handler on a six-hour clock, and one tool an agent may
call.

The handler is the interesting half:

```ts
export default function register(): Handlers {
  return {
    "radar.poll": async () => {
      const watching = await memory.list({ kind: "release-radar.watch", limit: 100 })

      for (const watch of watching.records) {
        const newest = await newestFor(String(watch.fields?.repository))
        if (!newest || newest.tag_name === watch.fields?.pinned) continue

        await work.order({
          kind: "agent.session",
          agent: "claude",
          title: `${watch.title} ${newest.tag_name} is out`,
          prompt: { text: `Read ${newest.html_url} and say whether upgrading is worth doing now.` },
          onInterrupted: "continue",
          about: watch.key,
        })
      }
    },
  }
}
```

Three constraints are visible in those lines, and each of them is a decision.

A handler runs for milliseconds and answers; the host is what outlives it. So a
handler never *does* work that takes hours — it **orders** it, gets a key back
before anything has happened, and the host raises the agent. A handler that
tried to do the work itself would be killed by its own five-second clock.

`title` is required and nothing else can supply it. Without one, a conversation
gets named after the first words said in it — which a handler wrote, to an
agent. A sentence written for a machine standing in for a sentence written for a
list reads exactly like something a person typed, and it is wrong in a way
nobody can put their finger on.

`onInterrupted` is required and has no default, because neither answer is right
for both cases. *Continue* is for a nightly poll that should finish without
anybody. *Wait* is for something a person would want to pick up themselves.

---

## What we would tell you to copy

**Put the rule where the compiler is.** "We agree not to name extensions in the
core" is a norm; deleting the union so there is no place to write a name is a
property. Norms lose to Tuesday afternoons.

**Split permissions on the question a person is actually being asked**, not on
the mechanism. *Runs code* and *runs code while you sleep* and *spends your
money while you sleep* are three sentences, so they are three capabilities —
even though one implementation answers all three.

**Say what a mechanism does not buy.** The write capability is an agreement and
not a boundary. The secret rule is unenforceable. The signature is verified and
reported and does not yet gate. Writing that down cost us nothing and is the
difference between somebody trusting the system correctly and trusting it
generally.

**Refuse in words; never drop a member in silence.** Our network door takes the
vocabulary of `fetch` narrowed to what crosses a process boundary — and what it
does not support, it refuses *by name*. A `signal` that is silently ignored is a
timeout somebody believes they set.

**And run your own mechanism in the loop your users work in.** We had measured
the artefact path, written it up, and shipped it — in the only build our own
authors would never be running.

---

*Sync is a macOS desktop environment where a project's memory, its agents and
its vocabulary are all packages. The extension format, the capability list and
the loader are documented in the repository: `docs/extension-architecture.md`
for the architecture, `docs/writing-an-extension.md` for building one.*
