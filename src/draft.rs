//! Deterministic parser for a research draft written in Markdown.
//!
//! See SPEC.md, section "Draft (Markdown)", for the input contract.

use std::fmt;

/// A parsed draft: its title, its claims in reading order, and its bibliography.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub title: String,
    pub claims: Vec<Claim>,
    pub sources: Vec<Source>,
}

/// One sentence of the draft, with the references it makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    /// `C1`, `C2`, ... in reading order.
    pub id: String,
    /// 1-based position in reading order.
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

/// Parse a draft. Every sentence outside the References section becomes a claim.
pub fn parse_draft(input: &str) -> Result<Draft, ParseError> {
    let mut parser = Parser::default();
    for (index, raw) in input.lines().enumerate() {
        parser.line(index + 1, raw)?;
    }
    parser.flush()?;
    Ok(Draft {
        title: parser.title.unwrap_or_default(),
        claims: parser.claims,
        sources: parser.sources,
    })
}

#[derive(Default)]
struct Parser {
    title: Option<String>,
    section: String,
    in_references: bool,
    paragraph: Paragraph,
    claims: Vec<Claim>,
    sources: Vec<Source>,
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
        let trimmed = raw.trim();
        if let Some(heading) = trimmed.strip_prefix("# ") {
            self.flush()?;
            if self.title.is_none() {
                self.title = Some(heading.trim().to_string());
            }
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            self.flush()?;
            self.section = heading.trim().to_string();
            self.in_references = self.section.eq_ignore_ascii_case("references");
        } else if trimmed.starts_with('#') || trimmed.starts_with('>') || trimmed.is_empty() {
            // Deeper headings, notes and blank lines end a paragraph and are not claims.
            self.flush()?;
        } else if self.in_references {
            self.source(line, trimmed)?;
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            self.flush()?;
            self.paragraph.push(line, item.trim());
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
            let ordinal = self.claims.len() + 1;
            self.claims.push(Claim {
                id: format!("C{ordinal}"),
                ordinal,
                section: self.section.clone(),
                text: sentence.to_string(),
                citations: refs.citations,
                runs: refs.runs,
                experiments: refs.experiments,
            });
        }
        Ok(())
    }
}

/// Split text into trimmed sentences with their byte offsets.
///
/// A sentence ends at `.`, `!` or `?` followed by whitespace or the end of the text.
fn split_sentences(text: &str) -> Vec<(usize, &str)> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let at_boundary = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
        if matches!(ch, '.' | '!' | '?') && at_boundary {
            let end = index + ch.len_utf8();
            push_trimmed(&mut sentences, text, start, end);
            start = end;
        }
    }
    push_trimmed(&mut sentences, text, start, text.len());
    sentences
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
}

/// Find `[@key; @key]`, `[run:ID]` and `[exp:ID]` markers.
/// Other bracketed text, such as Markdown links, is left alone.
/// Errors carry the byte offset of the offending marker.
fn extract_references(sentence: &str) -> Result<References, (usize, String)> {
    let mut refs = References::default();
    let mut search_from = 0;
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
                for part in body.split(';') {
                    let key = part.trim().strip_prefix('@').ok_or_else(|| bad("citation"))?;
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
        search_from = open + 1 + close + 1;
    }
    Ok(refs)
}

enum Kind {
    Citation,
    Run,
    Experiment,
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(draft: &Draft) -> Vec<&str> {
        draft.claims.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn reads_title_sections_and_numbers_claims_in_order() {
        let draft = parse_draft(
            "# A title\n\n## Abstract\n\nFirst claim. Second claim.\n\n## Results\n\nThird claim.\n",
        )
        .unwrap();
        assert_eq!(draft.title, "A title");
        assert_eq!(ids(&draft), ["C1", "C2", "C3"]);
        assert_eq!(draft.claims[0].text, "First claim.");
        assert_eq!(draft.claims[1].text, "Second claim.");
        assert_eq!(draft.claims[1].section, "Abstract");
        assert_eq!(draft.claims[2].section, "Results");
        assert_eq!(draft.claims[2].ordinal, 3);
    }

    #[test]
    fn decimal_points_do_not_end_a_sentence() {
        let draft = parse_draft("We reach 71.3% success. It is 6.1 points better.\n").unwrap();
        assert_eq!(draft.claims.len(), 2);
        assert_eq!(draft.claims[0].text, "We reach 71.3% success.");
        assert_eq!(draft.claims[1].text, "It is 6.1 points better.");
    }

    #[test]
    fn question_and_exclamation_marks_end_sentences() {
        let draft = parse_draft("Does it work? It does! Mostly\n").unwrap();
        let texts: Vec<&str> = draft.claims.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["Does it work?", "It does!", "Mostly"]);
    }

    #[test]
    fn lines_of_one_paragraph_are_joined() {
        let draft = parse_draft("This sentence\nspans two lines.\n").unwrap();
        assert_eq!(draft.claims[0].text, "This sentence spans two lines.");
    }

    #[test]
    fn notes_are_ignored() {
        let draft = parse_draft("> A note. Not a claim.\n\nA claim.\n").unwrap();
        assert_eq!(ids(&draft), ["C1"]);
        assert_eq!(draft.claims[0].text, "A claim.");
    }

    #[test]
    fn each_list_item_is_its_own_paragraph() {
        let draft = parse_draft("- First item\n- Second item.\n").unwrap();
        let texts: Vec<&str> = draft.claims.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["First item", "Second item."]);
    }

    #[test]
    fn extracts_all_three_reference_kinds() {
        let draft =
            parse_draft("We win [run:r-002] in [exp:E1], as shown before [@lee2025].\n").unwrap();
        let claim = &draft.claims[0];
        assert_eq!(claim.runs, ["r-002"]);
        assert_eq!(claim.experiments, ["E1"]);
        assert_eq!(claim.citations, ["lee2025"]);
    }

    #[test]
    fn a_citation_can_name_several_keys() {
        let draft = parse_draft("Known result [@a2020; @b2021].\n").unwrap();
        assert_eq!(draft.claims[0].citations, ["a2020", "b2021"]);
    }

    #[test]
    fn ordinary_markdown_links_are_not_references() {
        let draft = parse_draft("See [the site](https://example.org) for more.\n").unwrap();
        let claim = &draft.claims[0];
        assert!(claim.citations.is_empty() && claim.runs.is_empty());
        assert!(claim.experiments.is_empty());
    }

    #[test]
    fn an_empty_reference_is_an_error() {
        let err = parse_draft("Title line\n\nBad ref [run:].\n").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn references_section_becomes_sources_not_claims() {
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

    #[test]
    fn the_synthetic_draft_parses() {
        let draft = parse_draft(include_str!("../data/synthetic/draft.md")).unwrap();
        assert_eq!(draft.claims.len(), 7);
        assert_eq!(draft.sources.len(), 2);
        assert_eq!(draft.claims[3].experiments, ["E2"]);
        assert_eq!(draft.claims[6].citations, ["okafor2023"]);
    }
}
