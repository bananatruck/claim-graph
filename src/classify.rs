//! The rule that decides which sentences are checkable claims.
//!
//! A sentence is a checkable claim when it contains a number, a comparison word,
//! a citation, or a link to a run or an experiment. Every other sentence is prose.
//! The rule lives here, behind one function, so it can be replaced without touching
//! the parser or the store.

use crate::figures::extract_figures;

/// Why a sentence counts as a checkable claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Number,
    Comparison,
    Citation,
    Reference,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Number => "number",
            Reason::Comparison => "comparison",
            Reason::Citation => "citation",
            Reason::Reference => "reference",
        }
    }
}

/// Words that state a comparison, matched case-insensitively as whole words.
pub const COMPARISON_WORDS: &[&str] = &[
    "above",
    "below",
    "better",
    "worse",
    "best",
    "worst",
    "higher",
    "lower",
    "more",
    "less",
    "most",
    "least",
    "fewer",
    "greater",
    "smaller",
    "larger",
    "faster",
    "slower",
    "than",
    "versus",
    "vs",
    "outperform",
    "outperforms",
    "outperformed",
    "exceed",
    "exceeds",
    "exceeded",
    "surpass",
    "surpasses",
    "beat",
    "beats",
    "improve",
    "improves",
    "improved",
    "improvement",
    "increase",
    "increases",
    "increased",
    "decrease",
    "decreases",
    "decreased",
    "reduce",
    "reduces",
    "reduced",
    "halves",
    "doubles",
    "triples",
    "match",
    "matches",
    "matched",
];

/// Classify one sentence. `text` is the sentence with reference markers removed.
/// An empty result means the sentence is prose.
pub fn classify(text: &str, has_citation: bool, has_run_or_experiment: bool) -> Vec<Reason> {
    let mut reasons = Vec::new();
    if !extract_figures(text).is_empty() {
        reasons.push(Reason::Number);
    }
    let compares = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .any(|word| {
            COMPARISON_WORDS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(word))
        });
    if compares {
        reasons.push(Reason::Comparison);
    }
    if has_citation {
        reasons.push(Reason::Citation);
    }
    if has_run_or_experiment {
        reasons.push(Reason::Reference);
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_makes_a_claim() {
        assert_eq!(
            classify("We reach 71.3% success.", false, false),
            [Reason::Number]
        );
    }

    #[test]
    fn a_comparison_word_makes_a_claim() {
        assert_eq!(
            classify("The planner halves the tool calls.", false, false),
            [Reason::Comparison]
        );
        assert_eq!(
            classify("It is Better Than the baseline.", false, false),
            [Reason::Comparison]
        );
    }

    #[test]
    fn references_make_a_claim() {
        assert_eq!(
            classify("Retrieval helps on long tasks.", true, true),
            [Reason::Citation, Reason::Reference]
        );
    }

    #[test]
    fn plain_prose_is_not_a_claim() {
        assert!(classify("We study a planner for a toy research agent.", false, false).is_empty());
        assert!(classify("This section describes the setup.", false, false).is_empty());
    }

    #[test]
    fn comparison_words_must_be_whole_words() {
        // "moreover" contains "more", "thane" contains "than".
        assert!(classify("Moreover, the thane agreed.", false, false).is_empty());
    }

    #[test]
    fn numbers_inside_identifiers_do_not_count() {
        assert!(classify("We describe model v2 in the appendix.", false, false).is_empty());
    }
}
