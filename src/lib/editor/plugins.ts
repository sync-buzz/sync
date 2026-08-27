"use client";

/**
 * What a record's body is allowed to be.
 *
 * The store holds Markdown, so this list is not a menu of everything the editor
 * library can do: it is the set of blocks that survive being written back. A
 * plugin whose block has no Markdown spelling would be a block that disappeared
 * the next time the record was opened, which is worse than not offering it.
 *
 * Two consequences worth stating, because both look like omissions:
 *
 * - **Every heading level is registered, including the ones nothing offers to
 *   insert.** A body may already hold `#####`, and a level with no plugin is
 *   read as a paragraph — the words would survive and the structure would not.
 * - **There is no colour, no font, no alignment and no image.** None of them are
 *   Markdown, so none of them are content this store can keep.
 *
 * Lists are the indent flavour rather than nested `<ul>`s: any block can carry a
 * list style, which is what lets Tab and Shift-Tab move an item, and it is the
 * flavour that round-trips a task list's checkbox. The classic flavour drops it.
 */

import {
  BlockquoteRules,
  BoldRules,
  CodeRules,
  HeadingRules,
  HorizontalRuleRules,
  ItalicRules,
  MarkComboRules,
  StrikethroughRules,
} from "@platejs/basic-nodes";
import {
  BlockquotePlugin,
  BoldPlugin,
  CodePlugin,
  H1Plugin,
  H2Plugin,
  H3Plugin,
  H4Plugin,
  H5Plugin,
  H6Plugin,
  HorizontalRulePlugin,
  ItalicPlugin,
  StrikethroughPlugin,
} from "@platejs/basic-nodes/react";
import { CodeBlockRules } from "@platejs/code-block";
import { CodeBlockPlugin, CodeLinePlugin } from "@platejs/code-block/react";
import { IndentPlugin } from "@platejs/indent/react";
import { ImagePlugin, PlaceholderPlugin } from "@platejs/media/react";
import { LinkRules } from "@platejs/link";
import { LinkPlugin } from "@platejs/link/react";
import { BulletedListRules, OrderedListRules, TaskListRules } from "@platejs/list";
import { ListPlugin } from "@platejs/list/react";
import { MarkdownPlugin } from "@platejs/markdown";
import { SlashInputPlugin, SlashPlugin } from "@platejs/slash-command/react";
import {
  TableCellHeaderPlugin,
  TableCellPlugin,
  TablePlugin,
  TableRowPlugin,
} from "@platejs/table/react";
import { isUrl, KEYS, TrailingBlockPlugin } from "platejs";
import { ParagraphPlugin } from "platejs/react";

import {
  Blockquote,
  Bold,
  CodeBlock,
  CodeLine,
  heading,
  HorizontalRule,
  Image,
  InlineCode,
  Italic,
  Link,
  ListWrapper,
  Paragraph,
  Strikethrough,
  Table,
  TableCell,
  TableCellHeader,
  TableRow,
} from "@/components/editor/nodes";
import { LinkToolbar } from "@/components/editor/link-toolbar";
import { PictureDrop } from "@/components/editor/picture-drop";
import { SlashMenu } from "@/components/editor/slash-menu";
import { MARKDOWN_OPTIONS } from "@/lib/editor/markdown";
import { isProjectPath, RECORD_SCHEME, recordTarget } from "@/lib/record-link";

/** The blocks a list style can be attached to. A list item is a block. */
const LIST_TARGETS = [...KEYS.heading, KEYS.p, KEYS.blockquote, KEYS.codeBlock];

export const EDITOR_PLUGINS = [
  ParagraphPlugin.withComponent(Paragraph),

  // The Markdown spelling of a block is also the fastest way to type one, and
  // it is the spelling the corpus is already written in: `## `, `- `, `1. `,
  // `> `, ``` and `**bold**` become the block or the mark as the space is typed.
  H1Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(1) },
    shortcuts: { toggle: { keys: "mod+alt+1" } },
  }),
  H2Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(2) },
    shortcuts: { toggle: { keys: "mod+alt+2" } },
  }),
  H3Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(3) },
    shortcuts: { toggle: { keys: "mod+alt+3" } },
  }),
  H4Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(4) },
  }),
  H5Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(5) },
  }),
  H6Plugin.configure({
    inputRules: [HeadingRules.markdown()],
    node: { component: heading(6) },
  }),

  BlockquotePlugin.configure({
    inputRules: [BlockquoteRules.markdown()],
    node: { component: Blockquote },
  }),
  HorizontalRulePlugin.configure({
    inputRules: [
      HorizontalRuleRules.markdown({ variant: "-" }),
      HorizontalRuleRules.markdown({ variant: "_" }),
    ],
    node: { component: HorizontalRule },
  }),

  BoldPlugin.configure({
    inputRules: [
      BoldRules.markdown({ variant: "*" }),
      BoldRules.markdown({ variant: "_" }),
      MarkComboRules.markdown({ variant: "boldItalic" }),
    ],
    node: { component: Bold },
  }),
  ItalicPlugin.configure({
    inputRules: [
      ItalicRules.markdown({ variant: "*" }),
      ItalicRules.markdown({ variant: "_" }),
    ],
    node: { component: Italic },
  }),
  StrikethroughPlugin.configure({
    inputRules: [StrikethroughRules.markdown()],
    node: { component: Strikethrough },
  }),
  CodePlugin.configure({
    inputRules: [CodeRules.markdown()],
    node: { component: InlineCode },
  }),

  CodeBlockPlugin.configure({
    inputRules: [CodeBlockRules.markdown({ on: "match" })],
    node: { component: CodeBlock },
  }),
  CodeLinePlugin.withComponent(CodeLine),

  // A link is written — `[text](url)` — or made from selected words with `⌘K`,
  // which opens a field that searches the project's own records. The field
  // waited until a link had somewhere to go; now that following one opens the
  // record it names, it is the way most links are made.
  //
  // Two things have to be told about `sync://`, and missing either one is a
  // link that silently is not one:
  //
  // - **The allowed schemes.** The plugin sanitises a url against that list and
  //   strips a scheme it has not been told about — every record-to-record link
  //   would become plain text the first time its record was opened. The other
  //   four are the plugin's own default, restated because naming the list
  //   replaces it.
  // - **What counts as a url.** The plugin's `isUrl` answers no to both of the
  //   spellings a link inside this project uses — `sync://decision/d-1` and
  //   `./setup.md` — and it is consulted twice over: the input rule refuses a
  //   link typed by hand, and `upsertLink` refuses to insert one at all. The
  //   second is the quiet one: the control that inserts a link would have
  //   written nothing and reported nothing.
  //
  // Widening it to any relative path is not a loosening of anything. Markdown
  // already says `[text](anything)` is a link; what decides whether one goes
  // anywhere is `targetOf`, and that is asked when the link is drawn.
  LinkPlugin.configure({
    inputRules: [LinkRules.markdown()],
    node: { component: Link },
    // Where the plugin itself puts its panels: after the editable, inside the
    // container it renders. Hung beside the editor instead, they are outside
    // what the plugin considers its own surface — which is the difference
    // between a panel that appears over the words and one that does not.
    render: { afterEditable: LinkToolbar },
    options: {
      allowedSchemes: ["http", "https", "mailto", "tel", RECORD_SCHEME],
      isUrl: (text: string) =>
        isUrl(text) || recordTarget(text) !== null || isProjectPath(text),
    },
  }),

  // A picture. `![alt](url)` is Markdown, so it is content this store can keep —
  // which is the whole test — and it round-tripped through the serialiser before
  // anything here drew one: a record holding a picture opened, saved correctly
  // and showed a gap.
  //
  // The node is the media package's, and the drawing is ours because the url is
  // a path inside the repository rather than something a browser can fetch.
  ImagePlugin.withComponent(Image),

  // What catches a picture pasted from the clipboard or dropped on the text.
  // The gesture, the type and size checks, and the placeholder are all the
  // plugin's; where the bytes belong is the one part it cannot know, and that
  // is what `PictureDrop` answers.
  //
  // Images only. Video and audio would each need a way to play them, which is a
  // protocol this window does not have, and a placeholder that accepted a file
  // it could never show would be an invitation to lose one.
  //
  // What is deliberately not installed beside these: captions and resizing.
  // Neither has a Markdown spelling, so a caption typed under a picture would
  // be gone the next time the record was opened.
  PlaceholderPlugin.configure({
    node: { component: PictureDrop },
    options: {
      disableEmptyPlaceholder: true,
      uploadConfig: {
        image: { maxFileCount: 1, maxFileSize: "8MB", mediaType: KEYS.img },
      },
    },
  }),

  IndentPlugin.configure({ inject: { targetPlugins: LIST_TARGETS } }),
  ListPlugin.configure({
    inputRules: [
      BulletedListRules.markdown({ variant: "-" }),
      BulletedListRules.markdown({ variant: "*" }),
      OrderedListRules.markdown({ variant: "." }),
      OrderedListRules.markdown({ variant: ")" }),
      TaskListRules.markdown({ checked: false }),
      TaskListRules.markdown({ checked: true }),
    ],
    inject: { targetPlugins: LIST_TARGETS },
    render: { belowNodes: ListWrapper },
  }),

  TablePlugin.withComponent(Table),
  TableRowPlugin.withComponent(TableRow),
  TableCellPlugin.withComponent(TableCell),
  TableCellHeaderPlugin.withComponent(TableCellHeader),

  // `/` in an empty block, and nowhere else: inside a fenced block a slash is a
  // path, and a menu opening over a shell command would be the editor guessing.
  SlashPlugin.configure({
    options: {
      trigger: "/",
      triggerPreviousCharPattern: /^\s?$/,
      triggerQuery: (editor) =>
        !editor.api.some({
          match: { type: editor.getType(KEYS.codeBlock) },
        }),
    },
  }),
  SlashInputPlugin.withComponent(SlashMenu),

  // The last block of a record is a paragraph, so there is always somewhere to
  // put the caret under the text a person just finished writing.
  TrailingBlockPlugin,

  MarkdownPlugin.configure({ options: MARKDOWN_OPTIONS }),
];
