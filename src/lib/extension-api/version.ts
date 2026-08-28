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
export const SYNC_API_VERSION = "2.13.0" as const;

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
   * `background` is. It is not a token, not an account, and not writing: the
   * surface has no header and no body on the way out, so a package that reaches
   * a private repository is a further agreement rather than a wider reading of
   * this one.
   */
  "net",
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
