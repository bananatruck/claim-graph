//! Deterministic parser for a research draft written in Markdown.
//!
//! See SPEC.md, section "Draft (Markdown)", for the input contract.

use std::fmt;

use crate::classify::{Reason, classify};
use crate::figures::{Figure, extract_figures};

/// A parsed draft: its title, its sentences in reading order, and its bibliography.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub title: String,
    pub sentences: Vec<Sentence>,
    pub sources: Vec<Source>,
}

impl Draft {
    /// The sentences that are checkable claims, in reading order.
    pub fn claims(&self) -> impl Iterator<Item = &Sentence> {
        self.sentences.iter().filter(|s| s.is_claim())
    }
}

/// One sentence of the draft, with the references and figures it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct Sentence {
    /// `C1`, `C2`, ... for checkable claims and `P1`, `P2`, ... for prose, each in reading order.
    pub id: String,
    /// 1-based position among all sentences, in reading order.
    pub ordinal: usize,
    /// Name of the enclosing `## ` section, or empty before the first section.
    pub section: String,
    /// The sentence as written, references included.
    pub text: String,
    /// Source keys cited with `[@key]`.
    pub citations: Vec<String>,
    /// Run IDs referenced with `[run:ID]`.
    pub runs: Vec<String>,
    /// Experiment IDs referenced with `[exp:ID]`.
    pub experiments: Vec<String>,
    /// Why the sentence is a checkable claim; empty for prose (see `classify`).
    pub reasons: Vec<Reason>,
    /// Numbers stated in the sentence, outside reference markers.
    pub figures: Vec<Figure>,
}

impl Sentence {
    pub fn is_claim(&self) -> bool {
        !self.reasons.is_empty()
    }
}

/// One entry of the References section.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub key: String,
    pub entry: String,
}

/// A parse failure, with the 1-based line it was found on.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Words that end with a period without ending a sentence, compared lowercase.
/// `al.` counts only after `et`, and `No.` only before a number (see `is_abbreviation`).
const ABBREVIATIONS: &[&str] = &[
    "e.g", "i.e", "al", "fig", "figs", "sec", "eq", "eqs", "tab", "vs", "cf", "approx", "no",
];

/// Characters that may close a sentence after its final punctuation: `.)`, `."`.
const CLOSERS: &[char] = &[')', ']', '"', '\'', '\u{201D}', '\u{2019}'];

/// Parse a draft. Every sentence outside the References section is kept;
/// `classify` decides which ones are checkable claims.
pub fn parse_draft(input: &str) -> Result<Draft, ParseError> {
    let mut parser = Parser::default();
    for (index, raw) in input.lines().enumerate() {
        parser.line(index + 1, raw)?;
    }
    if let Some(block) = parser.block {
        let (line, what) = match block {
            Block::Fence { line, .. } => (line, "code fence"),
            Block::Math { line } => (line, "`$$` math block"),
        };
        return Err(ParseError {
            line,
            message: format!("{what} opened here is never closed"),
        });
    }
    parser.flush()?;
    Ok(Draft {
        title: parser.title.unwrap_or_default(),
        sentences: parser.sentences,
        sources: parser.sources,
    })
}

#[derive(Clone, Copy)]
enum Block {
    /// A code fence of `len` backticks, opened on `line`.
    Fence { len: usize, line: usize },
    /// A `$$` display-math block opened on `line`.
    Math { line: usize },
}

#[derive(Default)]
struct Parser {
    title: Option<String>,
    section: String,
    in_references: bool,
    /// Inside a code fence or a `$$` display-math block.
    block: Option<Block>,
    paragraph: Paragraph,
    sentences: Vec<Sentence>,
    sources: Vec<Source>,
    claims: usize,
    prose: usize,
}

/// Lines of one paragraph joined with single spaces, remembering where each line starts.
#[derive(Default)]
struct Paragraph {
    text: String,
    /// `(byte offset in text, 1-based line number)`, in increasing offset order.
    line_starts: Vec<(usize, usize)>,
}

impl Paragraph {
    fn push(&mut self, line: usize, content: &str) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.line_starts.push((self.text.len(), line));
        self.text.push_str(content);
    }

    fn line_at(&self, offset: usize) -> usize {
        self.line_starts
            .iter()
            .rev()
            .find(|(start, _)| *start <= offset)
            .map_or(0, |(_, line)| *line)
    }
}

impl Parser {
    fn line(&mut self, line: usize, raw: &str) -> Result<(), ParseError> {
        let mut trimmed = raw.trim();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match self.block {
            // A fence closes on a line of at least as many backticks and nothing else.
            Some(Block::Fence { len, .. }) => {
                if ticks >= len && trimmed[ticks..].trim().is_empty() {
                    self.block = None;
                }
                return Ok(());
            }
            Some(Block::Math { .. }) => {
                if trimmed.starts_with("$$") || trimmed.ends_with("$$") {
                    self.block = None;
                }
                return Ok(());
            }
            None => {}
        }
        // Three or more backticks open a fence, unless the line closes them again,
        // which makes it inline code.
        if ticks >= 3 && !trimmed[ticks..].contains('`') {
            self.flush()?;
            self.block = Some(Block::Fence { len: ticks, line });
            return Ok(());
        }
        if let Some(rest) = trimmed.strip_prefix("$$") {
            self.flush()?;
            match rest.find("$$") {
                // `$$ ... $$` on one line: drop the math, keep any text after it.
                Some(close) => trimmed = rest[close + 2..].trim(),
                None => {
                    self.block = Some(Block::Math { line });
                    return Ok(());
                }
            }
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            // A top-level heading also ends the References section.
            self.flush()?;
            let heading = heading.trim().to_string();
            if self.title.is_none() {
                self.title = Some(heading.clone());
            }
            self.section = heading;
            self.in_references = false;
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            self.flush()?;
            self.section = heading.trim().to_string();
            self.in_references = self.section.eq_ignore_ascii_case("references");
        } else if trimmed.is_empty()
            || is_atx_heading(trimmed)
            || trimmed.starts_with('>')
            || trimmed.starts_with('|')
            || trimmed.starts_with("![")
        {
            // Blank lines, deeper headings, notes, table rows and images end a paragraph
            // and are not sentences.
            self.flush()?;
        } else if self.in_references {
            self.source(line, trimmed)?;
        } else if let Some(item) = list_item(trimmed) {
            self.flush()?;
            self.paragraph.push(line, item);
        } else {
            self.paragraph.push(line, trimmed);
        }
        Ok(())
    }

    fn source(&mut self, line: usize, trimmed: &str) -> Result<(), ParseError> {
        let malformed = || ParseError {
            line,
            message: format!("expected `- [@key] entry` in References, found `{trimmed}`"),
        };
        let rest = trimmed.strip_prefix("- [@").ok_or_else(malformed)?;
        let (key, entry) = rest.split_once(']').ok_or_else(malformed)?;
        let entry = entry.trim();
        if !is_identifier(key) || entry.is_empty() {
            return Err(malformed());
        }
        if self.sources.iter().any(|s| s.key == key) {
            return Err(ParseError {
                line,
                message: format!("source key `{key}` is listed twice"),
            });
        }
        self.sources.push(Source {
            key: key.to_string(),
            entry: entry.to_string(),
        });
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ParseError> {
        let paragraph = std::mem::take(&mut self.paragraph);
        for (start, sentence) in split_sentences(&paragraph.text) {
            let refs = extract_references(sentence).map_err(|(offset, message)| ParseError {
                line: paragraph.line_at(start + offset),
                message,
            })?;
            let reasons = classify(
                &refs.plain,
                !refs.citations.is_empty(),
                !(refs.runs.is_empty() && refs.experiments.is_empty()),
            );
            let id = if reasons.is_empty() {
                self.prose += 1;
                format!("P{}", self.prose)
            } else {
                self.claims += 1;
                format!("C{}", self.claims)
            };
            self.sentences.push(Sentence {
                id,
                ordinal: self.sentences.len() + 1,
                section: self.section.clone(),
                text: sentence.to_string(),
                citations: refs.citations,
                runs: refs.runs,
                experiments: refs.experiments,
                figures: extract_figures(&refs.plain),
                reasons,
            });
        }
        Ok(())
    }
}

/// The text of a `- `, `* `, `1. ` or `1) ` list item, or `None`.
fn list_item(trimmed: &str) -> Option<&str> {
    if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return Some(item.trim());
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &trimmed[digits..];
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .map(str::trim)
}

/// Split text into trimmed sentences with their byte offsets.
///
/// A sentence ends at `.`, `!` or `?` followed by whitespace or the end of the text,
/// except after one of the `ABBREVIATIONS`.
fn split_sentences(text: &str) -> Vec<(usize, &str)> {
    let markers = marker_spans(text);
    let inside_marker = |index: usize| markers.iter().any(|&(a, b)| a < index && index < b);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut sentences = Vec::new();
    let mut start = 0;
    for (position, &(index, ch)) in chars.iter().enumerate() {
        if index < start || !matches!(ch, '.' | '!' | '?') || inside_marker(index) {
            continue;
        }
        // Closing brackets and quotes after the punctuation belong to the sentence.
        let mut next = position + 1;
        while chars.get(next).is_some_and(|&(_, c)| CLOSERS.contains(&c)) {
            next += 1;
        }
        let end = chars.get(next).map_or(text.len(), |&(b, _)| b);
        let at_boundary = chars.get(next).is_none_or(|&(_, c)| c.is_whitespace());
        if !at_boundary {
            continue;
        }
        if ch == '.' && next < chars.len() && is_abbreviation(&text[start..index], &text[end..]) {
            continue;
        }
        push_trimmed(&mut sentences, text, start, end);
        start = end;
    }
    push_trimmed(&mut sentences, text, start, text.len());
    sentences
}

/// Byte ranges of `[@...]`, `[run:...]` and `[exp:...]` markers, to the closing `]` or the
/// end of the text.
fn marker_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find('[') {
        let open = from + found;
        let after = &text[open + 1..];
        if after.starts_with('@') || after.starts_with("run:") || after.starts_with("exp:") {
            let close = after.find(']').map_or(text.len(), |c| open + 1 + c);
            spans.push((open, close));
            from = close.min(text.len());
        } else {
            from = open + 1;
        }
        if from >= text.len() {
            break;
        }
    }
    spans
}

/// Whether the word just before a period is a known abbreviation, given the text before
/// the period and the text after it.
fn is_abbreviation(before_period: &str, after_period: &str) -> bool {
    let word: String = before_period
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic() || *c == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let word = word.to_lowercase();
    if !ABBREVIATIONS.contains(&word.as_str()) {
        return false;
    }
    match word.as_str() {
        "al" => before_period[..before_period.len() - word.len()]
            .split_whitespace()
            .next_back()
            .is_some_and(|previous| previous.eq_ignore_ascii_case("et")),
        "no" => after_period
            .trim_start()
            .starts_with(|c: char| c.is_ascii_digit()),
        _ => true,
    }
}

fn push_trimmed<'a>(out: &mut Vec<(usize, &'a str)>, text: &'a str, start: usize, end: usize) {
    let slice = &text[start..end];
    let trimmed = slice.trim_start();
    let offset = start + (slice.len() - trimmed.len());
    let trimmed = trimmed.trim_end();
    if !trimmed.is_empty() {
        out.push((offset, trimmed));
    }
}

#[derive(Default)]
struct References {
    citations: Vec<String>,
    runs: Vec<String>,
    experiments: Vec<String>,
    /// The sentence with every reference marker replaced by a space.
    plain: String,
}

/// Find `[@key; @key]` (or comma-separated), `[run:ID]` and `[exp:ID]` markers.
/// Other bracketed text, such as Markdown links, is left alone.
/// Errors carry the byte offset of the offending marker.
fn extract_references(sentence: &str) -> Result<References, (usize, String)> {
    let mut refs = References::default();
    let mut search_from = 0;
    let mut copied_to = 0;
    while let Some(found) = sentence[search_from..].find('[') {
        let open = search_from + found;
        let after = &sentence[open + 1..];
        let kind = if after.starts_with('@') {
            Some(Kind::Citation)
        } else if after.starts_with("run:") {
            Some(Kind::Run)
        } else if after.starts_with("exp:") {
            Some(Kind::Experiment)
        } else {
            None
        };
        let Some(kind) = kind else {
            search_from = open + 1;
            continue;
        };
        let close = after
            .find(']')
            .ok_or_else(|| (open, "reference is missing its closing `]`".to_string()))?;
        let body = &after[..close];
        let bad = |what: &str| (open, format!("malformed {what} `[{body}]`"));
        match kind {
            Kind::Citation => {
                for part in body.split([';', ',']) {
                    let key = part
                        .trim()
                        .strip_prefix('@')
                        .ok_or_else(|| bad("citation"))?;
                    if !is_identifier(key) {
                        return Err(bad("citation"));
                    }
                    refs.citations.push(key.to_string());
                }
            }
            Kind::Run | Kind::Experiment => {
                let id = &body[4..];
                if !is_identifier(id) {
                    return Err(bad("reference"));
                }
                let list = if matches!(kind, Kind::Run) {
                    &mut refs.runs
                } else {
                    &mut refs.experiments
                };
                list.push(id.to_string());
            }
        }
        refs.plain.push_str(&sentence[copied_to..open]);
        refs.plain.push(' ');
        search_from = open + 1 + close + 1;
        copied_to = search_from;
    }
    refs.plain.push_str(&sentence[copied_to..]);
    Ok(refs)
}

enum Kind {
    Citation,
    Run,
    Experiment,
}

pub(crate) fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// A heading: one to six `#` followed by a space or the end of the line, as SPEC.md
/// defines headings. Other text that starts with `#` ("#1 reached 74.2%", "##\tResults")
/// is ordinary text, so it is checked rather than dropped.
fn is_atx_heading(line: &str) -> bool {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&level) && matches!(line.as_bytes().get(level), None | Some(b' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(draft: &Draft) -> Vec<&str> {
        draft.sentences.iter().map(|s| s.id.as_str()).collect()
    }

    fn texts(draft: &Draft) -> Vec<&str> {
        draft.sentences.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn reads_title_sections_and_numbers_sentences_in_order() {
        let draft = parse_draft(
            "# A title\n\n## Abstract\n\nWe win by 2 points. It is faster.\n\n## Results\n\nIt has 3 parts.\n",
        )
        .unwrap();
        assert_eq!(draft.title, "A title");
        assert_eq!(ids(&draft), ["C1", "C2", "C3"]);
        assert_eq!(draft.sentences[1].text, "It is faster.");
        assert_eq!(draft.sentences[1].section, "Abstract");
        assert_eq!(draft.sentences[2].section, "Results");
        assert_eq!(draft.sentences[2].ordinal, 3);
    }

    #[test]
    fn prose_and_claims_are_numbered_separately() {
        let draft =
            parse_draft("We study planners. Accuracy is 71.3% here. The setup follows.\n").unwrap();
        assert_eq!(ids(&draft), ["P1", "C1", "P2"]);
        assert_eq!(draft.sentences[1].reasons, [Reason::Number]);
        assert!(draft.sentences[0].reasons.is_empty());
        assert_eq!(draft.sentences[2].ordinal, 3);
        assert_eq!(draft.claims().count(), 1);
    }

    #[test]
    fn decimal_points_do_not_end_a_sentence() {
        let draft = parse_draft("We reach 71.3% success. It is 6.1 points better.\n").unwrap();
        assert_eq!(
            texts(&draft),
            ["We reach 71.3% success.", "It is 6.1 points better."]
        );
    }

    #[test]
    fn known_abbreviations_do_not_end_a_sentence() {
        let draft = parse_draft(
            "Lee et al. report 60% on it, e.g. on long tasks (see Fig. 2). Next sentence.\n",
        )
        .unwrap();
        assert_eq!(
            texts(&draft),
            [
                "Lee et al. report 60% on it, e.g. on long tasks (see Fig. 2).",
                "Next sentence."
            ]
        );
    }

    #[test]
    fn question_and_exclamation_marks_end_sentences() {
        let draft = parse_draft("Is it 2x faster? It is 2x faster! Mostly 2x\n").unwrap();
        assert_eq!(
            texts(&draft),
            ["Is it 2x faster?", "It is 2x faster!", "Mostly 2x"]
        );
    }

    #[test]
    fn lines_of_one_paragraph_are_joined() {
        let draft = parse_draft("This sentence\nspans 2 lines.\n").unwrap();
        assert_eq!(draft.sentences[0].text, "This sentence spans 2 lines.");
    }

    #[test]
    fn notes_tables_images_code_and_math_are_skipped() {
        let draft = parse_draft(
            "> A note with 1 number.\n\n| a | 1 |\n|---|---|\n\n![plot of 3 runs](p.png)\n\n```\nx = 1. y = 2.\n```\n\n$$\na = 0.5.\n$$\n\n$$ b = 2 $$\n\nA claim with 4 parts.\n",
        )
        .unwrap();
        assert_eq!(texts(&draft), ["A claim with 4 parts."]);
    }

    #[test]
    fn only_atx_headings_are_skipped_and_hash_text_is_a_sentence() {
        let draft = parse_draft(
            "# Title\n\n### Results\n\n#### Detail\n\n#1 reached 74.2% [run:r].\n\n#hashtag runs gave 3 wins.\n\n###\n\nLast claim has 2 parts.\n",
        )
        .unwrap();
        assert_eq!(
            texts(&draft),
            [
                "#1 reached 74.2% [run:r].",
                "#hashtag runs gave 3 wins.",
                "Last claim has 2 parts."
            ]
        );
    }

    #[test]
    fn a_tab_after_hashes_is_not_a_heading() {
        // SPEC: a heading's `#` run is followed by a space or the end of the line, so a
        // tab-separated line is a sentence, kept and checked rather than dropped.
        let draft = parse_draft("## Results\n\n##\tTuned run reached 74.2% [run:r].\n").unwrap();
        assert_eq!(texts(&draft), ["##\tTuned run reached 74.2% [run:r]."]);
        assert_eq!(draft.sentences[0].section, "Results");
        assert!(draft.sentences[0].is_claim());

        // Inside References it is not a heading either, so it is reported at its own line.
        let error =
            parse_draft("## References\n- [@a] One.\n##\tAppendix\nWe add 1 note.\n").unwrap_err();
        assert_eq!(error.line, 3, "{error}");
    }

    #[test]
    fn a_one_line_code_span_at_line_start_is_not_a_fence() {
        let draft =
            parse_draft("```x = 1``` sets the seed to 3.\n\nNext claim has 2 parts.\n").unwrap();
        assert_eq!(
            texts(&draft),
            ["```x = 1``` sets the seed to 3.", "Next claim has 2 parts."]
        );
    }

    #[test]
    fn inline_display_math_is_removed_and_the_rest_of_the_line_kept() {
        let draft = parse_draft("$$x = 1$$ where x is seed 7.\n").unwrap();
        assert_eq!(texts(&draft), ["where x is seed 7."]);
    }

    #[test]
    fn a_math_block_closes_at_a_line_ending_in_dollars() {
        let draft = parse_draft("$$\nx = y + z $$\nAfter the math there are 2 claims.\n").unwrap();
        assert_eq!(texts(&draft), ["After the math there are 2 claims."]);
    }

    #[test]
    fn a_fence_closes_only_with_a_bare_run_at_least_as_long() {
        let draft =
            parse_draft("````\n```\ncode 1.\n```\n````\nReal text has 2 claims.\n").unwrap();
        assert_eq!(texts(&draft), ["Real text has 2 claims."]);
        let draft = parse_draft("```\ncode\n``` not a close\n```\nText has 3 parts.\n").unwrap();
        assert_eq!(texts(&draft), ["Text has 3 parts."]);
    }

    #[test]
    fn an_unclosed_block_is_an_error_naming_its_opening_line() {
        assert_eq!(
            parse_draft("Intro has 1.\n\n```\ncode\n").unwrap_err().line,
            3
        );
        assert_eq!(
            parse_draft("Intro has 1.\n$$\nx = 1\n").unwrap_err().line,
            2
        );
    }

    #[test]
    fn a_sentence_ends_after_closing_brackets_and_quotes() {
        let draft =
            parse_draft("(Prior work reaches 60% [@lee2025].) Ours reaches 99.9%.\n").unwrap();
        assert_eq!(
            texts(&draft),
            [
                "(Prior work reaches 60% [@lee2025].)",
                "Ours reaches 99.9%."
            ]
        );
        assert!(draft.sentences[1].citations.is_empty());
        let draft = parse_draft("\"Prior work reaches 60%.\" Ours reaches 99.9%.\n").unwrap();
        assert_eq!(draft.sentences.len(), 2);
    }

    #[test]
    fn punctuation_inside_a_reference_marker_does_not_split() {
        let draft = parse_draft("See [@lab. ; @lee] for 2 results.\n").unwrap();
        assert_eq!(texts(&draft), ["See [@lab. ; @lee] for 2 results."]);
        assert_eq!(draft.sentences[0].citations, ["lab.", "lee"]);
    }

    #[test]
    fn no_is_an_abbreviation_only_before_a_number() {
        let draft = parse_draft("Does it need shaping? No. It reaches 99.9%.\n").unwrap();
        assert_eq!(
            texts(&draft),
            ["Does it need shaping?", "No.", "It reaches 99.9%."]
        );
        let draft = parse_draft("See No. 3 in the list.\n").unwrap();
        assert_eq!(draft.sentences.len(), 1);
    }

    #[test]
    fn al_is_an_abbreviation_only_after_et() {
        let draft = parse_draft("The team named it al. Then 2 runs failed.\n").unwrap();
        assert_eq!(draft.sentences.len(), 2);
    }

    #[test]
    fn a_title_heading_after_references_ends_the_references() {
        let draft =
            parse_draft("## References\n- [@a] One.\n# Appendix\nWe add 1 note.\n").unwrap();
        assert_eq!(texts(&draft), ["We add 1 note."]);
        assert_eq!(draft.sentences[0].section, "Appendix");
        assert_eq!(draft.sources.len(), 1);
    }

    #[test]
    fn each_list_item_is_its_own_paragraph() {
        let draft = parse_draft(
            "- First item has 1\n* Second item has 2.\n1. Third has 3\n2) Fourth has 4\n",
        )
        .unwrap();
        assert_eq!(
            texts(&draft),
            [
                "First item has 1",
                "Second item has 2.",
                "Third has 3",
                "Fourth has 4"
            ]
        );
    }

    #[test]
    fn extracts_all_three_reference_kinds() {
        let draft =
            parse_draft("We win [run:r-002] in [exp:E1], as shown before [@lee2025].\n").unwrap();
        let sentence = &draft.sentences[0];
        assert_eq!(sentence.runs, ["r-002"]);
        assert_eq!(sentence.experiments, ["E1"]);
        assert_eq!(sentence.citations, ["lee2025"]);
        assert_eq!(sentence.reasons, [Reason::Citation, Reason::Reference]);
    }

    #[test]
    fn a_citation_can_name_several_keys_with_semicolons_or_commas() {
        let draft = parse_draft("Known [@a2020; @b2021]. Also [@c2022, @d2023].\n").unwrap();
        assert_eq!(draft.sentences[0].citations, ["a2020", "b2021"]);
        assert_eq!(draft.sentences[1].citations, ["c2022", "d2023"]);
    }

    #[test]
    fn numbers_inside_reference_markers_are_not_figures() {
        let draft = parse_draft("Success rose [run:r-002] [@lee2025] [exp:E1].\n").unwrap();
        let sentence = &draft.sentences[0];
        assert!(sentence.figures.is_empty());
        assert!(!sentence.reasons.contains(&Reason::Number));
    }

    #[test]
    fn figures_are_extracted_from_claims() {
        let draft = parse_draft("We reach 71.3% and 6.1 points [run:r-002].\n").unwrap();
        let written: Vec<&str> = draft.sentences[0]
            .figures
            .iter()
            .map(|f| f.as_written.as_str())
            .collect();
        assert_eq!(written, ["71.3%", "6.1"]);
    }

    #[test]
    fn ordinary_markdown_links_are_not_references() {
        let draft = parse_draft("See [the site](https://example.org) for more.\n").unwrap();
        let sentence = &draft.sentences[0];
        assert!(sentence.citations.is_empty() && sentence.runs.is_empty());
        assert!(sentence.experiments.is_empty());
    }

    #[test]
    fn an_empty_reference_is_an_error() {
        let err = parse_draft("Title line\n\nBad ref [run:].\n").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn a_reference_error_names_the_line_inside_a_paragraph() {
        let err = parse_draft("First line is fine\nsecond line has [@].\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn references_section_becomes_sources_not_sentences() {
        let draft = parse_draft(
            "A claim [@lee2025].\n\n## References\n\n- [@lee2025] Lee. A paper. 2025.\n- [@chen2024] Chen. Another. 2024.\n",
        )
        .unwrap();
        assert_eq!(ids(&draft), ["C1"]);
        assert_eq!(
            draft.sources,
            [
                Source {
                    key: "lee2025".into(),
                    entry: "Lee. A paper. 2025.".into()
                },
                Source {
                    key: "chen2024".into(),
                    entry: "Chen. Another. 2024.".into()
                },
            ]
        );
    }

    #[test]
    fn a_malformed_references_line_reports_its_line_number() {
        let err = parse_draft("## References\n\n- [@ok] Fine.\nnot an entry\n").unwrap_err();
        assert_eq!(err.line, 4);
    }

    #[test]
    fn a_duplicate_source_key_is_an_error() {
        let err = parse_draft("## References\n- [@a] One.\n- [@a] Two.\n").unwrap_err();
        assert_eq!(err.line, 3);
    }
}
