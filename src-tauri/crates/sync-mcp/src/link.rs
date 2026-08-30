//! A record named rather than addressed, and a name missing where one belongs.
//!
//! A record has two names and only one of them is a name. The key is an
//! address — permanent, what every link resolves to, and unreadable: it says
//! nothing about what it points at, and somebody reading a sentence cannot open
//! it. The title is the name. Every answer that hands out a key hands out the
//! title and the kind beside it, so writing the address instead of the name
//! throws away what the reader needed and keeps what they cannot use.
//!
//! Both directions live here because they are one fact about the same shape:
//!
//! - [`markdown`] writes an address as a name, which is what belongs in prose.
//! - [`candidates`] and [`bare`] find an address written where a name belonged,
//!   which is what the write door reports back.
//! - [`wikilinks`] finds the one spelling the write door refuses outright.
//!
//! # Why one is refused and the other only reported
//!
//! A key in a code span is ambiguous: `d-one` is a key in one body and the name
//! of a command in the next, and the store is what tells them apart. Refusing on
//! that would throw away a transaction over a guess, so it is reported and the
//! writer closes the loop on its next turn.
//!
//! Double brackets are not ambiguous. They mean nothing in Markdown, nothing in
//! this corpus and nothing to the window, which draws them as dead text — so
//! there is no reading of one that was worth writing. That is decided from the
//! text alone, without asking the store anything, which is what makes refusing
//! cheap enough to be worth it: the writer is told what to write instead and
//! writes it, rather than leaving a dead end behind for a reader to find.
//!
//! Nothing here reads a corpus. What a key *is* is a question for the store,
//! and [`bare`] takes the answer as a function rather than a connection — which
//! is what lets the whole rule be tested on strings, and what keeps the cost
//! honest: the text is read first, and the store is asked only about what the
//! text actually named.
//!
//! # Why this is written twice
//!
//! `src/lib/record-link.ts` holds the reader — it parses the url, resolves it
//! and opens the record — and it is TypeScript because it runs in the window.
//! This is the writer, and the two have to spell one thing identically or every
//! link written here is a link the window declines to follow. That is what
//! `the_spelling_is_the_one_the_window_reads` below is for: it states the
//! spelling in full rather than deriving it, so a change on either side has to
//! be made deliberately on both.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The scheme that names a record with no file of its own.
///
/// `RECORD_SCHEME` in `src/lib/record-link.ts` is the same constant, read.
pub const SCHEME: &str = "sync";

/// How many names one body may have looked up.
///
/// Every candidate past the text is a question for the store, so this is what
/// keeps one write from becoming a walk through a body written as a hundred
/// code spans. A body naming more records than this has them checked as far as
/// the cap and no further — the check reports less, never wrongly.
const MOST_CANDIDATES: usize = 40;

/// The two things a link needs beyond the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: String,
    pub title: String,
}

/// Everything `encodeURIComponent` escapes, and nothing it does not.
///
/// The window decodes with `decodeURIComponent`, so anything escaped further
/// than this still reads back — but anything escaped *less* does not, and a
/// kind spelled with a space would end the url early and name another record.
///
/// The parentheses are the addition, and they are not tidiness: a Markdown
/// destination ends at the first unbalanced `)`, so a key holding one would cut
/// the link in half and leave the rest as visible text. `encodeSegment` in
/// `src/lib/record-link.ts` escapes them for the same reason, on the same side
/// of the same round trip.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'');

/// How a record is addressed from inside a body or a message.
#[must_use]
pub fn href(kind: &str, key: &str) -> String {
    let kind = utf8_percent_encode(kind, COMPONENT);
    let key = utf8_percent_encode(key, COMPONENT);
    format!("{SCHEME}://{kind}/{key}")
}

/// How a record is named in prose: the title, carrying the address.
#[must_use]
pub fn markdown(kind: &str, key: &str, title: &str) -> String {
    format!("[{}]({})", label(title), href(kind, key))
}

/// A title, safe as the text of a link.
///
/// A bracket in a title would close the label early and leave the rest of the
/// title outside the link, which is how a record called `The window [and the
/// engine]` becomes half a link and a stray bracket. Titles are one line by
/// construction, and a newline in one is folded rather than escaped: there is
/// no spelling of a line break inside a link label.
fn label(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for character in title.chars() {
        match character {
            '\\' | '[' | ']' => {
                out.push('\\');
                out.push(character);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(character),
        }
    }
    out
}

/// One record named by its address where its name belonged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bare {
    /// The key that was written.
    pub key: String,
    /// The record it was written in.
    pub written_in: String,
    /// The link to write instead.
    pub instead: String,
}

/// The keys `content` writes in a code span where a name belonged, resolved.
///
/// `resolve` answers what a key names, or `None` for a string that is not a
/// record. Only what it answers for is reported: a code span holding a command
/// is not a reference, and neither is a key of a record that does not exist.
/// That filter is what makes reporting safe here and is exactly what
/// [`wikilinks`] does not need — see this module's opening.
pub fn bare(
    content: &str,
    written_in: &str,
    mut resolve: impl FnMut(&str) -> Option<Record>,
) -> Vec<Bare> {
    candidates(content)
        .into_iter()
        .filter_map(|key| {
            let record = resolve(key)?;
            Some(Bare {
                key: key.to_owned(),
                written_in: written_in.to_owned(),
                instead: markdown(&record.kind, key, &record.title),
            })
        })
        .collect()
}

/// What `content` might be naming a record by, in first-written order.
///
/// # What this catches, and what it deliberately does not
///
/// A bare string in a code span, which is unmistakably somebody writing *I am
/// naming a record here* while leaving out the name. The other spelling that
/// means the same thing — double brackets — is [`wikilinks`], and it is
/// separate because it is answered differently: refused rather than reported.
///
/// A key in running text is **not** caught, and that is a decision rather than
/// an omission. A key is a string the project chose, and plenty of any
/// project's are ordinary words. A check that read every sentence containing
/// the word *architecture* as a reference would be switched off in its first
/// week, and it would take the real case with it. A code span is what the
/// corpus and the prompts actually write, which is the whole reason it is the
/// one.
///
/// A fence is skipped whole: it holds a command, an example, or a record
/// somebody is quoting, and none of those is prose. So is anything holding a
/// character no key of a corpus carries — a space, a slash, a dot, a bracket —
/// because every one of those is a path, a call or a sentence, and asking the
/// store about each would turn a body of code spans into a walk through it.
#[must_use]
pub fn candidates(content: &str) -> Vec<&str> {
    let mut found: Vec<&str> = Vec::new();
    for (spelling, name) in spellings(content) {
        if spelling != Spelling::Span || found.contains(&name) {
            continue;
        }
        found.push(name);
        if found.len() == MOST_CANDIDATES {
            break;
        }
    }
    found
}

/// The names `content` writes in double brackets, in first-written order.
///
/// Uncapped, unlike [`candidates`], because nothing here is asked of the store:
/// the whole answer is read off the text, so a body writing forty of them is
/// told about forty rather than about the first few.
///
/// What is inside the brackets still has to look like a key — no space, no
/// slash, no comma — for the same reason a code span does. `[[a, b], [c, d]]`
/// written into a sentence is somebody's array, and refusing a write over it
/// would be this door deciding it knows what a table means.
#[must_use]
pub fn wikilinks(content: &str) -> Vec<&str> {
    let mut found: Vec<&str> = Vec::new();
    for (spelling, name) in spellings(content) {
        if spelling == Spelling::Brackets && !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

/// Which of the two spellings a name was written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spelling {
    /// `` `d-one` ``
    Span,
    /// `[[d-one]]`
    Brackets,
}

/// Every name `content` writes in either spelling, in the prose of it.
///
/// Neither deduplicated nor capped — both of those are what the caller wants
/// them to be, and they differ between the two callers.
fn spellings(content: &str) -> Vec<(Spelling, &str)> {
    let mut found = Vec::new();
    let mut fence: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(open) = fence_at(trimmed) {
            match &fence {
                // A fence closes on its own character, at its own length or
                // longer. Anything else is a fence inside a fence.
                Some(current) if open.starts_with(&current[..1]) && open.len() >= current.len() => {
                    fence = None;
                }
                Some(_) => {}
                None => fence = Some(open),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        found.extend(
            referenced(line)
                .into_iter()
                .filter(|(_, name)| plausible(name)),
        );
    }

    found
}

/// Whether a string is shaped like a key at all.
///
/// Cheap and deliberately blunt: this decides only whether the store is worth
/// asking, and the store is what actually answers. What it rejects is what no
/// key is — a path, a call, a sentence, a snippet of code.
fn plausible(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.chars().any(|character| {
            character.is_whitespace()
                || matches!(
                    character,
                    '/' | '\\'
                        | '.'
                        | ','
                        | ';'
                        | ':'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '"'
                        | '\''
                        | '='
                        | '`'
                        | '|'
                        | '&'
                        | '$'
                        | '#'
                        | '@'
                        | '?'
                        | '!'
                )
        })
}

/// The fence a line opens or closes, if it is one.
fn fence_at(trimmed: &str) -> Option<String> {
    let character = trimmed.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let run: String = trimmed.chars().take_while(|c| *c == character).collect();
    (run.len() >= 3).then_some(run)
}

/// What one line names, as the two spellings that mean a record.
///
/// Both are read in one pass over the line rather than by two searches, because
/// they nest: a code span may hold double brackets and a link label may hold a
/// code span, and two independent passes would read the inside of one as if it
/// stood alone.
fn referenced(line: &str) -> Vec<(Spelling, &str)> {
    let bytes = line.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'`' {
            let ticks = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
            let opened = at + ticks;
            let Some(closed) = closing(line, opened, &"`".repeat(ticks)) else {
                // An unmatched backtick opens no code span, so the rest of the
                // line is ordinary text and is read as such.
                at += ticks;
                continue;
            };
            names.push((Spelling::Span, line[opened..closed].trim()));
            at = closed + ticks;
            continue;
        }
        if bytes[at] == b'['
            && bytes.get(at + 1) == Some(&b'[')
            && let Some(closed) = closing(line, at + 2, "]]")
        {
            names.push((Spelling::Brackets, line[at + 2..closed].trim()));
            at = closed + 2;
            continue;
        }
        at += 1;
    }

    names
}

/// Where `close` next appears at or after `from`.
fn closing(line: &str, from: usize, close: &str) -> Option<usize> {
    line.get(from..)
        .and_then(|rest| rest.find(close))
        .map(|offset| from + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus these tests are written against: one record keyed as a
    /// digest, and one keyed as an ordinary English word, because both exist in
    /// every corpus and the second is the one that makes this hard.
    fn resolve(key: &str) -> Option<Record> {
        match key {
            "d-one" => Some(Record {
                kind: "decision".to_owned(),
                title: "The one that was taken".to_owned(),
            }),
            "architecture" => Some(Record {
                kind: "docs".to_owned(),
                title: "Architecture".to_owned(),
            }),
            _ => None,
        }
    }

    /// The window's half of this is `recordHref` in `src/lib/record-link.ts`,
    /// and a link it cannot parse is a link drawn as text. Stated in full
    /// rather than derived, so moving either side has to be deliberate.
    #[test]
    fn the_spelling_is_the_one_the_window_reads() {
        assert_eq!(href("decision", "d-one"), "sync://decision/d-one");
        assert_eq!(
            markdown("decision", "d-one", "The one that was taken"),
            "[The one that was taken](sync://decision/d-one)"
        );
    }

    /// A kind is the host of the url and a space in one would end it early,
    /// which is the difference between a link to a record and a link to nothing.
    #[test]
    fn what_would_break_the_url_is_escaped_and_what_would_not_is_left() {
        assert_eq!(href("a kind", "a key"), "sync://a%20kind/a%20key");
        assert_eq!(href("kind", "d(1)"), "sync://kind/d%281%29");
        // Left alone, because `decodeURIComponent` gives them back unchanged
        // and escaping them would make an ordinary key unreadable in a url.
        assert_eq!(href("kind", "a-b_c.d"), "sync://kind/a-b_c.d");
    }

    /// A bracket in a title closes the label, and the rest of the title falls
    /// out of the link.
    #[test]
    fn a_bracket_in_a_title_does_not_cut_the_link_in_half() {
        assert_eq!(
            markdown("kind", "k", "The window [and the engine]"),
            "[The window \\[and the engine\\]](sync://kind/k)"
        );
    }

    /// A key written in a code span, and the link to put in its place.
    #[test]
    fn a_key_written_in_a_code_span_is_reported_with_its_link() {
        let found = bare("Recorded `d-one`, which supersedes it.", "d-two", resolve);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "d-one");
        assert_eq!(found[0].written_in, "d-two");
        assert_eq!(
            found[0].instead,
            "[The one that was taken](sync://decision/d-one)"
        );
    }

    /// The two spellings are answered differently, so they are read apart: what
    /// is reported is the code span, and the brackets are refused elsewhere.
    #[test]
    fn double_brackets_are_not_reported_as_a_bare_key() {
        let content = "Recorded `d-one`, which supersedes [[architecture]].";
        assert_eq!(candidates(content), vec!["d-one"]);
        assert_eq!(wikilinks(content), vec!["architecture"]);
    }

    /// The whole reason brackets can be refused: nothing is asked of the store,
    /// so a name nothing answers to is found exactly as readily as one that
    /// resolves. It is the spelling that is wrong, not the destination.
    #[test]
    fn a_wikilink_is_found_whether_or_not_anything_answers_to_it() {
        assert_eq!(
            wikilinks("See [[d-one]] and [[d-missing]]."),
            vec!["d-one", "d-missing"]
        );
    }

    /// A document about this syntax has to remain writable, and the way to
    /// write about a spelling is the way to write about any other code.
    #[test]
    fn a_wikilink_quoted_as_an_example_is_not_one() {
        assert!(wikilinks("Do not write `[[d-one]]`.").is_empty());
        assert!(wikilinks("Before:\n\n```\n[[d-one]]\n```\n\nAfter.").is_empty());
    }

    /// Somebody's array is not somebody's link, and refusing a write over one
    /// would be this door deciding what a pair of brackets means.
    #[test]
    fn brackets_around_something_no_key_looks_like_are_left_alone() {
        assert!(wikilinks("The shape is [[1, 2], [3, 4]] rows.").is_empty());
    }

    /// A key is a word somebody chose, and plenty of them are words. Reading
    /// one in running text would report every sentence about architecture.
    #[test]
    fn an_ordinary_word_that_happens_to_be_a_key_is_left_alone() {
        assert!(bare("The architecture is settled.", "d-two", resolve).is_empty());
    }

    /// A fence holds a command or a quoted record, and neither is prose.
    #[test]
    fn a_key_inside_a_fence_is_an_example_rather_than_a_reference() {
        let content = "Before:\n\n```json\n{\"key\": \"d-one\"}\n```\n\nAfter.";
        assert!(bare(content, "d-two", resolve).is_empty());
    }

    /// A key already written as a link is what this asks for, and reporting it
    /// would be reporting somebody for doing as they were told.
    #[test]
    fn a_key_already_written_as_a_link_is_not_reported() {
        let content = "Superseded by [The one that was taken](sync://decision/d-one).";
        assert!(bare(content, "d-two", resolve).is_empty());
    }

    /// A key nothing answers to cannot be named, so there is nothing to suggest
    /// and nothing to report.
    #[test]
    fn a_key_no_record_answers_to_is_not_reported() {
        assert!(bare("See `d-gone`.", "d-two", resolve).is_empty());
    }

    /// Said twice, reported once: the report is a list of records to name, not
    /// a list of places.
    #[test]
    fn the_same_key_twice_is_one_report() {
        let found = bare("`d-one` and again `d-one`.", "d-two", resolve);
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// The store is asked about what could be a key, and a body of ordinary
    /// technical prose is full of code spans that could not be.
    #[test]
    fn a_path_a_call_and_a_sentence_are_never_asked_about() {
        let content = "Read `src/lib/record-link.ts`, call `href(kind, key)`, and see \
                       `the note above`.";
        assert!(candidates(content).is_empty(), "{:?}", candidates(content));
    }

    /// One body may not turn one write into a walk through the store.
    #[test]
    fn the_number_of_names_one_body_can_ask_about_is_capped() {
        let content = (0..MOST_CANDIDATES * 2)
            .map(|n| format!("`k-{n}`"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(candidates(&content).len(), MOST_CANDIDATES);
    }
}
