//! Turning a summary into blocks the Meetings view can lay out.
//!
//! The summariser returns markdown -- `**Decisions**`,
//! `**Planned actions**`, `-` bullets -- and the view printed it as one
//! flat string, so the asterisks showed literally and every bullet
//! collapsed into a paragraph. A meeting note that reads like a wall of
//! text is most of the difference between this and a notes app people
//! like.
//!
//! **Why this is in Rust rather than the view.** The frontend has four
//! runtime dependencies and no test runner; parsing is the part most
//! likely to be subtly wrong, and it is the part worth pinning with the
//! real summaries that have come out of real meetings. So the view
//! receives blocks and only has to lay them out.
//!
//! Deliberately a small subset: headings, bullets with one level of
//! nesting, bold runs, paragraphs. Not a markdown implementation -- the
//! input is one prompt's output, not arbitrary documents, and anything
//! unrecognised survives as plain text rather than disappearing.

/// A run of text, bold or not.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Span {
    Text { text: String },
    Strong { text: String },
}

/// One bullet. Nesting is a depth rather than a tree: the view indents by
/// it, and a flat list is far easier to render correctly than a recursive
/// one for a maximum of two levels.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Bullet {
    pub spans: Vec<Span>,
    pub depth: u8,
    /// One of the reader's own typed lines, kept word for word. The view
    /// shows these in full ink and Jelly's additions around them in grey.
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Heading { text: String },
    Paragraph { spans: Vec<Span> },
    Bullets { items: Vec<Bullet> },
}

/// Splits a line into bold and plain runs.
///
/// Unbalanced markers are left alone: `**Planned actions` with no closing
/// pair should read as written rather than swallow the rest of the note.
fn spans_of(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find("**") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("**") else { break };
        if open > 0 {
            spans.push(Span::Text { text: rest[..open].to_string() });
        }
        let bold = &after[..close];
        if !bold.is_empty() {
            spans.push(Span::Strong { text: bold.to_string() });
        }
        rest = &after[close + 2..];
    }
    if !rest.is_empty() {
        spans.push(Span::Text { text: rest.to_string() });
    }
    if spans.is_empty() {
        spans.push(Span::Text { text: String::new() });
    }
    spans
}

/// Whether a line is a heading, and its text.
///
/// Two forms, because the model uses both: a markdown `## Heading`, and a
/// short line that is entirely bold. The second is the one it actually
/// reaches for -- `**Planned actions**` on its own line is a heading in
/// everything but syntax, and treating it as a paragraph is what made
/// summaries read as undifferentiated prose.
fn heading_of(line: &str) -> Option<String> {
    let t = line.trim();
    if let Some(rest) = t.trim_start_matches('#').strip_prefix(' ') {
        if t.starts_with('#') {
            return Some(rest.trim().to_string());
        }
    }
    let inner = t.strip_prefix("**")?.strip_suffix("**")?;
    // A whole paragraph wrapped in bold is emphasis, not a heading.
    if inner.contains("**") || inner.len() > 60 || inner.ends_with('.') {
        return None;
    }
    Some(inner.trim_end_matches(':').trim().to_string())
}

/// Whether a line is a bullet, and how deep.
fn bullet_of(line: &str) -> Option<(u8, &str)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    let body = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("• "))?;
    // Two spaces or a tab is one level in; anything deeper still counts as
    // one, since the view only indents twice.
    Some((if indent >= 2 { 1 } else { 0 }, body))
}

/// The words of a line with case, punctuation and spacing ignored, so a
/// bullet the model kept verbatim matches the line the reader typed.
fn plain_words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Marks the bullets that are the reader's own typed lines. The summary
/// is told to keep those lines word for word, so this is a match on the
/// words, not a guess at meaning; a bullet the model reworded stays grey.
pub fn mark_mine(blocks: &mut [Block], my_notes: &str) {
    let mine: Vec<String> = my_notes
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim())
        .map(plain_words)
        .filter(|l| !l.is_empty())
        .collect();
    if mine.is_empty() {
        return;
    }
    for block in blocks.iter_mut() {
        if let Block::Bullets { items } = block {
            for item in items.iter_mut() {
                let text: String = item
                    .spans
                    .iter()
                    .map(|s| match s {
                        Span::Text { text } | Span::Strong { text } => text.as_str(),
                    })
                    .collect();
                item.mine = mine.contains(&plain_words(&text));
            }
        }
    }
}

/// Parses a summary into blocks.
pub fn parse(summary: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();

    let flush = |paragraph: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if paragraph.is_empty() {
            return;
        }
        let joined = paragraph.join(" ");
        paragraph.clear();
        if !joined.trim().is_empty() {
            blocks.push(Block::Paragraph { spans: spans_of(joined.trim()) });
        }
    };

    for raw in summary.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        if let Some((depth, body)) = bullet_of(line) {
            flush(&mut paragraph, &mut blocks);
            let item = Bullet { spans: spans_of(body.trim()), depth, mine: false };
            match blocks.last_mut() {
                Some(Block::Bullets { items }) => items.push(item),
                _ => blocks.push(Block::Bullets { items: vec![item] }),
            }
            continue;
        }
        if let Some(text) = heading_of(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading { text });
            continue;
        }
        paragraph.push(line.trim().to_string());
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Span {
        Span::Text { text: s.to_string() }
    }
    fn strong(s: &str) -> Span {
        Span::Strong { text: s.to_string() }
    }

    /// A real-shaped summary: the kind that rendered as a wall of text with
    /// literal asterisks in it.
    const REAL: &str = "The meeting was about **Atlas**, a privacy-focused \"second brain\" intended to combine note-taking, memory, reminders, and task management in one product rather than requiring multiple subscriptions. The first version is planned to identify open loops, schedule reminders, summarise meetings, extract follow-up items into a to-do list, and let users search or converse with their locally stored memories.

**Planned actions**
- One participant will continue improving note-taking, multilingual support, and the user-memory component.
- A teammate will work on UI changes and improvements.
- The team plans to launch the initial version with the features discussed.";

    #[test]
    fn the_real_summary_becomes_structured_blocks() {
        let blocks = parse(REAL);
        // Paragraph, heading, bullets -- not one undifferentiated run.
        assert!(matches!(blocks[0], Block::Paragraph { .. }), "{blocks:#?}");
        assert_eq!(blocks[1], Block::Heading { text: "Planned actions".into() });
        let Block::Bullets { items } = &blocks[2] else {
            panic!("expected bullets, got {:#?}", blocks[2])
        };
        assert_eq!(items.len(), 3, "each action is its own bullet");
    }

    /// The asterisks must become emphasis, never survive as characters.
    #[test]
    fn bold_markers_never_reach_the_reader() {
        let blocks = parse(REAL);
        let Block::Paragraph { spans } = &blocks[0] else { panic!() };
        assert!(spans.contains(&strong("Atlas")), "{spans:#?}");
        for block in &blocks {
            let rendered = format!("{block:?}");
            assert!(!rendered.contains("**"), "an asterisk pair survived: {rendered}");
        }
    }

    /// A bold line on its own is how the model writes a heading. Treating
    /// it as a paragraph is what made summaries read as undifferentiated
    /// prose.
    #[test]
    fn a_bold_line_alone_is_a_heading() {
        assert_eq!(heading_of("**Planned actions**"), Some("Planned actions".into()));
        assert_eq!(heading_of("**Decisions:**"), Some("Decisions".into()));
        assert_eq!(heading_of("## Privacy"), Some("Privacy".into()));
    }

    /// But a whole bold sentence is emphasis, not a heading -- promoting
    /// it would lose the sentence from the body.
    #[test]
    fn a_bold_sentence_is_not_a_heading() {
        assert_eq!(
            heading_of("**The team agreed to ship the first version in October.**"),
            None
        );
        assert_eq!(heading_of("**Atlas** is a second brain"), None);
    }

    /// Unbalanced markers are left alone rather than swallowing the rest
    /// of the note.
    #[test]
    fn an_unclosed_bold_marker_is_harmless() {
        let blocks = parse("Something **unfinished here");
        let Block::Paragraph { spans } = &blocks[0] else { panic!() };
        assert_eq!(spans, &vec![text("Something **unfinished here")]);
    }

    #[test]
    fn nested_bullets_keep_their_depth() {
        let blocks = parse("- Memory stays local\n  - Redaction runs first\n- Zero retention");
        let Block::Bullets { items } = &blocks[0] else { panic!("{blocks:#?}") };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].depth, 0);
        assert_eq!(items[1].depth, 1, "an indented bullet is a child");
        assert_eq!(items[2].depth, 0);
    }

    /// Consecutive bullets are one list, not one list each -- otherwise
    /// the spacing between them is wrong everywhere.
    #[test]
    fn consecutive_bullets_form_one_list() {
        let blocks = parse("- one\n- two\n- three");
        assert_eq!(blocks.len(), 1);
    }

    /// Plain prose with no markup at all still renders.
    #[test]
    fn an_unformatted_summary_still_produces_a_paragraph() {
        let blocks = parse("We talked about the release and agreed to ship on Friday.");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
    }

    /// Confirms the premise the 2026-09-19 prompt rewrite rests on: this
    /// parser already renders topic-grouped `##` sections with nested
    /// bullets correctly, and always has. Compared against Granola on a
    /// real meeting, the gap was the prompt asking for three fixed slots
    /// (overview, Decisions, Follow-ups) with nowhere to put anything
    /// else -- not a renderer limitation. Shaped after that real
    /// comparison, trimmed down.
    #[test]
    fn adaptive_topic_headings_with_nested_bullets_render_correctly() {
        // Built by explicit `\n` joins rather than backslash-continued
        // lines: a `\` at end-of-line in a Rust string literal also eats
        // the NEXT line's leading whitespace, which silently flattened
        // the nested bullet's intended two-space indent the first time
        // this was written -- `depth: 0` where the test expected `1`,
        // and the fix was the test, not `bullet_of`.
        let summary = [
            "The meeting covered the payout fix and a product review.",
            "",
            "## Payout System",
            "- Instant payout lands about one second after purchase",
            "  - Full test suite passing",
            "- Delay traced to accrual running every 30 seconds",
            "",
            "## Product Notes",
            "- Score mentioned: 294 or 295, unclear which is final",
            "- Feature idea: drafting on behalf of the user",
            "",
            "**Decisions**",
            "- You agreed the project should be done by tomorrow afternoon",
            "",
            "**Follow-ups**",
            "- Someone will tighten the fallback timers",
        ]
        .join("\n");
        let summary = summary.as_str();
        let blocks = parse(summary);

        let headings: Vec<&str> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            headings,
            vec!["Payout System", "Product Notes", "Decisions", "Follow-ups"],
            "a topic heading and a bold Decisions/Follow-ups heading must both parse, in order: {blocks:#?}"
        );

        // The nested bullet under Payout System survives with its depth,
        // between the two top-level bullets either side of it.
        let Block::Bullets { items } = &blocks[2] else { panic!("expected bullets: {:#?}", blocks[2]) };
        assert_eq!(items.len(), 3, "{items:#?}");
        assert_eq!(items[0].depth, 0);
        assert_eq!(items[1].depth, 1, "the sub-bullet must be nested, not flattened: {items:#?}");
        assert_eq!(items[2].depth, 0);

        // The unresolved figure survives verbatim -- not silently resolved
        // to one number.
        let rendered = format!("{blocks:#?}");
        assert!(rendered.contains("294 or 295"), "{rendered}");
    }
}

#[cfg(test)]
mod mine_tests {
    use super::*;

    /// Your typed lines are found again in the written notes; Jelly's
    /// additions and a line it reworded are not.
    #[test]
    fn mark_mine_matches_the_readers_own_lines_only() {
        let mut blocks = parse("## Android\n- Need the team estimate first.\n  - Priya wants a rough timeline.\n- Card shows too late\n- card shows a little late");
        mark_mine(&mut blocks, "need the team estimate first\n- card shows too late!\n\n");
        let Block::Bullets { items } = &blocks[1] else { panic!("bullets expected: {blocks:?}") };
        let mine: Vec<bool> = items.iter().map(|b| b.mine).collect();
        assert_eq!(mine, vec![true, false, true, false]);
        let mut untouched = parse("- anything");
        mark_mine(&mut untouched, "   ");
        let Block::Bullets { items } = &untouched[0] else { panic!() };
        assert!(!items[0].mine, "no notes typed, nothing is yours");
    }
}
