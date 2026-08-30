# Design foundation

This document records what the shell is, why it looks the way it does, and what
has to be true before it changes. It describes the delivered repository — it is
not a roadmap.

## Product character

Sync is a professional macOS environment for work a person directs and agents
carry out. It is not a website, a SaaS dashboard, an IDE clone or a chat client.

**What kind of work is not the shell's business, and that is a design
constraint rather than a modesty.** A project is a repository and the packages
installed into it; what those hold — documentation, research, requirements,
changes, verification, review, or something nobody here has thought of — is
theirs to decide. So the shell has to feel capable of holding that depth
whatever the depth turns out to be, while staying calm enough that a person can
read its structure without being taught. A composition that assumed one subject
would be a composition that fought the second one, and the second one is not
ours to predict.

The reference points are macOS productivity applications: native window
composition, desktop density, system typography, clear sidebars and toolbars,
restrained motion, and both appearances designed from the first commit.

## Accepted design direction

- A column-based composition, not a page.
- Project switching lives in the header.
- A frame and a slab: a tinted material frame around the whole window, and the
  entire interface — toolbar and all four columns — as one opaque rounded slab
  inset into it.
- Quiet, neutral greys. No large coloured regions, gradients or decorative
  glass. The window material is structure, not decoration: it is what the back
  layer is made of, and nothing floats on top of it for effect.
- Clean borders, compact labels, disciplined button hierarchy and readable
  technical density — GitHub-adjacent precision without imitating GitHub.
- The workspace is the dominant area at every window size.
- The structure of the window is visible without explanation.

The shell is a direction, not a frozen layout. Surfaces that are not yet
settled are left empty and labelled rather than filled with plausible-looking
content.

## Token philosophy

`src/app/globals.css` holds one compact semantic layer. Components consume
tokens; they never contain a colour literal, a hard-coded control height, a
radius or a duration.

The layer covers window, sidebar, panel, workspace and raised-control surfaces;
primary, secondary and tertiary text; separators in two weights; hover,
selected and keyboard-focus states; the scrim a sheet dims the slab with;
success, warning and danger; spacing; control heights; corner radii; and motion
duration and easing. The shadcn/ui
variable contract (`--background`, `--foreground`, `--border`, `--ring`, …) is
mapped onto those tokens rather than maintained beside them, so the vendored
components inherit the system automatically.

Two deliberate choices:

- **Spacing is one token.** Tailwind 4 derives its whole numeric scale from
  `--spacing`, which is set to `4px`. That gives desktop density everywhere
  without a parallel scale to keep in sync.
- **An extension refers to a token and never declares one.** The `@theme inline`
  block is published to the contract by `pnpm api:publish`, so a package
  compiles `bg-panel` to `var(--surface-panel)` and ships no value of its own —
  retinting this window retints every extension in it, with nothing rebuilt. A
  package that declared a token would not be restyling its section; it would be
  repainting every column, sheet and menu, and `sync-ext check` refuses it. It
  compiles its own *rules*, because Tailwind generates only the classes it finds
  and it does not read a package's source — see `extensions.md` §5.
- **Panel geometry is not a CSS token.** Minimum, preferred and maximum widths
  live in `src/lib/shell-layout.ts` because they drive the panel group
  directly. Expressing them twice would create two sources of truth for the
  same numbers.

### Appearance

Both appearances follow the system by default, and settings can hold the window
in one of them. The dark appearance is designed rather than inverted: the rule
that carries over is *content sits furthest from mid-grey and chrome sits
closest to it*, so in light the workspace is the brightest surface and in dark
it is the darkest one. Every value — text, separators, status colours, the focus
ring — is chosen per appearance.

The decision lives on the document as `data-appearance`, and "follow the system"
is the **absence** of that attribute rather than a third value: the token layer
would otherwise have to keep a third state in step with two media queries.
Because it has to be applied before the window paints its first frame, it is
read from this window's own storage rather than over IPC — the window is hidden
until the layout effect that applies it has run, so a held appearance is never
seen arriving.

### Base colour

Every grey in the layer is mixed from one hue and one saturation scale
(`--tint-h`, `--tint-s`), which is precisely what separates the base colours
shadcn/ui publishes — zinc, neutral, gray, slate, stone. Choosing one retints
the surfaces, the text and the separators together and changes nothing else:
there is one design, not five. Status colours and the focus ring are not
tinted, because they are the two things that must not shift with a preference.

Zinc is the default and is what the shell was designed in.

### Typography

The macOS system stack, so an installed system resolves San Francisco. A system
monospace stack is defined for future technical content and tiny utility labels
and is not used decoratively. Nothing is downloaded.

### The window material, and what it costs

The frame is the macOS `underWindowBackground` material — the quietest of them,
meant to sit *under* a window's content rather than to be looked at — requested
in `src-tauri/tauri.conf.json` so the window opens with it already applied. No
crate had to be added: Tauri carries `window-vibrancy` itself and exposes it as
`windowEffects`.

Two rules keep it from becoming a glass theme:

1. **The material is muted, not transparent.** A tint at ~0.75 opacity sits over
   it, so the frame keeps the grey the token layer chose and the desktop only
   shifts its hue. The window stays the colour it was designed to be on any
   wallpaper.
2. **The material only ever touches the frame.** The slab — toolbar, sidebar,
   navigator, workspace, inspector — is fully opaque. Glass is the edge of the
   window catching what is behind it, never a surface the interface is built on.

Both rules are corrections. The first attempt used the `sidebar` material across
the full width of the title bar, and the second confined it to a vertical band
behind the sidebar; both read as glass applied to the interface rather than as a
window frame, and both were rejected on sight.

The price is explicit and was accepted deliberately: `windowEffects` requires
`transparent: true`, which on macOS requires `app.macOSPrivateApi`, and an
application built on a private API **cannot be published to the Mac App Store**.
Distribution is therefore direct.

Everything is designed to hold without it. `data-vibrancy` is set by
`useWindowMaterial` only after the effect is confirmed applied, so a browser,
a system asking for reduced transparency, and any platform where the effect
fails all fall back to the opaque back layer with no second visual story to
maintain.

### Launch

The window is created hidden (`visible: false`) and revealed by
`src/lib/window-reveal.ts` one frame after the shell has painted. A transparent
window that appears before its first frame shows the desktop straight through
itself — the material has nothing over it yet — and that reads as a broken
launch rather than as an application starting.

What the window shows on that first frame is decided by the document, not by a
timer:

- If the document is already complete, the window opens directly onto the
  interface. Nothing flashes.
- If it is not, the window opens onto `LaunchScreen`: the same slab, at the same
  size, on the same surface, with the name of the application, a progress track
  and the word `Starting`. The interface arrives inside it and the screen fades
  out rather than cutting.

Nothing is delayed to make the launch screen easier to admire. It appears only
when there is genuinely something to wait for, which is the only version of it
that tells the truth — and it is why the loading state has to be in the exported
HTML rather than rendered after hydration.

There is no logo on it. The product has no brand mark yet, and a launch screen
is the last place to invent one, so the name of the application carries it — at
`--text-display`, the one type size that exists above the interface scale and is
used nowhere else.

## Panel roles and priority

Four roles, addressed by name everywhere in the code:

| Role               | Purpose                                     | Behaviour                          |
| ------------------ | ------------------------------------------- | ---------------------------------- |
| Primary sidebar    | Durable product areas                       | Stable, collapsible on request     |
| Context navigator  | Items belonging to the selected area        | First to give up space             |
| Workspace          | The content surface                         | Always present, never collapsible  |
| Context inspector  | Optional tools for the workspace object     | Optional, collapses independently  |

Rules the implementation actually enforces:

1. The workspace keeps the majority of the width and never drops below
   `WORKSPACE_MIN_WIDTH`, currently 500 px.
2. Secondary panels collapse rather than letting every column shrink
   proportionally. The thresholds are derived from the widths, not guessed: a
   panel gives up its space at exactly the width where keeping it would push
   the workspace under its minimum.
3. Space is released before it is claimed, and the layout is applied as one
   atomic operation, so the result never depends on the order in which
   individual panels were resized.
4. Each panel owns its scrolling. The window itself never scrolls, and that is
   held by how the boxes around a panel are closed rather than by the code
   drawing inside one: they clip with `overflow: clip` and never with
   `overflow: hidden`. A hidden box is still a scrollport that has lost its
   bar, so the browser goes on scrolling it to reveal a caret or reach whatever
   was focused, and with no bar and no wheel reaching it the offset stays —
   leaving a column shifted with its foot over empty panel. A clipped box
   cannot hold an offset at all. A panel's own scroller stops at its end
   instead of passing the rest of the gesture on, so a flick through a list
   never moves the surface behind it.
5. Separators inside the slab are structural edges — one hairline, no shadow,
   no radius, no inset. No column is a floating card: the rounding and the
   shadow belong to the slab as a whole and are stated once, so no arrangement
   of columns can leave a corner or a seam to re-derive.
6. An edge beside a collapsed column stops drawing its hairline but stays in
   place. A collapsed column is zero pixels wide, so both of its edges would
   otherwise sit against each other and draw a line that divides nothing — but
   the edge must not leave the tree: collapsing by dragging ends with the panel
   library still holding a pointer capture on it, and removing it mid-gesture
   throws `InvalidStateError`. It is also how the column is dragged back out.
7. When the window is too narrow to hold a panel, its control says so instead
   of doing nothing.
8. The panel header is one band at one height across all three columns of the
   slab, and each column's header has to say something the others do not. That
   is why the navigator names the section, the workspace names what is being
   shown of it, and the sidebar carries no header at all. A header may carry a
   control, and only one kind: the command that writes into the very thing the
   header is naming.
9. A column that can be acted on carries a **bottom bar** at that same height:
   the navigator's holds the control that adds a type, the one that acts on the
   selected type, and the one that decides which types are listed — the actions
   on the leading edge, the view preference on the trailing one. That is where
   macOS keeps the actions belonging to a
   source list — Mail, Reminders, Music, Xcode's navigator — and it is the only
   band besides the header that does not scroll away with the list it acts on.
   The window toolbar is for the window; a control inside the scroller is one
   you have to scroll back to find. Everything in that bar acts on the list
   itself and nothing in it writes what the list contains: those are the two
   halves of the same convention, and the second half is why writing a record
   is `⌘N` and a control in the workspace's own header.
10. The sidebar's foot carries one **pinned row** — `Extensions` — in the same
    band, at the same height, as the navigator's bottom bar beside it, so the
    two read as one line across the slab. It is an area and selecting it
    deselects everything above it, but it is **not** marked with a filled
    surface: the band is 34 px and the fill would run into the hairline above
    it, and lifting the band would break the line it shares with its neighbour.
    Weight and colour carry the selection instead — the half of the selection
    rule that survives greyscale — and hover does not fill it either.

11. A section's row may carry a **badge** on its trailing edge, and it is the
    one thing in that column that says something about a row's *contents*
    rather than about the row. It sits in the slot the `Development` note used,
    at the tertiary tier, with **no colour**: a count is information and this
    window keeps colour for status and for destruction, so position, weight and
    the shape of the mark are what say it and the row reads the same in
    greyscale.

    **A figure and a dot are two claims, and never each other.** A figure is
    how many there are — standing, as true when nobody is looking. A dot is
    *something happened, go and look*, which is what a dot means everywhere
    else on this system, and it is what a section says when it knows there is
    news and has no number for it. A dot standing in for a figure too large to
    print would be one mark carrying both claims, so a large figure is
    abbreviated to `99+` instead and the dot stays what it is. The first pass
    had the dot doing both and it was rejected on sight, correctly: a mark that
    is permanently on is not news.

    That division decides the fold as well. On the rail the dot moves onto the
    icon and the figure does not appear at all: news is news at any width, and
    a standing figure is attached to the word that qualifies it — folding this
    column is the words leaving. The tooltip still says the figure. A dot earns
    a tooltip even in a column wide enough to have needed none, because it is
    the one mark here that cannot explain itself.

Layout state is ephemeral: it is rebuilt from the defaults on every launch, and
it is kept in `src/lib/shell-layout.ts`, separate from the selection state the
shell uses to demonstrate itself.

## Selection

Selection is a surface shift plus a weight change, and nothing else. No
coloured fill, no leading marker. Remove colour from the window and the
selected row is still the obvious one.

The shift is quiet: a selected row sits at 7.5% over the surface in light and
8.5% in dark, hover at 4.5% and 5%. An earlier pass ran a third heavier and read
as a highlighter rather than as a source list — the weight change is what says
"this one", and the surface only has to separate it from its neighbours. The
opaque equivalents used under reduced transparency are tuned to the same
apparent step rather than to the same numbers.

An earlier draft carried a two-pixel rail on the leading edge of the selected
row, offered as the shell's signature detail and as the future home of
verification state. It was removed: the sidebar rows
are navigation, not status, and a marker that means "selected" today and
"verified" tomorrow overloads one piece of geometry with two unrelated jobs.

## The signature detail: typed marks

The shell went without a signature detail for exactly as long as it had nothing
of its own to say. The rule was that one had to come from the product's own
subject rather than from branding — and the subject is already in the window:
context is typed, and every claim carries a validation state.

So the marks in `src/components/shell/entity-marks.tsx` are the shell's
signature, and they are the only place a visual language is spent on content
rather than on furniture:

- **Kind** is a glyph — a signpost, a lock, an eye, a question mark, a rule, a
  page. Neutral in colour, because colour is reserved for state. Which glyph is
  not decided here: a type carries the *name* of its mark in its own definition,
  and this module resolves the names it can draw. A project's types are the
  project's, so a type invented this morning is one no build has heard of, and
  it is drawn neutrally rather than guessed at.
- **Freshness** is a ring whose shape carries the meaning — solid for fresh,
  dashed for unverified, alerted for stale, crossed for invalid — always
  accompanied by the word itself, with weight added only to the two states that
  mean a claim stopped matching the code. The word is the engine's: memory-hub
  derives the state by reconciling code history against each record's scope,
  which is why it is the one shown.

Both survive the greyscale test on shape alone, and both appear in every column
that shows a claim, so the navigator, the workspace and the inspector read as
one language rather than three.

An earlier note here recorded that inventing a signature before the product had
one would produce branding rather than meaning. That still holds; this is the
other half of it. The marks were earned by what Sync is for:
a typed context store rather than a memory feature, which requires the types to
be visible in the interface.

## A record is edited where it is read

There is no edit mode, no pencil and no Done. A record opens as the text it is,
the caret goes where it was clicked, and typing changes it. That is the whole
interaction, and it is the only one that matches what actually happens: a person
reads a claim, sees the part that stopped being true, and fixes that part. A
button that turned reading into editing would ask them to declare an intention
they have already acted on.

The page is therefore one geometry rather than two — same measure, same margins,
same type scale, whether a record is being read or written. The title is part of
that surface, because it is stored beside the body in the same record and written
in the same transaction; a page whose text can be corrected but whose first line
cannot would be two rules in one column.

What the editor adds over the reading view is a caret, a list `/` opens, a
toolbar over a selection, and the commands a table needs.
`src/components/editor/nodes.tsx` and `src/components/shell/markdown.tsx` are the
same treatment of the same blocks and have to be changed together.

### The page is set like a document, not like a page of a website

Prose is set at one and a half, and blocks are half a line apart. Both numbers
are one decision and the first pass made it twice: a relaxed line under a full
line of air put 42 px between one paragraph's baseline and the next's, so
pressing Return dropped the caret most of a line further than the text it was
leaving, and every paragraph read as the start of a section. A desktop document
— TextEdit, Notes, Pages — sets prose at about one and a half and separates
paragraphs by half a line.

What air a section gets is carried by the heading that starts it, above itself,
rather than by the gap between every pair of paragraphs. A gap that has to say
"new section" cannot also be the gap that says "next sentence".

**The project's own record is a document like any other.** Its title is the
project's name, its body is the description and its `language` field is the
language the project writes its knowledge in — all three the project's own data,
all three edited here, with one line on the page saying which record this is.
What is not a document is a type *definition*: that is edited as a type, and
definitions are never listed as records. The project's record is the one that
cannot be created a second time, archived or deleted, because a project without
it cannot be opened. Both commands are still drawn on it and refused with the
reason: a menu missing an item explains nothing.

### Markdown is the format, so it decides the feature set

The store holds a body as Markdown. The editor is a view of that, not a second
format, so what it offers is exactly what survives being written back: headings,
paragraphs, lists — bulleted, numbered and task — quotes, fenced code, tables,
rules, links, and the four marks that carry meaning inside a sentence. No colour,
no font, no alignment, no image, no comment thread. Not because they are hard,
but because none of them is Markdown, and a block that vanished the next time the
record was opened is worse than a block that was never offered.

Three consequences are stated rather than hidden:

- **The first save reflows the body.** A soft-wrapped paragraph becomes one line,
  `*` bullets become `-`, and the serialiser adds its own escapes. None of that
  changes what the record says, and normalising it is what keeps a second save
  from producing a third spelling.
- **`[[wikilinks]]` are unescaped on the way out.** The serialiser escapes a
  leading bracket because a bracket starts a link. That one escape is undone by
  name, in `src/lib/editor/markdown.ts`. The write door refuses that spelling
  from an agent, but bodies holding it are already stored, and an editor that
  respelled one on opening would change a record somebody came to read.
- **A table has commands, and no width.** Rows and columns are what a table is,
  so they are seven commands: four insertions and three removals, on the
  system's own menu on any cell and in `Format ▸ Table` where the keyboard
  reaches them. Both call the same seven functions, and the menu bar draws them
  disabled when the caret is not in a table rather than growing a submenu as
  somebody types. Column widths are **not** offered, and that is the same rule
  rather than an omission: a Markdown table has no widths, so a column dragged
  wider would be back where it started the next time the record was opened.
- **A body the editor would not survive is read instead of edited, and says so.**
  Raw HTML, or anything whose round trip loses a word, is shown exactly as stored
  with one line explaining why. That is checked by round-tripping the Markdown
  rather than guessed at. A read-only record is a small disappointment; a record
  whose footnotes disappeared because somebody fixed a typo is a lost claim.

  What counts as raw HTML is narrower than what looks like a tag, and getting
  that wrong cost the application its most characteristic records. A round trip
  escapes a leading bracket, so `<Workspace>` in a paragraph about a component
  comes back as `\<Workspace>` and renders as the same words it always did —
  that is formatting. A `<sup>` or a comment is not: it did something, and
  escaped it does nothing but show its own source. So the check is for the names
  the platform actually renders, and a record describing software is editable,
  which is the one kind this product exists to hold.

  **`<br>` is the editor's own writing, so it is read *and* written as a line
  break.** A line break inside a table cell has no other spelling — a GFM row is
  one line — so pressing Return in a cell wrote a tag that the next open refused
  to edit, naming as raw HTML the thing the editor had just produced. Reading it
  back as a break without also writing it as one was worse and briefly shipped:
  a break inside a cell serialises to nothing at all, so the round trip dropped
  every line somebody had typed there. The two halves live beside each other in
  `src/lib/editor/markdown.ts` and neither is correct alone.

  The rule that follows is general: **a refusal to edit may only ever name
  something the editor could not have written**, and a spelling the editor
  produces has to be one it also reads.

### Saving is not a button

Typing stops, and 1.2 seconds later the record is written; leaving the record or
the window writes it immediately. Every save is a transaction and a commit on
`refs/memory/*`, which is what that delay is for — a history of what somebody
wrote rather than of every keystroke they took to write it.

Leaving means the open key changed, and that is where the write is hung. The
hook that owns a record outlives the view of it — the window keeps one for the
whole project — so a cleanup that waited for an unmount would have waited for
the project to close. It did, and the paragraph above was true of the document
and not of the build: a record left inside the delay was written a second later,
after the list behind it had already been drawn.

**A list is re-read when it is returned to, and when a write lands.** The rows
were read when the record was opened, so they describe the record as it was
then; a person who has just named something and gone back to look for it would
otherwise find it listed under its key. Both moments are asked for, and the
second is what makes the first invisible: a title written while the record is
open is already in the list before anybody goes back to it.

The header band says which of four things is true, and nothing else: nothing at
all for a record nobody has typed into, `Edited`, `Saving…` — the one place an
ellipsis is allowed, an action in progress — and `Saved`, said once, because a
write here is a commit and the person who made it is owed the confirmation.

A refused write is the only one that gets a colour and the only one that gets a
strip under the header: it names the engine's own reason, offers `Try again`, and
says that what was typed is still there. The draft outlives the view, so closing
the record and coming back to it does not discard it. Nothing pretends to have
saved.

### What is true *of* a record is edited beside it

The centre column is the claim; the context panel is what is true of it, and that
is where those facts are changed. Which of them a person may change is the
store's decision rather than this build's, and it comes out as three rules:

- **Identity is not editable.** The key is what every link and every agent refer
  to and the store has no rename; the kind is what validates the record. A form
  offering either would be a rewrite of the corpus in disguise.
- **Freshness is not editable.** The engine derives it by reconciling code
  history against the record's scope. It is an answer, not a field.
- **Everything else is, on the schema's own terms.** Tags, scope and observed
  paths, the archive flag — and the fields the record's type declares, drawn from
  the declaration. Nothing here knows what `validation_state` or `horizon`
  *means*, which is the whole point.

#### A control is chosen by what the value is

The schema says what each field is — `string`, `text`, `integer`, `number`,
`boolean`, `enum`, `array` of one of those, `object` — and each of those words
means a different thing to type into. The first pass drew a text box for nearly
all of them, which is the window refusing to read a schema it had been handed:
tags as `a, b, c` in one box, paths as lines in a textarea, and anything that
was not a plain string shown as JSON and not editable at all.

So each declaration is answered by the control that matches it, and this system
already has one for most of them:

- A **short repeated value** — a tag, an array of strings — is a token field.
  That is what `NSTokenField` is for, and it is why a tag is something one
  Backspace removes rather than a substring somebody has to find the commas in.
- A **path** is chosen with the system's open panel. It names a file in this
  repository, so the panel opens at the project and what is stored is relative
  to it; typing one by hand stays, because a claim may be scoped to a file that
  does not exist yet.
- A **choice** is a pop-up over exactly the values the schema allows, and a set
  of choices is those values as checkboxes.
- A **flag** is a checkbox, a **number** is a number field, a **string** is one
  line, and `text` is the several lines the schema says it is.
- A **shape no control can be generated for** — an object, an array of objects —
  is shown as the store spells it and left alone. That is the one fallback, and
  it is stated as one.

Two of those were not preferences but repairs. A list parsed on every keystroke
and handed back normalised cannot be a controlled field: the comma that starts
a second tag was deleted as it was typed, so a second tag could not be entered
at all, and the same was true of the newline that starts a second path. A number
parsed the same way lost its minus sign and its decimal point. What is being
typed now lives in the control until it is a value, and only then does it reach
the record.

Links are the same rule taken seriously. The engine validates every link against
the relations its type declares and rejects any other, so the relation is a
picker over exactly those — and a type that declares none says so instead of
offering a field the store would refuse.

A single choice is written at once; text is written on the same pause as the
body, because typing a tag is typing. Each patch carries only what changed, so
the panel can write a tag while somebody is still typing a paragraph.

### Removing something has two meanings, and both are offered

`Archive` is the reversible one: the record leaves the lists, keeps every link
that points at it, and comes back. It is what most of "this is no longer
relevant" actually means, so it leads in the menu.

`Delete` is a transaction nothing in the window undoes, and it opens a sheet
that first asks the project what holds on to the record. Two answers come back
and they are never treated as one:

- **What links to it.** A structural dependency: delete the target and the link
  points at nothing. The sheet offers to take those with it — one level, the
  records that link to this one, not everything that links to those. A whole
  branch deleted from one confirmation is the kind of thing nobody can undo and
  few would have chosen.
- **What mentions it in prose.** A record naming this one in its body is a
  sentence about it. Deleting the sentence's author because it mentioned
  something would delete the reasoning along with the conclusion, so mentions are
  counted, listed, and never deleted here. The sheet says plainly that they will
  name a record that no longer exists, because a memory with a dangling sentence
  is honest and a memory missing its reasoning is not.

### A command that did not happen says so

A write the store refused is reported where it was asked for — a strip above the
workspace, in the store's own words, dismissed by hand. Creating a record is one
click and one answer, and without this the answer is a button that appears to do
nothing. That is the failure mode this shell is least allowed to have, and it had
it for exactly one session: the promise was dropped on the floor and the person
was left clicking.

### A record is made beside the list it joins

Writing a record is **not** in the navigator's bottom bar, and it is **not** in
the window's title bar. Those are the two wrong answers, and each is wrong for
its own reason.

The bottom bar belongs to the source list beside it. Mail's `+` adds a mailbox,
Notes' adds a folder, Reminders' adds a list, and none of them writes what the
list contains. An earlier pass moved `+` there to writing a record, on the
argument that a project gains a claim far more often than it gains a kind of
claim. The frequency is real; the conclusion was not. That pass left the
structural command inside an overflow menu and put the frequent one in the one
place macOS never puts it.

The title bar is where an application puts the command it exists for —
composing, in Mail and Notes. **Sync is not a text editor.** A window is opened
here to read what a project knows and to see what stopped being true, not to
produce prose; a claim is written often, but it is not what the window is for,
and a control in the title bar would claim otherwise about the whole product.

What is left is the surface that lists records, and that is also where macOS
puts the command when it belongs to the content rather than to the window: the
`+` beside a list's title in Reminders. So the command is offered in three
places, and each one is the same command:

- **The workspace's own header**, beside the name of what it is showing and the
  count of it. `+` writes one of the kind the header names, and the tooltip says
  which — no list, no question. The kind was chosen in the navigator, it is
  written at the top of this column, and a menu here would be asking about the
  answer already on screen.
- **`File ▸ New <Type>`**, under `⌘N`, naming the same kind for the same
  reason. It is one command rather than a submenu of every kind: a submenu would
  ask that answered question again, and it would make `⌘N` mean whichever row it
  happened to land on.
- **A type's own row**, under the secondary button, leads with `New <Type>`.
  That is the one place in the window where the kind is named by the thing under
  the pointer, and it is how a record of a kind the workspace is not showing is
  written without visiting it first.

Where the workspace is showing a view rather than a kind — everything, or
everything that needs attention — there is no list to add to, and both the
control and the command are disabled, exactly as a native `+` is over a smart
list. Choosing a kind is what enables them, and it is one click in the column
that is already open.

`⇧⌘N` names a type, in the same menu. It is the one way to add a type while the
navigator is collapsed, and the `+` in that column's bottom bar — which acts on
the list of types, as macOS intends — is the other. Two `+` controls in one
window is not a duplication: each sits beside a different list and adds to the
one it sits beside, which is the whole of the convention.

**Sync's own type is offered nowhere.** A record of it is the record that names
the project: there is one, the project cannot be opened without it, and a second
would be a project claiming to be two. It is the same rule the archive and
delete commands already read, so it is stated once and every surface that offers
a write reads it: the kind is absent from the lists that write, and the command
on its own row is drawn and refused, because a menu missing an item explains
nothing.

The record is created empty, and the fields its type requires are filled from the
definition — the `default` it states, else the first value of an enumeration.
Nothing about the shape of a new record is this build's to choose. Its title is
empty rather than "Untitled": a stored placeholder is a word somebody has to
delete before they can write their own, and a record with no title is listed
under its key, which is a name rather than a blank.

### The one menu the shell draws itself

`/` in an empty block opens a list of blocks, filtered as more is typed. That is
the one menu in the window that is not the system's, and the exception is
narrow: it is triggered by typing inside the text, the caret has to stay where it
is while the list filters, and a native menu cannot do either.

Being drawn by hand is a reason to look like every other menu in the window, not
a licence to look like something else. It carries the surface, the corner, the
ring, the padding and the row metrics of the shell's own menus, stated in one
place — `src/components/ui/dropdown-menu.tsx` — and read from there rather than
re-chosen. It groups what it offers the way a menu on this system groups
commands that are alternatives to one another: text blocks, lists, then the
blocks that are neither, separated by a rule that is drawn from what survived
the filter rather than from the list as written.

Everything in it can also be typed as Markdown, so it is a shortcut rather than
the only way in — and each row says which Markdown, on the trailing edge, where
a menu on this system says which key. That is the same claim the rest of the
window makes about discoverability, made in the one place where the shortcut is
not a chord but a character.

Two things follow from it being a list rather than a picture of one. Its height
is decided by the window, not by a number: it may grow to the space beneath the
caret and no further, because a menu running past the bottom edge hides the rows
the arrows are about to reach. And the selection moving under the arrow keys
scrolls the box that holds it — by moving that box itself, never through
`scrollIntoView`, which would ask the document underneath to scroll too. A menu
that moved the text under it would be worse than one that did not scroll at all.

The secondary button inside text is still the system's own menu — Cut, Copy,
Paste and Select All as predefined items, the same implementations the menu bar
claims — because that gesture is the one macOS already owns.

## Colour restraint

Colour is supporting information, never layout. It appears in the keyboard
focus ring and is reserved for status and destructive actions. Selection,
hierarchy and grouping are carried by position, surface value, border, weight
and spacing, so the shell survives being read in greyscale.

## Accessibility requirements

- Native semantics. Buttons are `<button>`; Radix supplies the behaviour for
  menus, tooltips and separators. No clickable `<div>`s.
- Every icon-only control has an accessible name and a tooltip, and reports its
  state through `aria-pressed`.
- One focus indicator for the whole application, defined once as a
  `:focus-visible` outline, so focus never disappears inside a component that
  styles itself differently.
- The primary sidebar uses a roving tab index with arrow-key navigation, as a
  native source list does.
- The project sheet is a Radix dialog, so focus is trapped while it is open,
  Escape closes it, and the step's own text is its accessible description.
- Nothing is reachable only from a context menu. Every command the secondary
  button offers is also a control the keyboard can reach — for the project's
  types, the actions menu in the navigator's bottom bar; for writing a record,
  `File ▸ New` and the control in the workspace's header.
- `prefers-reduced-motion` and `prefers-contrast` are answered in the token
  layer. `prefers-reduced-transparency` is answered in two places, because the
  shell now has two kinds of translucency: the hover and selection overlays
  harden to opaque values in the token layer, and the window material itself is
  withdrawn in `src/lib/window-material.ts`. The material has to be removed
  through the window API rather than through CSS — making the surfaces opaque
  while the blur stayed underneath would satisfy the media query and ignore the
  person who set it.
- Text contrast is checked against the surface it sits on, including the
  tertiary tier used for 11 px labels.

## Why there is no code editor

A record's prose is edited in the workspace; code is not, and the two are not the
same specification.

Editor geometry, a file tree, tabs, a minimap, diagnostics and terminal
integration are a specification in their own right, and each of them would
impose constraints on the shell — gutter alignment, density, scroll ownership,
keyboard routing. Sketching them here would settle those questions by accident
and in the wrong order. `Code` is a marketplace card and nothing else until
that specification exists.

## Why arbitrary docking is deferred

The first stage gives resizable boundaries, individually collapsible panels,
sensible minimum, preferred and maximum widths, and one Reset Layout action.
It does not give drag-and-drop docking, free panel movement, multiple presets,
detached windows or persistence.

Docking is not a layout feature but a product feature: it needs a model of what
a panel *is* before a person can move one somewhere else, and that model does
not exist yet. Because the shell addresses panels only by role, that model can
be added to `shell-layout.ts` later without rewriting any shell component.

## The types belong to the project, not to the build

A new project is given exactly one type — `project` — because the record that
names the project has to have a kind the engine's strict schema knows, and
without it the project could not be created at all. Nothing else is published:
what a project is able to say is the project's decision, made in its own window
or by an agent, and opening a window is not the moment to make it on its behalf.

`project` is permanent: it is republished whenever a project lacks it, it cannot
be redefined from the window, and nothing that removes a type may offer to
remove it — deleting it would leave the record naming the project with a kind
the strict schema rejects.

That is why the navigator reads its list from the store rather than from a
constant, why the name and the mark travel inside each type's definition, and
why a corpus written by a different version of Sync is listed as it is rather
than corrected. Where a definition names neither and the kind is one Sync knows
how to describe, Sync's own are used — otherwise the eleven kinds people
recognise on sight would all arrive as the same neutral glyph under their raw
identifiers. Where it knows nothing of the kind either, the identifier is made
readable and that is stated as the fallback it is. The shell shows what the
project has.

### A name and an identifier are two different things

A type carries both, and conflating them was the first thing that had to be
undone. The **name** is what the type is called wherever a person reads it —
"Open question", several words, whatever case the project writes in. The
**identifier** is what the engine stores on every record of the type, what the
definition's key is built from, and what an agent writes: lower case, one word,
`open_question`.

The identifier is derived from the name when the type is added, and then it
stops moving. It is shown where a person is working with the type — in the form
that names it, and beside the type in the removal sheet — because it is the word
an instruction to an agent has to use, and a person configuring a type should
not have to discover it later. It is **not** in the navigator's tooltip: reading
down a list of types is not working with one, and a tooltip that recites an
identifier at somebody moving a pointer past a row is noise. It is never
editable: the store has no rename, so a field offering one would be a rewrite of
every record disguised as a text box.

The name, being nothing the store keys on, is free. Changing it changes what
every column says and touches no record.

**A name in another script is given a generated identifier, never a refusal.**
The kind alphabet is lower-case ASCII, and a project writing its knowledge in
Russian, Chinese or Arabic is a project whose type names do not survive it. So
`Открытый вопрос` is stored under something like `type_k3n8q2`, with the name
kept exactly as it was typed, and the form says in one line that the identifier
was generated and why. The alternative was transliteration, and it was rejected:
romanisation is a guess about a language rather than a fact about a string — the
same Cyrillic letter romanises differently for Russian and for Serbian — and the
guess would sit inside every record of the type for ever. Accents are not
another script: `Décision` is Latin with marks on it, so the marks come off and
the person's own word survives as `decision`.

Sync generates identifiers in the bare namespace. Extensions prefix the kinds
they bring, so an extension's types cannot collide with a project's own — which
is also why nothing in the window derives a name by parsing an identifier,
except as the last resort for a definition that never said what it is called.

### Changing a type, and removing one

Naming a type and redefining one are the same questions, so they are the same
sheet. A second screen asking any of them differently would be a second
definition of what a type is.

The sheet has two panes because there are two subjects, not two steps. **Type**
is what the thing is — name, description, mark. **Storage** is where its
records live: the engines as small cards, each carrying the sentence a person is
actually choosing between, and under them whatever the chosen one needs to be
told. Cards rather than a pop-up because the engines are not interchangeable
values of one field — each is a different bargain about visibility, ownership
and review — and a menu row would keep the word and lose the sentence. The list
is also going to grow, so a new engine is a card and a settings block rather
than a new command in the window.

Storage is answered when the type is created and never edited. A field whose
change moved data would be data loss wearing the clothes of a preference, and
the engine refuses such an edit while the kind has records; moving them is an
operation with a plan and an acknowledgement of every warning it lists. Editing
a type therefore shows the pane as a fact and says why it is not a question.

What is written is the stored definition with those three answers replaced,
never a definition rebuilt from them: a type may declare fields, relationships
or members a later engine added, and none of that is the window's to discard
while changing a sentence.

Removing a type deletes every record written as it. That is not a convenience
the window chose — the engine runs a strict schema, so
a record whose kind has no definition is one nothing can read, write or
validate, and a definition deleted on its own would strand everything written as
it. Because there is no version of this that keeps the records, the confirmation
names the number: it asks the store when it opens rather than reading the count
off the row behind it, because that count is of the last read and leaves out
whatever the window hides, and a sentence naming a number is promising the one
about to be destroyed. `project` is offered neither operation, for the reason it
is permanent.

It is two writes rather than one, and the order is what makes them safe: the
records first, the definition second. A transaction addresses a single storage
backend, and a type whose records are repository files keeps its records in one
and its definition in `refs` — so atomicity is not on offer, and asking for it
would mean a two-phase commit with no coordinator. Interrupted between the two,
the project holds a type with nothing in it, which is an ordinary state that
running the removal again finishes. The reverse order would leave the one state
that must never exist: records of a kind nothing can define.

The documents of an attached folder are not deleted with it. Memory removes the
records that point at the files and leaves the files where the team put them —
it never wrote them — and the confirmation says so before anybody agrees to
anything.

### The menu bar

A Mac application has one, and Sync's is built from Tauri's own menu API — the
same one the context menu uses, under the `core:menu` permissions `core:default`
already grants. Everything in it is a predefined item except `Settings` and the
two commands under `File`: Quit, Hide, Services, Undo, Cut, Copy, Paste, Select
All, Minimize, Close Window are the system's implementations rather than ours
wearing its labels, and the editing commands do not work fully in a webview
until a menu claims them.

`File` is where a thing is made — `New` over the project's kinds under `⌘N`, and
`New Type` under `⇧⌘N`. It is the only part of the menu that is about the open
project, so it is the only part that changes: the shell installs the menu with
both commands disabled, and the window with a project open replaces it whenever
what it *says* changes — the kinds it lists, or which of them the navigator has
selected. A re-render is not a change: the commands are read at the moment one
is chosen, so the menu is rebuilt from what the window says rather than from how
often it drew itself. A window that can make nothing shows the commands disabled
rather than dropping them, because a menu whose items come and go teaches nobody
where a command lives.

It replaced two `keydown` handlers that answered `⌘,` and `⌘W` themselves. They
fired inside text fields, appeared nowhere a person could look, and were the
kind of thing that makes an application feel like a web page in a window.

### Secondary click

A context menu is the one gesture in the window the system already owns, so it
is the system's own menu — `@tauri-apps/api/menu`, which Tauri carries itself,
under the `core:menu` permissions `core:default` already grants. No plugin, no
capability, nothing new in the dependency list. A web menu in its place would be
the one part of the window that announces it is a webview: wrong font, wrong
metrics, wrong dismissal, and none of the keyboard behaviour the rest of the
system has.

**No command in the shell carries an ellipsis** — not in this menu, not in the
bottom bar, not on the button that opens a folder. The system's convention marks
the commands that ask for something before they happen, and it was applied here
first, and it was removed. A window this small does not have enough commands
for the distinction to earn its punctuation, and half of them carrying a mark the
other half does not reads as an inconsistency long before it reads as a
convention. An ellipsis still means one thing here, and only one: an action in
progress — `Opening…`, `Deleting…`, `Reading the project's types…`.

Two rules keep it honest. Where there is no native menu to show — a browser
during development — the event is left alone, because suppressing the system's
own menu to then show nothing is worse than either menu. And nothing is reachable
only from it: a menu that opens under the pointer is invisible to the keyboard,
so the same two commands sit in the navigator's bottom bar, beside the one that
adds a type, and the `New <Type>` that leads a type's own menu is `⌘N` in the
menu bar. The bar's menu is the shell's own rather than a native one because
its neighbour there already is — one bar, one kind of menu.

## The sidebar lists one section, and that is the point

An area is a section that exists now. The sidebar has one — `Records` — and
that is the honest count: it is the only part of the product with something to
show, so the window reads as one thing rather than as a table of contents for a
product that is not there.

Everything else a project might do arrives as an extension, and the catalogue is
the marketplace in the `Extensions` area — also shown, as a chooser, while a
project is being opened. That is a different claim from a sidebar item. A
sidebar item says *this is a part of this window*; a marketplace card says
*this is something a project could install*.

**The catalogue lists only what can be installed.** An earlier draft carried
cards for what had not shipped, on the argument that naming what is coming says
what the product is for. It does — to us. To somebody deciding what their
project should do, five cards they cannot choose are five refusals to a question
they asked in good faith. What is coming belongs in release notes.

The rule that follows: an area is a section with a screen behind it. Anything
else is a marketplace entry.

`Extensions` is the second area, and it obeys that rule: it has a screen — the
catalogue, and what each entry would install. It is pinned to the foot of the
column because it is not a section of the project. The sections grow above it as
extensions install them.

The Community relationship model is a design contract for an extension, not a
description of anything in the window.

### The order of the sections is the reader's, and it stays on their machine

The sections arrive in the order the project's record declares its extensions,
which is the order they were installed in. That is a starting point and not a
statement: a person works in one section every day and visits another twice a
month, and which of those is at the top is worth a drag.

So the rows are dragged, the way a favourite is dragged in Finder's sidebar and
a mailbox in Mail. The pinned row is not: it is pinned, and being able to carry
`Extensions` away from the foot of the column would be the interface
contradicting its own rule in the one gesture that tests it.

**The rows do not part.** The row being carried stays where it is and goes
quiet, and a two-pixel line appears on the seam it would land on — which is the
gap the rows already sit two pixels apart by, so nothing moves to make room for
it. That is what macOS source lists do, and it is the same rule the navigator's
tree already keeps: a list whose rows shift under the pointer is a list you
cannot aim at. The line is drawn in the tier the badges use and carries no
colour, because where something will land is neither status nor destruction.

The keyboard does it with `⌥↑` and `⌥↓` on the selected section, and there is no
mode to enter. Space and the plain arrows are already spent here — selection
follows focus in a source list, so a row is selected by arriving at it — and a
gesture that took the arrows would leave a column that can be rearranged and not
walked.

**Where they end up is stored on this Mac, per project, beside the type
filter.** Rearranging a column changes nothing about what the project holds, so
it is the same kind of decision as hiding a type and it goes to the same place:
one person deciding how they work, not a fact travelling to a colleague through
`refs/memory/*`. What is stored is the keys somebody arranged, not the column: a
section installed since has never been placed and joins at the foot, and a key
whose extension failed to run this launch keeps its place rather than being
forgotten on its behalf.

## Opening a project

The window has two states and one fact decides which: whether a project is
open. With none, the slab holds the header and one centred block — what the
window is for, the action that leads out of it, and the projects opened before.
There is no sidebar listing sections of a project that does not exist. The
recent list appears only when there is one: an empty "Recents" heading on a
first launch would name a feature instead of showing one.

A project is a Git repository. That is a product rule, not a convenience: the
store keeps its knowledge in the repository's own refs, so a folder outside
version control has nowhere to put any of it. The flow asks as little as it can
get away with, in the only order the questions can be asked:

1. **Can this folder hold a project?** A folder already in a repository passes
   straight through. One that is not is told why it cannot be opened and
   offered `git init`. Declining ends the flow — which is why the reason is on
   that screen and not on a second one that appears after the refusal. Being
   told twice is an argument, not an interface.
2. **Has it been opened before?** A repository whose memory already carries a
   project record answers for itself, and the flow ends here without showing a
   single field. An application that asks a question it already has the answer
   to is not being careful; it is forgetting.
3. **What is it?** Name, an optional description, and the language the project
   writes its knowledge in — asked once, for a project that has never existed.
   The language belongs to the project rather than to the person reading it:
   claims and documents travel through the repository, and a store that mixes
   languages is a store nobody can search.
4. **What can it do?** The extension marketplace, with Records installed and
   not removable, and the rest shown unavailable.

### Where the answers go

**A project's settings belong to the project, not to this Mac.** They are
written as one record in the repository's own memory — the same place the rest
of what the project knows lives — so the same repository opened on another
machine is the same project rather than a folder that has to be described
again. That write is also what creates the memory: the first transaction is
what makes `refs/memory/*` exist, so there is no separate "set up memory" step
to forget.

The one thing kept on this machine is which projects were opened recently. That
is genuinely a fact about the installation and not about any project, it is a
path and a name, and losing it costs a shorter menu.

Both places a project is offered — the empty window and the project switcher —
list it under its path. Two checkouts of the same repository have the same name
and are not the same project, and the path is the only thing on the row that
ever says so.

Two failures are stated rather than absorbed. If memory cannot be read, the
flow still asks — there is nothing else it can do — but says so on the same
screen, because silently re-describing a project that already exists would
overwrite what it knew. If the answers cannot be written, the first attempt
reports the failure and stops; pressing the button again opens the project
anyway. That is a decision the person makes, not a fallback taken on their
behalf.

### Sheets

A modal that configures *this* window is a sheet. It slides out from under the
title bar rather than floating in the middle of the screen, the title bar stays
where it was, and the scrim dims the slab and not the frame — the frame is the
window's edge, not its content, and dimming it would say the desktop was modal
too.

That is the only kind of modal the shell has. Anything that is not about the
window itself does not earn one.

## Settings are a window, not a sheet

A sheet configures the window it slides out of. Settings do not: which agents
this Mac connects to Sync, and which extensions it has, are true of the
installation and of every project opened in it, including none. On macOS that is
a window — the one `⌘,` opens in every native application — so that is what it
is, opened by the shortcut and by one control in the header, because a command
reachable only from a shortcut is a command nobody discovers.

It is the same document as the main window, told apart by the window's label,
and it holds a source list beside a column of settings at the density of the
rest of the application. Two rules keep it from becoming a second product:

- **No frame, no slab, no material.** The material is the main window's edge;
  a second window wearing it would make glass a theme rather than a detail.
- **The system's title bar, and nothing restating it.** The window is called
  Settings by the system, so no header inside it says so again.

It has one section, and that is the honest count. Agents are listed with the
file each keeps its connections in and the words "Not connected", because Sync
publishes no interface for them yet — and the control that would connect one is
present, inert and says why, the way the header's search field does.

**Extensions are not settings.** An extension installs the types a project
stores, the scripts it runs and the screen it renders, and the project declares
what it depends on, so it is chosen with a project open and from that project's
own window. Putting it here would have made what a project can do a property of
this Mac.

## Extensions are an area, and they read in three columns

Selecting `Extensions` deselects whatever was selected, exactly as choosing any
other area does, and all three columns become about extensions. The window never
shows two subjects at once — a navigator listing types of record beside a
workspace showing a catalogue would be two answers to two questions nobody
asked together.

- **The navigator** carries `Marketplace` as its first row and then the group
  `Installed` — nothing else. A group with nothing in it is not drawn, because
  an empty heading names a state instead of showing one, so a project that has
  installed nothing shows one row and that row is the way in.
- **The workspace** is the marketplace, or one extension. The marketplace lays
  every entry out as a card; an extension's page is what it does, what it adds
  to this window, the types it would publish — each with the identifier every
  record of it will carry — and what it tells a connected agent. Installing has
  to be a decision made with all of that in view, so all of it is on one
  surface, which is why a card opens the page rather than offering a button.
- **The context panel** is the package rather than the product: identifier,
  version, status, how many types, what it requires, where the dependency is
  declared, and the two different things removing it can mean. It is empty while
  the marketplace is open — that column describes one thing, and the marketplace
  is about a set.

**A row and a card are two different claims, and that is what decides where
something goes.** A row in the navigator says *this is a part of this window*; a
card says *this is something a project could install*. An earlier arrangement
listed every unpacked package as a row, grouped by where it came from, and it
read — correctly — as the window claiming sections it did not have. So the list
is now what the project actually runs: declared by it, answered by a package,
and runnable in this build. Everything short of that is a card.

What each card describes is what a project would be agreeing to: the types it
would publish, what it adds to the window, what it tells an agent. **Every entry
states where it stands, wherever it appears**, and the four states are all
reachable in ordinary use: in this project; unpacked and never asked for; asked
for and absent; and unpacked and refused by this build.

The last of those is why nothing is filtered out of the marketplace, which
reverses the earlier rule that only what can be installed is listed. That rule
is right about a product that has not shipped — five cards nobody can choose are
five refusals to a question asked in good faith — and wrong about a package
already on the disk: it is theirs, it takes up room, and removing it is
something they may want to do. It survived only while a second panel listed the
disk in full, and that panel is gone.

**The two sources a package arrives from are icons in the navigator's bottom
bar**, by rule 9: they act on the list above them, so they belong in the band
macOS keeps for exactly that, on the leading edge. They are not in a header —
a header carries only the command that writes into the thing it names — and not
in the inspector, which is where they were until 2026-08-24 and where they made
the only door in the application open from a room nobody could reach.

## A folder is a place, and a place has to look like one

The navigator draws types and the folders under them as **one tree**, not a list
with trees hanging off it. They are one hierarchy, so they are one control: a
single tab stop, one set of arrow keys, and no seam a person can fall into
between a type and its own directories. Disclosure is all-or-nothing across the
tree — reserving the triangle only where a sibling has children makes a child
sit left of its own parent's siblings, and a tree that indents backwards is
worse than one that indents for nothing.

**A directory nothing is filed in is still drawn**, quieter. It is on disk, a
person sees it in Finder, and it is somewhere they can file into; leaving it out
would make Sync disagree with the file tree beside it. The count on a folder row
is the documents of that type filed *directly* in it — not the subtree, and not
the record that is the folder, so it is the same number as the rows the
workspace then shows.

A folder can say what it is for. That is an ordinary document filed in it,
carrying `is_folder`, reached from a strip above the list rather than from a
verb in a menu: somebody looking at a folder reads what it is for, and somebody
who wants to say what it is for writes it there. Nothing had to learn about
folders for that text to be searched.

**Deleting a folder takes everything filed under it, whatever its type**, and
the sheet says so before it acts. A folder exists while something is in it, so
sparing another type's records would empty it rather than delete it. The number
in that sentence is asked of the store, never counted from the tree — a
confirmation may not name a figure it guessed.

## Synchronisation is the whole project's, so it lives in the title bar

Memory being in step with its remote is true of every column at once, which puts
it in the band that already holds the project switcher rather than in any one of
them. It is a word, not an icon with a badge: colour here is reserved for status
and destructive actions, and a count in a dot is something you have to learn to
read.

**Silence is a state.** A project in step with its remote shows nothing at all
and has no control there — the same silence the band above the workspace keeps
for a record nobody has typed into. What appears, appears because a person would
want to know it: their writing is only here, or somebody else's is not here yet.

The one coloured state is the one where a decision was taken on somebody's
behalf — a merge that kept this side where a colleague had changed the same
thing. The sheet then names what it cost, member by member, and offers to undo
the fetch. Reporting it is the point: nothing is lost, both versions are
commits, but a person whose colleague's sentence quietly vanished is owed the
news.

## Criteria for changing the shell

Change the shell when a real vertical slice needs something it cannot express,
and bring the evidence with the change:

- The change is driven by content that actually exists, not by a mockup.
- It does not reduce the workspace's share of the window.
- It survives the greyscale test: structure still reads with colour removed.
- It holds in both appearances, at 1024 × 700 and at full screen, and with
  reduced motion, increased contrast and reduced transparency.
- It adds no token that an existing token already covers.
- It leaves every panel role addressable by name.

A change that only makes a screenshot look richer is not one of these.
