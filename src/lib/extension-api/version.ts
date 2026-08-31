/**
 * What an extension is compatible with, and how that is decided.
 *
 * The application has a version — `0.8.0` at the time of writing — and it is
 * the wrong number for this. A release can redraw every panel in the window
 * without moving anything an extension can see, and a patch release can remove
 * an export. Tying the two would make every release a possible break and every
 * break invisible, so the surface carries a version of its own and moves on its
 * own clock.
 *
 * The clock has three rules and no discretion:
 *
 * | Change to `@/lib/extension-api` | Bump |
 * | --- | --- |
 * | An export removed, renamed or narrowed; a parameter added to a callback a host calls; a field dropped from something returned | **major** |
 * | An export added; an optional field added; an accepted type widened | **minor** |
 * | Nothing in the surface changed, and the published package must move anyway | **patch** |
 * | Nothing in the surface changed | nothing |
 *
 * The third row is a concession the fourth used to cover alone. The package
 * this number versions carries the `sync-ext` CLI as well as the surface, and
 * npm will not publish a version twice — so a packer fixed without touching an
 * export had no number to go out under. A patch says exactly that: the package
 * moved and the surface did not, and every range over it is unaffected.
 *
 * A version nobody verifies is a comment, so this one is verified: the surface
 * is extracted into `api/extension-api.api.md` and CI fails when the report
 * moves without this number moving with it. The number is in the report too —
 * `as const` puts the literal in the declaration file — which is what lets one
 * diff answer both halves of the question.
 */

import { satisfies, valid, validRange } from "semver";

/**
 * The version of the surface this build publishes.
 *
 * Started at 1.0.0 rather than 0.x deliberately. A zero major says "nothing is
 * promised", which would be true of the code and false of the intent: the whole
 * point of the number is that a manifest can state a range and be believed. The
 * cost is honest major bumps, which is the cost of meaning it.
 *
 * **3.4.0** is where a conversation happens. `startSession` takes a `worktree`,
 * which is either `"new"` or a tree that already exists, and a `SessionRow`
 * carries the tree it is being held in; `worktreesIn`, `adoptWorktree` and
 * `discardWorktree` are the list and the two decisions a tree ends in. Added,
 * nothing changed, so a minor.
 *
 * A tree is chosen when a conversation is opened and not afterwards, which is
 * the shape of the addition rather than an omission from it: the directory
 * reaches the agent in `session/new` and it reads files from there, so a
 * conversation whose tree could be changed under it would be an agent answering
 * about files it never saw. What a package offers on a running conversation is
 * therefore the two gestures, never the choice again.
 *
 * The path of an existing tree is checked against git's own list of this
 * project's trees. Where trees live is the installation's choice, and a caller
 * that could name any directory would have taken that choice away.
 *
 * **3.3.0** is a package sending something that is not text. `NetRequest` gains
 * `bodyBase64` for bytes and `form` for `multipart/form-data`, and `NetPart` is
 * what one part of a form is. Members added and none changed, so a minor: a
 * package that sends JSON is compiled by this the way it was before.
 *
 * The three are spelled apart rather than `body` being widened, and that is the
 * decision the number is really recording. One member meaning text or bytes
 * would be a member whose meaning is guessed from its contents — a base64
 * string is text, and a package sending one as text would go on being right
 * until the day it wasn't.
 *
 * **3.2.0** is the half of an extension with no screen reaching what the half
 * with one already reaches: `@sync-buzz/extension-api/service` gains `vault`
 * and `net`, so a handler may read, replace and forget its own secrets and make
 * a request against the hosts its manifest declared.
 *
 * **The number moves although `api:check` says the surface is unchanged**, and
 * that is the one thing to know about this entry. The service surface is
 * published as a second file — it is what a handler compiles against, not what
 * the window does — so API Extractor never sees it and the report never moves.
 * What decides the number is what a package can name: an author who writes
 * `import { vault } from "@sync-buzz/extension-api/service"` against a range
 * that resolves to 3.1.0 gets a contract with no such export. Exports added, so
 * a minor, and the check that cannot see them is stated here rather than
 * trusted to be remembered.
 *
 * **3.1.0** is what an extension offers an agent, written where an agent can
 * read it. `InstalledExtension` gains `tools` and `ToolDeclaration` is what one
 * of them is: the name a call carries, the sentence the decision to call it is
 * made on, and the schema its arguments are checked against.
 *
 * On the record rather than only in the manifest because the two are read by
 * different processes. A manifest is on the machine that installed the package;
 * the record travels with the repository, and the server an agent reaches has
 * no view of the catalogue at all — so a declaration that stayed in the window
 * would be one only the window could read, which is the same reason `prompt` is
 * already there.
 *
 * The package's own name for the function behind a tool is deliberately absent.
 * It is how the package finds its own code, it changes when its author renames
 * something, and nothing outside the package can act on it.
 *
 * An optional field and an export added, so a minor.
 *
 * **3.0.0** is the network door made whole, and the first major since this
 * number started. `ExtensionNet.read` is gone; `ExtensionNet.fetch` takes a
 * `NetRequest` — a URL, a method, headers and a body — and answers a
 * `NetResponse`, which carries the final URL, the status, `ok`, the response's
 * headers and the body. `NetAnswer` is gone with `read`, and `net.write` joins
 * the capability list.
 *
 * **Why a major rather than a second method beside the first.** `read(url)` and
 * `fetch(request)` would have been two ways to make one request, which is how
 * one of them comes to behave differently from the other; and the older one
 * would have gone on being the shape an author copied, because it is the
 * shorter. The surface is small and the number is what a range is stated
 * against, so paying for the break here is cheaper than carrying a second door
 * for as long as this package exists.
 *
 * **Why this vocabulary.** It is `fetch`'s, narrowed to what crosses a process
 * boundary, so an author writing against somebody else's API is reading the
 * same words in our documentation and in theirs. Stated as one object rather
 * than a URL and an init, because that is the shape that actually crosses:
 * Rust reads the same members back, and a request has one spelling instead of
 * one per surface. What `fetch` has and this does not — streams, `Request` and
 * `Response` objects, `signal`, `credentials`, `mode`, `cache`, `redirect` — is
 * refused by name when it is passed rather than dropped, so an author hears
 * about the timeout they thought they set.
 *
 * **Why `net.write` is its own capability.** Reading something nobody in this
 * window wrote and writing something into somebody else's are two things to
 * agree to, and a card that said only the first would describe the smaller of
 * them. The line between them is the protocol's: `GET` and `HEAD` are defined
 * as safe, everything else is defined as being allowed to have an effect. It is
 * declared in the manifest, so the card is honest before anything runs, and
 * refused at the call, because which verb a package uses on a given day is
 * inside its JavaScript.
 *
 * **2.17.0** is a tool an agent calls. `agent.tools` joins the capability
 * list, and a manifest declaring `tools` without it is refused when it is read.
 *
 * The list is the whole of the change: what a tool *is* — a handler, the name
 * it is published under, the sentence it is chosen on and the shape of what it
 * takes — is written in the manifest and read by the host, and there is nothing
 * for a package's own code to say about it. The same shape `schedule` has, and
 * for the same reason: a fact about the package that a person is shown before
 * anything of it runs belongs in the file they are shown, not in a call.
 *
 * A capability added and nothing else, so a minor. Every package stating `^2.0`
 * goes on installing, and one that offers no tools cannot tell this release
 * from the last.
 *
 * **2.16.0** is a package's own corner of the system keychain.
 * `ExtensionHost` gains `vault`, `ExtensionVault` is its shape — read, write,
 * forget — and `vault` joins the capability list.
 *
 * Handed over rather than imported, exactly as `net` is, and for a sharper
 * version of the same reason: the owner half of every entry is the id this
 * machine resolved, so a package able to pass an id would be a package spelling
 * somebody else's namespace. There is no function on this surface to call
 * instead, and nothing to hold for a package that did not ask.
 *
 * The three calls are one capability rather than three, because the flow that
 * needs any of them needs all of them: a package that signs somebody in holds a
 * token nobody could have typed, replaces it before it expires, and drops it
 * when they sign out. A choice every author makes the same way is not a choice.
 *
 * What the shape cannot say for itself is said in `ExtensionVault`'s own doc
 * comment, where an author reads it rather than where it is archived: a secret
 * is never handed to an agent, and this build does not check that.
 *
 * A member added to what a host hands over, an export added and a capability
 * added, so a minor: nothing an existing package names has changed, and one
 * that never asks for `vault` cannot tell this release from the last.
 *
 * **2.15.0** is the record a conversation is being held under. `SessionRow`
 * and `RememberedConversation` gain `about`, `SessionAbout` is its shape — a
 * key, a kind and a title — and `openSession` takes one.
 *
 * It is beside `source` rather than inside it because the two answer different
 * questions and only one of them has a person as an ordinary answer. A
 * conversation somebody opened from a task has no orderer and is still about
 * that task, so a list grouped by who asked leaves every one of those in one
 * undifferentiated heap — which is exactly what a section that hands work to an
 * agent produces most of.
 *
 * All three members are on it because a heading needs all three: the key is
 * what a list groups by, the kind is what opening the record takes beside it,
 * and the title is what the heading says. The title is a snapshot, so a heading
 * is drawn without reading the corpus once per row of a list that is polled
 * every few seconds — the same bargain `extensionName` makes, going stale in
 * the same direction.
 *
 * `SessionSource.about` stays where it is and keeps its meaning: the key the
 * order named, kept with the rest of the order. Dropping it would narrow
 * something already returned, which is a major by the table above, and the
 * number would buy nothing a reader of `about` does not already have.
 *
 * `useOpenRecord` comes with it, because a heading naming a record and no way
 * to reach it is a heading that lies about being one. It is the narrow half of
 * what the shell's own bodies use: *show this one*, by key and kind, and
 * nothing about parsing a body, finding a picture or spelling a link. It
 * answers `null` where nothing can show anything — the settings window, a test
 * — so a section leaves the command out rather than drawing one that does
 * nothing.
 *
 * Optional fields added, two exports added, and one optional argument on a
 * call, so a minor: every package stating `^2.0` goes on installing, and one
 * that never reads `about` cannot tell this release from the last.
 *
 * **2.14.0** is a picture the agent answered with. `Entry` gains a `picture`
 * block and `SentImage` is its shape: a media type, how many bytes it was, and
 * the id the session holds those bytes under — which is `sessionImage`'s
 * argument, the same call a pasted picture is already drawn by.
 *
 * The bytes are deliberately not in it, and that is the whole design rather
 * than an economy. A session's history is replayed **whole** to every screen
 * that comes back to the conversation, so base64 carried in an event is paid
 * for on every return for as long as the session lives. The host takes it out
 * of the update as it records it and holds it against the one ceiling a
 * conversation has for pictures — the same ceiling what was pasted into it
 * counts against, because an agent asked for twenty pictures in one turn fills
 * the same conversation a person can. `imageId` is `null` when it would not
 * fit, and the block stays: a turn that lost its picture entirely is a turn in
 * which the agent answered with nothing.
 *
 * `saveSessionImage` and `imageFileName` come with it, and they exist because
 * the webview's own image menu cannot be made to work here. WebKit draws `Save
 * Image` and `Open Image in New Window` on any `img`; the source is a `data:`
 * URL, saving one needs a download handler the shell does not install, and
 * opening one is a navigation the content security policy refuses. Both were
 * measured dead. Two menu items that look like the system offering something
 * and then do nothing are worse than no menu, so a picture is given a native
 * menu of its own and this is what its command calls. The bytes are not sent
 * back to be written: the panel answers with a path and Rust writes what the
 * session already holds.
 *
 * Exports added and a returned shape widened, so a minor by the table above,
 * exactly as 2.10.0 was. A package that has never heard of `picture` goes on
 * installing and goes on running: an unknown block falls out of its switch and
 * draws nothing, which is what it did with the picture before this build, and
 * what breaks is a compile it will only see when its author next builds it.
 *
 * **2.13.0** is which repository this project is. `projectRemote` answers with
 * `origin` as git states it, or `null` where there is none.
 *
 * An export added, so a minor.
 *
 * It exists because a section that reads a forge had nowhere to get its
 * subject. Its first shape asked a person to type `owner/name` into its own
 * column, which is the interface asking for something the machine already
 * knows — and worse, letting one project be pointed at another project's forge
 * by a typo nobody would notice.
 *
 * A call rather than a member of `OpenProject`, which is where it was first
 * put. Three things decided it: the shell never needs the value, so every
 * construction site of an open project would have been answering a question
 * nobody asked; one of those sites describes a folder in the opening flow that
 * is not a repository yet; and an `origin` added while the window is open is a
 * different answer a second later, which a field captured at open cannot give.
 *
 * Unparsed, and that is the part worth defending. `git@github.com:o/r.git` and
 * `https://github.com/o/r` are one repository in two spellings, and turning
 * either into an owner and a name is a claim about a *forge* — which this build
 * must not make, for the same reason no type in it may name an extension. The
 * package that knows what GitHub is does the parsing, and the package that
 * knows what GitLab is will read the same field.
 *
 * **2.12.0** is the node a `ScrollArea` actually scrolls. The component takes
 * `viewportRef`, which is handed the element Radix wraps and nothing else
 * changes.
 *
 * Panels here own their scrolling, which had always meant the scroller decides
 * where it sits and nobody asks. A conversation is the case that breaks it: an
 * answer arriving a chunk at a time has to follow the bottom edge while the
 * reader is at it and let go the moment they scroll away, and both halves of
 * that are readings of one number — how far the viewport is from its own end.
 * The number lives on a node the surface published no way to reach. `ref`
 * reaches the root, which is the box the panel is measured as and never the one
 * that scrolls, so an extension either dug the viewport out of the DOM by the
 * attribute the shell happens to mark it with, or scrolled with
 * `scrollIntoView` — which asks every scrollable ancestor to help and is the
 * call the shell's own foundation forbids.
 *
 * The node rather than the behaviour, deliberately. Following a stream is one
 * reading of that number; holding a place in a long document while it re-renders
 * is another, and restoring one across a remount is a third. A `stickToBottom`
 * prop would have answered the first and left the other two digging in the DOM
 * again — and it would have put a chat's own control in a component every panel
 * in the window draws.
 *
 * An optional prop added, so a minor: every existing call site passes nothing
 * and gets exactly the component it had.
 *
 * **2.11.0** is the network, and it is the first thing on this surface that
 * reaches outside the project at all. `ExtensionHost` gains `net`, `NetAnswer`
 * and `ExtensionNet` are its shapes, and `net` joins the capabilities.
 *
 * A member added to what the host hands over, so a minor: every package stating
 * `^2.0` goes on installing, and one that never looks at `net` cannot tell this
 * release from the last.
 *
 * It is on the host object rather than an exported function, and that is the
 * whole design rather than a matter of taste. What a package may reach is a
 * sentence in its own manifest, so the request has to reach Rust attributed to
 * a package — and the only two ways to attribute it are an argument the caller
 * supplies, which is an extension naming its own permission, or a door the host
 * builds for one package and hands to it. The second is the one that is worth
 * checking, so it is the one on the surface.
 *
 * What it deliberately does not carry: a method, a body, a header, a response
 * header. Read-only is not a limitation to be lifted when somebody asks — a
 * header is where a token goes, and a token is a further agreement with a
 * person rather than a field the surface already had.
 *
 * **2.10.0** is the fields a list carries. A row was a name and a state, which
 * was every question anybody asked of a list until an extension published a
 * type whose records are grouped by one of their own fields — a task by its
 * status, in the first case. That question could not be answered from a
 * listing, and it could not be answered around one either: the only read of a
 * single record in this surface is a hook, and a hook cannot be called once per
 * row.
 *
 * `MemorySelection` gains `fields`, which names what each row should carry, and
 * `MemoryRecord` gains `fields`, which is what came back. Absent asks for none
 * and is what every caller written so far already asks for, so nothing draws
 * anything new by accident.
 *
 * Optional on the row, and it had to be: a package builds a `MemoryRecord` of
 * its own for a row that is not in the corpus — for a sheet that asks what
 * holds on to a record, say — and a required member would have made
 * every one of those a compile error over a fact it has no answer to. That
 * would have been a major, over a member nobody had asked for. Absent rather
 * than empty on the wire as well, so the shape a caller reads is the shape a
 * caller may build.
 *
 * It is deliberately a request rather than *all of them*. A type may declare a
 * field of several lines, and a listing of two hundred rows that drew none of
 * it would have carried two hundred pages of prose to a column showing titles.
 * Naming the two or three it will draw costs a caller one line and is the only
 * version of this that stays honest as types grow.
 *
 * Nothing is fetched for it: the envelope already arrived whole, and the fields
 * were being dropped on the way out. What changed is which of them are kept.
 *
 * An optional field added and a returned shape widened, so a minor, and every
 * package stating `^2.0` goes on installing and behaving exactly as it did.
 *
 * **2.9.0** is how many conversations an order's work leaves behind, and it is
 * on the service half of the surface rather than this one. `WorkOrder` gains
 * `keep`, which is `"each"` or `"latest"`; absent it is `"each"`, which is what
 * every package built so far already does.
 *
 * A handler on a fifteen-minute clock orders ninety-six times a day, and until
 * now that was ninety-six conversations. A project keeps a hundred pointers, so
 * within a day one standing instruction had pushed out every conversation its
 * owner had held themselves. `"latest"` says *one conversation about this
 * record at a time*, and the run that starts replaces the one before it — so
 * the last run is always there to read and the list stays a list.
 *
 * It requires `about`, and requires it rather than defaulting: the slot is the
 * record, and `"latest"` with nothing named is a package asking to keep the
 * most recent of nothing. Two things it deliberately does not do — it does not
 * touch a conversation somebody kept as a record, because that is a decision a
 * person made and it outranks a package's arrangement of its own rows; and it
 * takes the previous run away only *after* the new one has started, so an agent
 * that will not rise leaves the last readable account standing.
 *
 * An optional field added, so a minor, and every package stating `^2.0` goes on
 * installing and behaving exactly as it did.
 *
 * **2.8.0** is session modes, which is Plan, Accept Edits and Default on Claude
 * Code and the equivalent wherever else an agent has them. The agents have been
 * stating these all along, in the same `session/new` answer the model options
 * arrive in; this build read one member of that answer and dropped the other,
 * so the choice a person makes several times an hour had no way of reaching the
 * window at all.
 *
 * `AgentSession` gains `modes` and `setMode`, and `SessionMode` and
 * `SessionModeState` are the protocol's own shapes for them. Which mode is
 * current is deliberately *not* among them: it is `transcript.mode`, where it
 * already was, because two things say it — the state the agent stated and its
 * own `current_mode_update` — and one field written twice in sequence is one
 * answer where two fields would be two to keep in step.
 *
 * A conversation an extension ordered gets this as well, and gets it for free:
 * the modes are held by the session rather than by whoever was watching when it
 * opened, so a screen that attaches to work raised at three in the morning
 * draws the same control as one that raised it itself.
 *
 * Additions only, so a minor, and every package stating `^2.0` goes on
 * installing.
 *
 * **2.7.0** is who ordered a conversation, in the three places a person or a
 * package meets one. A session an extension ordered is an ordinary session in
 * every other respect — it is in `useLiveSessions`, it can be watched and
 * stopped — and until now nothing told it apart from a conversation somebody
 * started by typing.
 *
 * `SessionRow.source` and `RememberedConversation.source` are that, and both
 * are needed rather than one: work ordered overnight has *finished* by the time
 * anybody looks, so the row somebody reads in the morning is the dormant one.
 * A source names the extension, the handler, what it was about, and **the order
 * it came from** — because one handler may order three times, and three rows
 * carrying the same extension and handler are three rows nothing can tell
 * apart. That last field is also what lets a package say "task 42 is running"
 * on its own screen, with no second call and nothing asked of any other
 * extension.
 *
 * `work.order` gains a required `title`. It is required because nothing else
 * can supply it: without one the conversation is named after the first words
 * said, which a handler wrote *to an agent*, and a sentence written for an
 * agent standing in for a sentence written for a list reads exactly like
 * something a person typed. `work.order` was introduced in 2.6.0 in this same
 * session, so no package can have been written against the shape without it.
 *
 * Additions to what is returned and one field on a call nothing had yet used,
 * so a minor, and every package stating `^2.0` goes on installing.
 *
 * **2.6.0** is `work.agent`, and with it the only function on the service
 * surface that changes anything: `work.order`. A handler runs for milliseconds
 * and may order an agent that runs for hours, so it orders and Sync performs —
 * the order is written down, a key comes back, and the handler is finished long
 * before the agent has been raised. The capability arrives with the machinery
 * that honours it, as `schedule` did, and it is named separately from
 * `background` for the reason `docs/background.md` §5 gives: this is the one
 * that spends somebody's tokens while they are asleep, and the card is where
 * they agree to that.
 *
 * It is also the first capability enforced when the call is made rather than
 * when the manifest is read. `background` and `schedule` are visible in the
 * file; whether a handler calls `work.order` is inside the built JavaScript, and
 * no reader of a manifest can see it. An addition, so a minor, and every package
 * stating `^2.0` goes on installing.
 *
 * **2.5.0** is the service surface — `@sync-buzz/extension-api/service`, the
 * half of the contract a handler is written against. It is an addition, so a
 * minor, and every package stating `^2.0` goes on installing.
 *
 * It is the one entry in this list that the rollup below does not describe, and
 * that is deliberate: the service surface is hand-written in the contract
 * package rather than extracted from here, because putting it in the same
 * declarations a UI module imports would let a window call functions that only
 * work inside a handler's isolate. `pnpm api:check` therefore says the surface
 * is unchanged, and the number still has to move — what a manifest states a
 * range over is the *package*, and the package gained an export. A version that
 * did not move would leave an author unable to say they need it.
 *
 * **2.4.0** is `schedule`, and it is the capability arriving with the machinery
 * that honours it rather than before it. Until this build there was a clock in
 * the manifest, a schema that validated it and a host that refused it: a
 * package could say when it wanted to run and could not be installed, which is
 * the correct half-built state — a build that publishes a capability it cannot
 * keep is lying in the one place a person is deciding what to trust. What ships
 * with the name is the clock itself, in the process that survives every window
 * being closed, and the switch that stops it for one project. An addition, so a
 * minor by the table above, and every package stating `^2.0` goes on
 * installing.
 *
 * **2.3.0** is two additions and no removals, so a minor by the table above and
 * every package stating `^2.0` goes on installing. `background` joins the
 * capabilities: a package that ships a service module needs a build with a
 * runtime to call it, and whether there is one is a fact about the build rather
 * than about the manifest — which is why it is declared and refused rather than
 * inferred. `SourceList` gains `onReorder`, and a list is rearrangeable exactly
 * when it is given one: the window's own sections are, the settings window's
 * two screens are not, and an extension's column decides for itself.
 *
 * **2.2.1** changes nothing an extension can see. It exists because `pack`
 * never learned about `styles`, which entered the manifest at 2.1.0 below: the
 * first release built by that packer shipped two archives that download, hash
 * exactly as the index says, and then will not open, because the one file they
 * declare was not in them. The fix is in the CLI this package carries, and a
 * patch is the only honest number for it — see the third row above.
 *
 * **2.2.0** is badges, in the two halves a badge has, and every part of it is
 * an addition — a minor by the table below, and every package stating `^2.0`
 * goes on installing. `badge` in the manifest is a count over the corpus that
 * the host answers **without running a line of the package**, which is what
 * makes a mark appear on a section nobody has opened: an area is mounted on
 * first visit, so the launch after a project is opened is exactly when a
 * running section could report nothing. `useBadge` is the other half, for what
 * only a running section knows — an agent's reply that arrived while somebody
 * was in another section is nowhere in the corpus and no query would find it.
 * Reporting nothing is not a report, so a section may declare a standing count
 * and speak over it when there is news, and it decides which of the two its row
 * should say. `SourceListItem` carries the same mark, so a list inside an
 * extension's own column reads like the window's.
 *
 * **2.1.0** adds `styles` to the manifest — an optional field, so a minor by
 * the table below, and every package stating `^2.0` goes on installing. It is
 * where a package names the stylesheet holding the utility rules its own markup
 * uses. Before it there was nowhere to put them, and the consequence was not an
 * error anywhere: Tailwind generates what it finds in the source files it is
 * given, the window's build reads the window's own `src`, and an extension is
 * not in it. Every utility a package used and the shell did not happen to use
 * as well produced no rule at all, so a section drew without its own spacing,
 * sizing or alignment and read as a redesign.
 *
 * **2.0.0** is the first of them, and it was paid for exactly what the rule
 * says: `ExtensionType` left the surface. A vocabulary is now a JSON file
 * inside the package rather than a constant in an extension's TypeScript, so
 * the type an extension used to name is a type that no longer describes
 * anything it writes. The contract an extension implements — `ExtensionHost`,
 * `AreaModule`, `ActivationResult` — arrived in the same commit, which on its
 * own would have been a minor.
 */
export const SYNC_API_VERSION = "3.4.0" as const;

/**
 * What this build can do, as opposed to what its surface looks like.
 *
 * Semver answers *is this surface compatible*. It cannot answer *can this build
 * do the thing*: a platform with no bundled ACP sidecar exposes exactly the same
 * `useAgentSession` type and cannot raise an agent behind it. Expressing that as
 * a version would mean a different version number per platform, which is a lie
 * in the other direction.
 *
 * So a capability is a promise about behaviour, named, and a manifest may
 * require one. Reading whether one is present is allowed too — an extension
 * that degrades deliberately is doing something better than refusing.
 */
export const SYNC_CAPABILITIES = [
  /** The corpus: types, records, freshness, the editor and the metadata panel. */
  "records",
  /** Agents driven over ACP, as processes on this machine. */
  "agents.acp",
  /** Replacing how a block of stored Markdown is drawn. */
  "markdown.plugins",
  /** Secondary click opens a system menu rather than a drawn one. */
  "native-menu",
  /** The repository's own folders, as a hierarchy records are filed in. */
  "folders",
  /** The window-level sheets: a type, a removal, a folder. */
  "sheets",
  /**
   * Reading something outside this window, from the hosts a manifest names.
   *
   * The promise is the door and the check behind it: a request goes to Rust,
   * where the package's own `net.hosts` is read off the artefact on this
   * machine, and a host it does not name — on the first request or on any
   * redirect after it — never leaves the machine. A build without it publishes
   * the same `ExtensionHost` type and refuses every call, which is exactly the
   * question a capability exists to answer.
   *
   * Named for what it is rather than for what it will be used for, as
   * `background` is. It is reaching and reading, and it is not changing
   * anything at the other end: that is `net.write`, which is the agreement this
   * one is deliberately not a wider reading of.
   */
  "net",
  /**
   * Changing something where this package reaches, rather than reading it.
   *
   * The second half of one permission rather than a permission of its own,
   * which is why a manifest asking for it without `net` is refused: what a
   * write is checked against is the list of hosts, and the list arrives with
   * the other one.
   *
   * The line between the two is the protocol's own — `GET` and `HEAD` are
   * defined as safe and everything else is defined as being allowed to have an
   * effect — because a verb that is merely usually harmless is not a category
   * anybody can be asked to agree to. Declared in the manifest so the card can
   * say it before anything runs, and refused at the call, since which verb a
   * package chooses on a given day is inside its JavaScript.
   */
  "net.write",
  /**
   * A package's own corner of the system keychain.
   *
   * The promise is a namespace and one door to it. The owner half of every
   * entry is the id this machine resolved, so a package reads, writes and
   * forgets its own secrets and has nothing to say about whose — and a build
   * with nowhere to keep a secret publishes the same `ExtensionHost` type and
   * refuses every call, which is the question a capability exists to answer.
   *
   * Checked when the call is made rather than when the manifest is read, as
   * `work.agent` is: whether a package touches a secret at all is inside its
   * built JavaScript, and the file a person installs says nothing about it.
   *
   * Reading, writing and forgetting are one agreement rather than three. A
   * package that signs somebody in holds a token nobody could have typed,
   * replaces it before it expires and drops it when they sign out; splitting
   * that into choices nobody makes differently would be three cards saying the
   * same thing.
   *
   * What a package may not do with a value it holds is not in this promise,
   * because it is not something a promise could hold: a secret is never handed
   * to an agent, and the reasoning is beside `ExtensionVault`, where the author
   * who has to keep it reads it.
   */
  "vault",
  /**
   * Handlers: code the host calls with no screen mounted.
   *
   * Required by any package that ships a service module, and the reason it is a
   * capability rather than an inference from the manifest is that it is a fact
   * about the *build*. A platform without the runtime behind it would read the
   * same manifest and be unable to honour it, and saying so before anything is
   * installed is the whole purpose of this list.
   *
   * It is named for what it is, not for what it will be used for. `schedule`
   * and `work.agent` are separate promises and arrive with the machinery that
   * keeps them — a capability a build publishes and cannot honour is a lie in
   * the one place a person is deciding what to trust.
   */
  "background",
  /**
   * The clock: a handler this build calls on an interval the manifest states.
   *
   * A second promise rather than a wider reading of `background`, and the two
   * are two different agreements. `background` says this package runs code;
   * `schedule` says it runs while nobody is there, on a machine whose windows
   * are all closed. A person shown only the first would have agreed to the
   * narrower of the two, which is why a manifest declaring `schedule` is
   * refused unless it asks for this by name.
   *
   * What the build promises with it is an interval and nothing more: lateness
   * is not made up for, drift is not corrected, and a machine asleep for six
   * hours runs a handler once when it wakes rather than six times.
   */
  "schedule",
  /**
   * Ordering work that runs an agent.
   *
   * The expensive one, and named on its own for exactly that reason: it spends
   * somebody's tokens while they are asleep, and the card a person installs
   * from has to say so before the first bill rather than after it. `background`
   * agrees that this package runs code and `schedule` that it runs unattended;
   * neither is an agreement about money.
   *
   * Unlike the other two, this one cannot be checked when the manifest is read.
   * A service module and a schedule are written in the file; whether a handler
   * calls `work.order` is inside the JavaScript. So the host refuses the call
   * itself — a refusal the handler can catch — and `sync-ext check` scans the
   * built module for it, which is the earliest anyone can be told.
   */
  "work.agent",
  /**
   * Tools an agent may call, which this package answers.
   *
   * The fourth agreement about code that runs with no screen, and the one whose
   * caller is not this application: `background` is the package running its own
   * code, `schedule` is running it unattended, `work.agent` is spending money
   * while somebody sleeps, and this is an agent being told the package is there
   * and acting through it. A person shown only the first has agreed to
   * something considerably narrower.
   *
   * Checked when the manifest is read rather than when the call is made, as
   * `schedule` is and unlike `work.agent`: a tool is declared in the file — a
   * handler, a name, a sentence and a schema — so a package offering one
   * without asking for this is answerable before anything of it runs.
   */
  "agent.tools",
] as const;

export type SyncCapability = (typeof SYNC_CAPABILITIES)[number];

/** What a package says it needs, as its manifest states it. */
export interface ApiRequirement {
  /** A semver range over `SYNC_API_VERSION`, such as `^1.2`. */
  readonly syncApi: string;
  /** Capability names. Unknown ones are refusals, not warnings — see below. */
  readonly capabilities?: readonly string[];
}

/**
 * Why an extension may not run here, in one sentence, or `null` when it may.
 *
 * One function rather than two booleans because the answer a person needs is
 * never "false": it is which of the two things to do about it. A range this
 * build is below means update Sync; a range it is above means the extension is
 * old and its own update is the fix. Those are different sentences and the
 * caller should not have to compose them.
 *
 * A capability this build has never heard of is refused rather than ignored. It
 * arrives in exactly one situation — a package built against a newer host —
 * and treating it as satisfied would run an extension that asked for something
 * and did not get it, which fails later and somewhere else.
 */
export function refuseIncompatible(required: ApiRequirement): string | null {
  if (validRange(required.syncApi) === null) {
    return `The extension states an unreadable Sync version range (${required.syncApi}).`;
  }

  if (!satisfies(SYNC_API_VERSION, required.syncApi)) {
    return `This extension was written for Sync's extension API ${required.syncApi}, and this build publishes ${SYNC_API_VERSION}.`;
  }

  const unmet = (required.capabilities ?? []).filter(
    (capability) => !(SYNC_CAPABILITIES as readonly string[]).includes(capability),
  );
  if (unmet.length > 0) {
    return `This build cannot do what the extension needs: ${unmet.join(", ")}.`;
  }

  return null;
}

/**
 * Whether this build satisfies a range, for a caller that only wants the fact.
 *
 * `false` for an unreadable range, and that is the only sensible reading: a
 * range nobody can parse is not one this build was stated to satisfy.
 */
export function supportsApiRange(range: string): boolean {
  return validRange(range) !== null && satisfies(SYNC_API_VERSION, range);
}

/**
 * Whether a string is a version at all.
 *
 * Here rather than in a manifest validator because the answer belongs with the
 * number: a packer, the loader and the catalogue all ask it, and three spellings
 * of "is this a version" is how two of them end up disagreeing.
 */
export function isVersion(candidate: string): boolean {
  return valid(candidate) !== null;
}
