/**
 * What an extension sees of this application.
 *
 * An extension renders inside the window's own columns, with the window's own
 * components, and it reaches none of them directly: everything it may use is
 * named here, and everything not named here is the shell's private business.
 * That is the whole of the boundary — there is no second mechanism, no escape
 * hatch and no "internal but stable" tier.
 *
 * The rule this exists to make enforceable: an extension imports from this
 * module and from nowhere else in the application. A lint rule holds the other
 * half of it, because a boundary that depends on remembering is not one.
 *
 * Two things are deliberately absent and will stay absent:
 *
 * - **Geometry.** Panel widths, collapse thresholds and the rules that release
 *   space before claiming it live in `lib/shell-layout.ts` and are the window's.
 *   An extension declares which frame its area uses and fills that frame's
 *   slots; it never learns a pixel. One extension able to pin a column open
 *   would change the window's behaviour for every other.
 * - **The shell's own screens.** Nothing that draws the project switcher, the
 *   sidebar or the settings window is reachable from here. An extension
 *   contributes to those through declared contribution points, or not at all.
 *
 * One import is allowed beside this module: `lucide-react`. Icons are a shared
 * library rather than a surface of the host, and re-exporting several hundred
 * of them through here would say otherwise. The host guarantees a single copy
 * by marking it external when an extension is built.
 *
 * Today every export is a re-export of something the shell already had, and
 * nothing in the application imports this module yet. That is the point of the
 * first step: the boundary exists before anything crosses it, so the pieces
 * that move across can be moved one at a time with somewhere to arrive.
 */

// ---------------------------------------------------------------------------
// What this build is, as a package checks it.
//
// First in the file because it is what decides whether the rest of it is
// reachable: the host reads a manifest's range against `SYNC_API_VERSION`
// before it executes a line of the package, so an extension written for a
// surface this build does not publish contributes nothing and says why.
// ---------------------------------------------------------------------------

export {
  SYNC_API_VERSION,
  SYNC_CAPABILITIES,
  refuseIncompatible,
  supportsApiRange,
  isVersion,
  type ApiRequirement,
  type SyncCapability,
} from "@/lib/extension-api/version";

// ---------------------------------------------------------------------------
// Panels — the surfaces a column is built from.
//
// A frame decides which columns exist; these are how the inside of one is laid
// out, at the header height and footer band the rest of the slab uses.
// ---------------------------------------------------------------------------

export {
  PanelSurface,
  PanelHeader,
  PanelBody,
  PanelFooter,
  PanelPlaceholder,
} from "@/components/shell/panel";

export { SourceList, type SourceListItem } from "@/components/shell/source-list";

/**
 * The source list's sibling, for a list that nests.
 *
 * The behaviour underneath it is a library; the markup is the shell's. That is
 * the whole reason it is exported as a component rather than as a hook: an
 * extension gets the window's rows, the window's selection and the window's
 * keyboard, and the library can be replaced without any of them noticing.
 */
export { SourceTree, type SourceTreeItem } from "@/components/shell/source-tree";

/**
 * The filter over which of the project's types a window shows.
 *
 * Here rather than in any one area because the preference is the project
 * view's: the navigator and the search palette mount the same control over the
 * same stored fact, and a second implementation of it would be a second answer
 * to a question with one.
 */
export { TypeFilter } from "@/components/shell/type-filter";

// ---------------------------------------------------------------------------
// Typed marks — the shell's signature detail.
//
// A kind's mark and a claim's state are drawn one way across the window. An
// extension that drew its own would be recognisable as foreign at a glance,
// which is the one thing the visual system is built to prevent.
// ---------------------------------------------------------------------------

export {
  KindGlyph,
  KindMark,
  StateMark,
  kindIcon,
  FRESHNESS_STATES,
} from "@/components/shell/entity-marks";

// ---------------------------------------------------------------------------
// The component library, as the application vendored it.
//
// Not "the same components" by convention — the same objects. An extension
// bundling its own Radix would ship a second copy of every portal, focus trap
// and scroll lock in the window.
// ---------------------------------------------------------------------------

// `buttonVariants` beside the component because the component's own props are
// typed from it, and because a link that has to look like a button is a real
// case — one that would otherwise be answered by copying the class list.
export { Button, buttonVariants } from "@/components/ui/button";
export { ScrollArea, ScrollBar } from "@/components/ui/scroll-area";
export {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
export {
  DropdownMenu,
  DropdownMenuPortal,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
} from "@/components/ui/dropdown-menu";
export {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";

// ---------------------------------------------------------------------------
// A record, read and edited.
//
// An extension opens a record; it does not draw one. The editor and the panel
// of what is true about a record are the corpus's, and every extension's
// records are read and written through the same two surfaces — which is why an
// extension's screen is thin and stays thin.
// ---------------------------------------------------------------------------

export { DocumentView } from "@/components/shell/document-view";
/**
 * The reading view for stored Markdown, and the seam for growing it.
 *
 * The same renderer the shell reads a record with, so prose in an extension and
 * prose in a record are one document rather than two. A plugin replaces the
 * drawing of a block and cannot change how a body is split into them: parsing
 * has to keep agreeing with the editor, and a plugin able to invent block kinds
 * would be a second Markdown dialect in one window.
 */
export {
  Markdown,
  type MarkdownBlock,
  type MarkdownPlugin,
} from "@/components/shell/markdown";
export { RecordMetadata } from "@/components/shell/record-metadata";
export { ContextInspector } from "@/components/shell/context-inspector";

// ---------------------------------------------------------------------------
// Native gestures.
//
// Secondary click opens a system menu, never a web one — a menu drawn in the
// document announces that the window is a webview.
// ---------------------------------------------------------------------------

export {
  showNativeContextMenu,
  type NativeMenuItem,
  type NativeMenuEntry,
  type NativeEditingCommand,
} from "@/lib/native-menu";

// ---------------------------------------------------------------------------
// The corpus: what it is made of, and how it is read and written.
//
// One hook answers for the whole store. An extension asks for the selection it
// cares about — a kind of its own, a freshness, everything — and gets the
// records, the counts over the whole corpus, the types, whether more was left
// unread, and why the store did not answer if it did not. Writes are on the
// same object, because a write changes what the read returned and the two
// disagreeing is the bug this shape prevents.
// ---------------------------------------------------------------------------

export {
  useCorpus,
  typeName,
  explain,
  ATTENTION_STATES,
  PAGE_LIMIT,
  type Corpus,
} from "@/lib/memory/use-corpus";

export type { MemorySelection } from "@/lib/memory/client";

export {
  PROJECT_KEY,
  absenceLabel,
  isAttachedType,
  typeOfLocator,
} from "@/lib/memory/types";

export type {
  MemoryType,
  MemoryRecord,
  MemoryDocument,
  MemoryCounts,
  MemoryView,
  DocumentPatch,
  Dependent,
  Dependents,
  EntityLink,
  FieldDeclaration,
  RelationshipDeclaration,
  TypeStorage,
  Freshness,
  Presence,
} from "@/lib/memory/types";

export {
  useDocument,
  type OpenDocument,
  type DocumentDraft,
  type SaveState,
} from "@/lib/memory/use-document";

// ---------------------------------------------------------------------------
// The confirmations that write to the corpus.
//
// Removing a record or a type destroys data, and what has to be said before
// that happens — what links to it, how many records go with it — is the
// corpus's answer rather than any one area's. An extension opens these; it does
// not draw its own version of them.
// ---------------------------------------------------------------------------

export { RecordRemovalSheet } from "@/components/shell/record-removal";
export { TypeRemovalSheet } from "@/components/shell/type-removal";
export { TypeSheet } from "@/components/shell/type-sheet";
export { FolderSheet } from "@/components/shell/folder-sheet";
export { FolderRemovalSheet } from "@/components/shell/folder-removal";
export { MoveArea } from "@/components/shell/move-area";
export { useDragHandle } from "@/components/shell/move-area";
export { UnmatchedFiles } from "@/components/shell/unmatched-files";

export { updateMemoryDocument } from "@/lib/memory/client";
export type { TypeDefinition } from "@/lib/memory/client";
// What a write answers with. It is returned by half the calls above, so an
// extension that holds the answer needs the name of what it is holding.
export type { TransactionResult } from "@/lib/memory/types";

// The hierarchy, which is a name and never a location. Reading it is one call
// rather than something derived from a page of records: an empty directory of
// an attached folder is in no record and would be missing from a tree built
// that way, while a person sees it in Finder.
export {
  memoryFolders,
  createMemoryFolder,
  describeMemoryFolder,
  deleteMemoryFolder,
  memoryFolderToll,
  renameMemoryFolder,
  moveMemoryDocument,
} from "@/lib/memory/client";
export type { MemoryFolder } from "@/lib/memory/client";

export {
  useFolders,
  foldersUnder,
  folderName,
  parentFolder,
  type Folders,
} from "@/lib/memory/use-folders";
export type {
  ScanChange,
  ScanOutcome,
  // Carried by a `ScanChange` for an unmatched file: which records it might be.
  RenameCandidate,
} from "@/lib/memory/types";

// ---------------------------------------------------------------------------
// What an extension implements.
//
// The one part of the surface that points the other way: everything else here
// is something the window hands over, and this is the shape of what comes back.
//
// A vocabulary is deliberately not here. A type used to be declared in an
// extension's TypeScript and reached the project through a constant in the
// shell; it is now a JSON file inside the package, read by the host and
// forwarded to the engine untouched. There is nothing left for an extension's
// code to say about its types, so there is nothing here for it to say it with.
// ---------------------------------------------------------------------------

export type {
  ActivationResult,
  AreaModule,
  AreaProviderProps,
  ExtensionHost,
  ExtensionNet,
  ExtensionTerminal,
  ExtensionVault,
  NetMethod,
  NetPart,
  NetRequest,
  NetResponse,
  TerminalEvent,
  TerminalOpening,
  TerminalRow,
  TerminalSize,
} from "@/lib/extension-api/contract";

// ---------------------------------------------------------------------------
// What an area contributes to the window beyond its columns.
//
// The menu bar belongs to the application and is filled by whichever area is
// selected; an area that is mounted but not selected passes `false` and keeps
// its hands off it. The table commands are the channel the other way: the
// record editor reports what the caret can do, and the area passes it to the
// menu — the caret is several components below anything that could ask.
// ---------------------------------------------------------------------------

export {
  useAppMenu,
  type WindowCommands,
  type MenuRecordType,
} from "@/lib/app-menu";

// ---------------------------------------------------------------------------
// What the window asks an area to show.
//
// An area keeps its own selection for as long as the window is open, so there
// is no way in from outside except this one: search hands an area an intent and
// the area decides what reaching it means. It carries what to show and nothing
// about how, which is the only form of it that can mean anything to an area
// this build has never seen.
// ---------------------------------------------------------------------------

export type { AreaIntent } from "@/lib/area-intent";

// ---------------------------------------------------------------------------
// What a section says about its own row.
//
// The half of a badge a manifest cannot express. A manifest declares a count
// over the corpus and the host answers it without running a line of the
// package, which is what makes a badge work for a section nobody has opened;
// this is for what only a running section knows — an agent's reply that landed
// while somebody was in another section is nowhere in the corpus.
//
// Reporting nothing is not a report: the declared count goes on showing
// through, so a section can have both and decide for itself which of the two
// its row should say.
// ---------------------------------------------------------------------------

export { useBadge, type BadgeReport } from "@/lib/extension-api/badge";
export {
  TableCommandsProvider,
  type TableCommands,
} from "@/lib/editor/table-commands";

// ---------------------------------------------------------------------------
// The project, and how it is being looked at.
//
// Which kinds are hidden is a preference over the whole corpus and belongs to
// the frame rather than to any area: every area drawn with a list gets it, and
// a person sets it once instead of once per section.
// ---------------------------------------------------------------------------

// `OpenProject` extends `ProjectSettings`, so the half of a project that is its
// own description has to be nameable too — and with it the two things that
// description is made of. `InstalledExtension` is what a project declares a
// dependency as; an extension reading the composition it is part of holds one,
// and `ToolDeclaration` is what one of those carries about the tools it offers.
export type {
  OpenProject,
  ProjectSettings,
  ProjectLanguageId,
  InstalledExtension,
  ToolDeclaration,
} from "@/lib/project/types";
// The list the id is drawn from, because the id is derived from it: a type
// defined as "one of these" cannot be named without the these.
export { PROJECT_LANGUAGES } from "@/lib/project/types";
/**
 * Where this project's code came from, as `origin` names it.
 *
 * The one thing on this surface that answers *which repository is this*, and it
 * answers it in git's own words rather than in anybody's product's. A section
 * that reads a forge decides for itself whether that URL is one it knows; this
 * build does not know what a forge is, which is the same rule that keeps an
 * extension's name out of the core.
 *
 * `null` is a repository nobody has given an `origin`. That is an ordinary
 * state and it is the project's to fix — in the project, with git — not a
 * question for a section to ask on its behalf.
 */
export { projectRemote } from "@/lib/project/client";
export {
  useProjectView,
  type ProjectViewState,
} from "@/lib/project/use-project-view";

// ---------------------------------------------------------------------------
// Agents.
//
// The narrower of the two meanings of the word this application uses. In
// settings an agent is a client that connects **to** Sync and has our server
// written into its configuration; here it is a process Sync **drives** over
// ACP. The two sets overlap and are not the same: Claude Desktop, Cursor, VS
// Code and Zed can be connected to and can never be driven, because they are
// applications rather than processes with a protocol on their standard input.
//
// A session belongs to the application, not to the screen that opened it. An
// extension may run several at once, and unmounting its area stops the watching
// and nothing else: the agents go on working, and coming back re-subscribes and
// is handed everything that happened in between. That is also why stopping one
// is a command rather than a cleanup — a process a person started on their
// behalf is theirs to end, from a list that outlives the screen.
//
// What crosses this line is the protocol's own shape, not a canon of ours. An
// update no build has a reading for still arrives, marked as unread, because
// the agents already disagree about what they emit and will disagree further.
// `foldTranscript` is the reading, and it is a library rather than a rule: what
// ends one message and starts the next is not in ACP at all, so an extension
// that wants a different answer can write one over the same events.
// ---------------------------------------------------------------------------

export {
  useAgents,
  useLiveSessions,
  type Agent,
} from "@/lib/agent-sessions/use-agents";
// Where a conversation is held, for a section that offers the choice. The
// window itself makes no use of these: it opens no conversations and lists
// none, so a tree is only ever chosen from a screen a package drew.
export {
  WorktreeError,
  adoptWorktree,
  discardWorktree,
  worktreesIn,
  type Worktree,
  type WorktreeChoice,
} from "@/lib/worktrees/client";
export {
  useAgentSession,
  startSession,
  stopSession,
  deleteSession,
  type AgentSession,
} from "@/lib/agent-sessions/use-session";
export {
  SessionError,
  chooseAttachments,
  conversationForRecord,
  conversationKeptAs,
  forgetRememberedConversation,
  rememberedConversations,
  renameSession,
  resumeSession,
  imageFileName,
  saveSessionImage,
  sessionBacklog,
  sessionImage,
  type AdapterState,
  type AgentDescriptor,
  type OpenedSession,
  type PastedContent,
  type PastedImage,
  type PermissionRequest,
  type RememberedConversation,
  type SessionConfigOption,
  type SessionConfigValue,
  type SessionEvent,
  type SessionMode,
  type SessionModeState,
  type SessionRow,
  type SentImage,
  type SessionAbout,
  type SessionSource,
  type SessionStatus,
} from "@/lib/agent-sessions/client";
/**
 * Showing a record a section does not own.
 *
 * A section reads what a conversation is about, and what it is about is a
 * record another section shows. Without this the only way to reach it is a link
 * inside a body, which is not where a heading is.
 */
export { useOpenRecord } from "@/lib/record-link";
export {
  EMPTY_TRANSCRIPT,
  PAUSE_MS,
  foldTranscript,
  modelOption,
  usageLines,
  withDropped,
  type Entry,
  type Usage,
  type OpenQuestion,
  type Transcript,
} from "@/lib/agent-sessions/transcript";

// ---------------------------------------------------------------------------
// The class-name helper, because every component above expects the classes it
// produces and a second implementation would merge Tailwind conflicts
// differently.
// ---------------------------------------------------------------------------

export { cn } from "@/lib/utils";
