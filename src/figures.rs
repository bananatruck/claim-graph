//! Numbers stated in a sentence, with the precision they were written with.
//!
//! The text is read as words, and each whole word either is a figure or holds none.
//! A word is a maximal run of letters, digits, `_`, `.`, `,`, `+`, `%`, `×` and the
//! dashes `-`, `−` (minus sign) and `–` (en dash); any other character separates words.
//! Trailing `.` and `,` and leading `,` and `+` are punctuation, not part of the word.
//! A word is then read by this grammar, first rule that fits:
//!
//! ```text
//! word     = figure                 one figure
//!          | figure dash figure     a range: two figures ("10-20", "71.3%-73.9%")
//!          | piece ("," piece)+     a list written without spaces: each piece read again;
//!                                   a comma the number rule reads as a thousands
//!                                   separator stays inside its piece ("1,024.5,2.5" is
//!                                   1,024.5 and 2.5)
//!          | anything else          no figure: an identifier such as v1.2.3, UTF-8,
//!                                   gpt-3.5-turbo, x86-64 or Llama-3.1-8B
//! figure   = sign? number exponent? suffix?
//! sign     = "-" | "−"
//! number   = digits ("," 3 digits)* ("." digits)? | "." digits
//! exponent = ("e" | "E") ("-" | "+" | "−")? 1 to 3 digits
//! suffix   = "%" | "×" | 1 to 3 ASCII letters other than st, nd, rd, th
//! ```
//!
//! A figure whose value or written precision a 64-bit float cannot hold, such as `1e-999`
//! (both would become 0 and match a metric of 0) or `1e999` (infinite), is an error, so
//! the draft is refused rather than the number read as a value it does not have. The
//! value and the precision must be finite, and the precision a normal float, at least
//! about 2.2e-308, on both the written and the percent scale.
//!
//! A figure is a measurement when it is a percentage or has a decimal point or a written
//! exponent, `e0` included; a bare integer is usually a count ("3 seeds", "2024").

/// A numeric token with its written precision and optional percent scaling.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    /// The token as written, sign and unit suffix included: "71.3%", "-0.25", "12.5ms".
    pub as_written: String,
    /// On the fraction scale when `percent` is set ("71.3%" and "6.1 points" give 0.713 and
    /// 0.061), otherwise the number itself.
    pub value: f64,
    /// Half a unit in the last written digit, on the scale of `value`.
    pub tolerance: f64,
    /// The number as written, without percent scaling.
    pub raw: f64,
    /// Half a unit in the last written digit, on the scale of `raw`.
    pub raw_tolerance: f64,
    /// Written with `%`, or followed by the word "points", "point" or "pp".
    pub percent: bool,
    /// A percentage, or written with a decimal point or an exponent.
    pub measured: bool,
}

/// A figure whose value or written precision a 64-bit float cannot hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Unrepresentable {
    /// The figure as written.
    pub as_written: String,
    /// Its byte offset in the text given to `extract_figures`.
    pub offset: usize,
}

const DASHES: [char; 3] = ['-', '\u{2212}', '\u{2013}'];

fn is_word_part(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(c, '_' | '.' | ',' | '+' | '%' | '\u{00D7}')
        || DASHES.contains(&c)
}

/// Extract the figures of `text` in order, by the grammar in the module documentation,
/// or the first figure a 64-bit float cannot hold.
pub fn extract_figures(text: &str) -> Result<Vec<Figure>, Unrepresentable> {
    let mut figures = Vec::new();
    let mut rest = text;
    while let Some(begin) = rest.find(is_word_part) {
        let tail = &rest[begin..];
        let length = tail.find(|c| !is_word_part(c)).unwrap_or(tail.len());
        let word = &tail[..length];
        let offset = text.len() - tail.len();
        let trimmed = word.trim_end_matches(['.', ',']);
        let inner = trimmed.trim_start_matches([',', '+']);
        read_word(
            text,
            offset + (trimmed.len() - inner.len()),
            inner,
            &mut figures,
        );
        rest = &tail[length..];
    }
    figures.into_iter().collect()
}

type FigureRead = Result<Figure, Unrepresentable>;

/// Read one word that starts at byte `offset` of `text`.
fn read_word(text: &str, offset: usize, word: &str, figures: &mut Vec<FigureRead>) {
    if word.is_empty() {
        return;
    }
    if let Some(figure) = figure(text, offset, word) {
        figures.push(figure);
        return;
    }
    for (at, dash) in word.char_indices().filter(|&(_, c)| DASHES.contains(&c)) {
        let (left, right) = (&word[..at], &word[at + dash.len_utf8()..]);
        let right_offset = offset + at + dash.len_utf8();
        if let (Some(a), Some(b)) = (
            figure(text, offset, left),
            figure(text, right_offset, right),
        ) {
            figures.extend([a, b]);
            return;
        }
    }
    let splits = list_commas(word);
    if !splits.is_empty() {
        let mut start = 0;
        for end in splits.into_iter().chain([word.len()]) {
            read_word(text, offset + start, &word[start..end], figures);
            start = end + 1;
        }
    }
}

/// The byte positions of the commas that separate a list written without spaces: every
/// comma except one the number rule reads as a thousands separator, which follows the
/// integer digits of its piece and precedes exactly three digits ("1,024.5,2.5" splits
/// only before "2.5").
fn list_commas(word: &str) -> Vec<usize> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let mut splits = Vec::new();
    let mut start = 0;
    for (at, _) in word.match_indices(',') {
        let piece = word[start..at].trim_start_matches(['-', '\u{2212}']);
        let integer = !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_digit() || b == b',');
        if !(integer && digits(&word[at + 1..]) == 3) {
            splits.push(at);
            start = at + 1;
        }
    }
    splits
}

/// Read `word`, found at byte `offset` of `text`, as one figure: `None` when the grammar
/// does not make it a figure, an error when it does but a float cannot hold it.
fn figure(text: &str, offset: usize, word: &str) -> Option<FigureRead> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let (negative, mut rest) = match word.strip_prefix(['-', '\u{2212}']) {
        Some(rest) => (true, rest),
        None => (false, word),
    };

    let mut number = String::new();
    let whole = digits(rest);
    number.push_str(&rest[..whole]);
    rest = &rest[whole..];
    while whole > 0 && rest.starts_with(',') && digits(&rest[1..]) == 3 {
        number.push_str(&rest[1..4]);
        rest = &rest[4..];
    }
    let mut decimal_places = 0;
    if let Some(after) = rest.strip_prefix('.') {
        decimal_places = digits(after);
        if decimal_places > 0 {
            number.push('.');
            number.push_str(&after[..decimal_places]);
            rest = &after[decimal_places..];
        }
    }
    if whole == 0 && decimal_places == 0 {
        return None;
    }

    // `has_exponent` records that an exponent was written, so `3e0` is a measurement.
    let mut exponent = 0_i32;
    let mut has_exponent = false;
    if let Some(after) = rest.strip_prefix(['e', 'E']) {
        let (sign, unsigned) = match after.strip_prefix(['-', '\u{2212}']) {
            Some(unsigned) => (-1, unsigned),
            None => (1, after.strip_prefix('+').unwrap_or(after)),
        };
        let n = digits(unsigned);
        if (1..=3).contains(&n) {
            exponent = sign * unsigned[..n].parse::<i32>().ok()?;
            has_exponent = true;
            rest = &unsigned[n..];
        }
    }

    let percent_sign = rest == "%";
    let suffix_ok = rest.is_empty()
        || percent_sign
        || rest == "\u{00D7}"
        || ((1..=3).contains(&rest.len())
            && rest.bytes().all(|b| b.is_ascii_alphabetic())
            && !matches!(
                rest.to_ascii_lowercase().as_str(),
                "st" | "nd" | "rd" | "th"
            ));
    if !suffix_ok {
        return None;
    }
    let end = offset + word.len();
    let percent = percent_sign || (rest.is_empty() && followed_by_points(&text[end..]));

    let magnitude = if exponent == 0 {
        format!("0{number}").parse::<f64>().ok()?
    } else {
        format!("0{number}e{exponent}").parse::<f64>().ok()?
    };
    let raw = if negative { -magnitude } else { magnitude };
    let raw_tolerance = 0.5 * 10.0_f64.powf(f64::from(exponent) - decimal_places as f64);
    let scale = if percent { 100.0 } else { 1.0 };
    let (value, tolerance) = (raw / scale, raw_tolerance / scale);
    if !(raw.is_finite() && value.is_finite() && raw_tolerance.is_normal() && tolerance.is_normal())
    {
        return Some(Err(Unrepresentable {
            as_written: word.to_string(),
            offset,
        }));
    }
    Some(Ok(Figure {
        as_written: word.to_string(),
        value,
        tolerance,
        raw,
        raw_tolerance,
        percent,
        measured: percent || decimal_places > 0 || has_exponent,
    }))
}

/// Whether the text after a number starts with whitespace and the word "points", "point"
/// or "pp".
fn followed_by_points(rest: &str) -> bool {
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        return false;
    }
    let word: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    matches!(word.as_str(), "points" | "point" | "pp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, expected: &[(&str, f64, f64, f64, f64, bool)]) {
        let figures = extract_figures(text).unwrap();
        assert_eq!(figures.len(), expected.len(), "{text:?}: {figures:?}");
        for (figure, &(written, value, tolerance, raw, raw_tolerance, percent)) in
            figures.iter().zip(expected)
        {
            assert_eq!(figure.as_written, written);
            assert_eq!(figure.percent, percent);
            for (actual, expected) in [
                (figure.value, value),
                (figure.tolerance, tolerance),
                (figure.raw, raw),
                (figure.raw_tolerance, raw_tolerance),
            ] {
                assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
            }
        }
    }

    #[test]
    fn percent_figure() {
        check(
            "We reach 71.3% success.",
            &[("71.3%", 0.713, 0.0005, 71.3, 0.05, true)],
        );
        check("65%", &[("65%", 0.65, 0.005, 65.0, 0.5, true)]);
    }

    #[test]
    fn plain_decimal() {
        check(
            "Score 0.652.",
            &[("0.652", 0.652, 0.0005, 0.652, 0.0005, false)],
        );
        check("12.00", &[("12.00", 12.0, 0.005, 12.0, 0.005, false)]);
    }

    #[test]
    fn integer_and_trailing_period() {
        check("in 2024.", &[("2024", 2024.0, 0.5, 2024.0, 0.5, false)]);
        check("12", &[("12", 12.0, 0.5, 12.0, 0.5, false)]);
    }

    #[test]
    fn thousands_separator() {
        check(
            "1,024 and 12,500",
            &[
                ("1,024", 1024.0, 0.5, 1024.0, 0.5, false),
                ("12,500", 12500.0, 0.5, 12500.0, 0.5, false),
            ],
        );
        check(
            "1,234,567.80%",
            &[("1,234,567.80%", 12345.678, 0.00005, 1234567.8, 0.005, true)],
        );
    }

    #[test]
    fn comma_list_is_two_figures() {
        check(
            "1, 2",
            &[
                ("1", 1.0, 0.5, 1.0, 0.5, false),
                ("2", 2.0, 0.5, 2.0, 0.5, false),
            ],
        );
        check(
            "1,23 and 1,2345",
            &[
                ("1", 1.0, 0.5, 1.0, 0.5, false),
                ("23", 23.0, 0.5, 23.0, 0.5, false),
                ("1", 1.0, 0.5, 1.0, 0.5, false),
                ("2345", 2345.0, 0.5, 2345.0, 0.5, false),
            ],
        );
    }

    #[test]
    fn a_list_split_keeps_thousands_groups_whole() {
        // A comma the number rule reads as a thousands separator is not a list comma.
        check(
            "1,024.5,2.5",
            &[
                ("1,024.5", 1024.5, 0.05, 1024.5, 0.05, false),
                ("2.5", 2.5, 0.05, 2.5, 0.05, false),
            ],
        );
    }

    #[test]
    fn multiplier_suffix() {
        check(
            "3x faster, 2.50× speed.",
            &[
                ("3x", 3.0, 0.5, 3.0, 0.5, false),
                ("2.50×", 2.5, 0.005, 2.5, 0.005, false),
            ],
        );
    }

    #[test]
    fn numbers_inside_words_are_ignored() {
        check("r2 E1 v1.2 ABC4 _12 12_ é2 2é α.12", &[]);
    }

    #[test]
    fn ordinals_and_long_or_glued_suffixes_are_ignored() {
        check("3rd 1st 22nd 4th 3xfaster 3×faster 65%of 2x_ 2×٣ 2é", &[]);
    }

    #[test]
    fn short_unit_suffixes_keep_the_figure() {
        check(
            "12.5ms and 4.2GB and 10k",
            &[
                ("12.5ms", 12.5, 0.05, 12.5, 0.05, false),
                ("4.2GB", 4.2, 0.05, 4.2, 0.05, false),
                ("10k", 10.0, 0.5, 10.0, 0.5, false),
            ],
        );
    }

    #[test]
    fn a_leading_minus_belongs_to_a_standalone_number() {
        check(
            "Loss was -0.25 and accuracy changed by \u{2212}3.1% (-2).",
            &[
                ("-0.25", -0.25, 0.005, -0.25, 0.005, false),
                ("\u{2212}3.1%", -0.031, 0.0005, -3.1, 0.05, true),
                ("-2", -2.0, 0.5, -2.0, 0.5, false),
            ],
        );
        // A hyphen after a number is a range, not a sign.
        check(
            "10-20",
            &[
                ("10", 10.0, 0.5, 10.0, 0.5, false),
                ("20", 20.0, 0.5, 20.0, 0.5, false),
            ],
        );
    }

    #[test]
    fn a_hyphen_after_a_percentage_is_a_range() {
        check(
            "71.3%-73.9%",
            &[
                ("71.3%", 0.713, 0.0005, 71.3, 0.05, true),
                ("73.9%", 0.739, 0.0005, 73.9, 0.05, true),
            ],
        );
    }

    #[test]
    fn dotted_and_hyphenated_identifiers_are_not_figures() {
        check(
            "release v1.2.3.4 with gpt-3.5-turbo in section 1.2.3, UTF-8 and x86-64",
            &[],
        );
        check("We describe v1.2.3.", &[]);
    }

    #[test]
    fn points_are_on_the_percent_scale() {
        check(
            "It is 6.1 points above and 2 pp below.",
            &[
                ("6.1", 0.061, 0.0005, 6.1, 0.05, true),
                ("2", 0.02, 0.005, 2.0, 0.5, true),
            ],
        );
    }

    #[test]
    fn several_figures_in_order() {
        check(
            "In 2024, 71.3% of 1,024 runs were 3x faster at 0.652.",
            &[
                ("2024", 2024.0, 0.5, 2024.0, 0.5, false),
                ("71.3%", 0.713, 0.0005, 71.3, 0.05, true),
                ("1,024", 1024.0, 0.5, 1024.0, 0.5, false),
                ("3x", 3.0, 0.5, 3.0, 0.5, false),
                ("0.652", 0.652, 0.0005, 0.652, 0.0005, false),
            ],
        );
    }

    /// One row per grammar rule: the text and the figures it yields, as written.
    #[test]
    fn each_word_is_read_by_the_grammar() {
        for (text, expected) in [
            ("71.3%", &["71.3%"][..]),
            ("(-2).", &["-2"]),
            ("+3.1%", &["3.1%"]),
            ("p<0.05, n=12", &["0.05", "12"]),
            ("1,024,", &["1,024"]),
            (".5 and -.5", &[".5", "-.5"]),
            ("1e-3 1.5E2 2e", &["1e-3", "1.5E2", "2e"]),
            ("1e1234", &[]),
            ("10-20 1.0e-3-2e-3", &["10", "20", "1.0e-3", "2e-3"]),
            ("1,23", &["1", "23"]),
            ("1,024.5,2.5 1,234,5", &["1,024.5", "2.5", "1,234", "5"]),
            ("1.5,250,3 -1,000,2%", &["1.5", "250", "3", "-1,000", "2%"]),
            ("1,000abcd,5", &["5"]),
            (
                "12.5ms 4.2GB 3x 2.50\u{00D7}",
                &["12.5ms", "4.2GB", "3x", "2.50\u{00D7}"],
            ),
            ("3rd 3xfaster 65%of 12_ r2", &[]),
            ("v1.2.3 1.2.3 UTF-8 x86-64 Llama-3.1-8B 3-fold", &[]),
        ] {
            let written: Vec<String> = extract_figures(text)
                .unwrap()
                .into_iter()
                .map(|f| f.as_written)
                .collect();
            assert_eq!(written, expected, "{text:?}");
        }
    }

    #[test]
    fn leading_dot_decimals_are_measurements() {
        check(
            "The score is .713 and the shift -.5.",
            &[
                (".713", 0.713, 0.0005, 0.713, 0.0005, false),
                ("-.5", -0.5, 0.05, -0.5, 0.05, false),
            ],
        );
        assert!(extract_figures(".713").unwrap()[0].measured);
    }

    #[test]
    fn scientific_notation_scales_value_and_precision() {
        check(
            "The loss is 1.0e-3 and the rate 2.5E+4, timed at 1.0ms.",
            &[
                ("1.0e-3", 0.001, 0.00005, 0.001, 0.00005, false),
                ("2.5E+4", 25000.0, 500.0, 25000.0, 500.0, false),
                ("1.0ms", 1.0, 0.05, 1.0, 0.05, false),
            ],
        );
        assert!(extract_figures("1e-3").unwrap()[0].measured);
        assert!(!extract_figures("12").unwrap()[0].measured);
    }

    #[test]
    fn a_written_exponent_is_a_measurement_even_when_zero() {
        // The grammar makes a figure with an exponent a measurement; `e0` is written.
        check("3e0", &[("3e0", 3.0, 0.5, 3.0, 0.5, false)]);
        assert!(extract_figures("3e0").unwrap()[0].measured);
        assert!(extract_figures("3E+0").unwrap()[0].measured);
        // No exponent: a bare integer, and `2e`, whose `e` is a unit suffix.
        assert!(!extract_figures("3").unwrap()[0].measured);
        assert!(!extract_figures("2e").unwrap()[0].measured);
    }

    #[test]
    fn numbers_inside_hyphenated_identifiers_are_not_figures() {
        check(
            "We describe Llama-3.1-8B, GPT-4o, COVID-19 and ResNet-50-v2.",
            &[],
        );
        check(
            "Ranges 10-20, 5\u{2013}10% and 0.1-0.2 stay ranges.",
            &[
                ("10", 10.0, 0.5, 10.0, 0.5, false),
                ("20", 20.0, 0.5, 20.0, 0.5, false),
                ("5", 5.0, 0.5, 5.0, 0.5, false),
                ("10%", 0.1, 0.005, 10.0, 0.5, true),
                ("0.1", 0.1, 0.05, 0.1, 0.05, false),
                ("0.2", 0.2, 0.05, 0.2, 0.05, false),
            ],
        );
    }

    #[test]
    fn a_number_a_float_cannot_hold_is_an_error_not_a_value() {
        // Read as a float, 1e-999 and its precision would both become 0 and match a
        // metric of 0, and 1e999 would become infinite, so these are refused.
        let long_integer = format!("1{}", "0".repeat(400));
        let long_decimal = format!("0.{}1", "0".repeat(400));
        for text in [
            "1e-999",
            "1e999",
            "-1e999",
            "0.0e-999",
            "1e-310",
            "1e-307%",
            "1e-999-2e-3",
            "loss 1e-999, then 2.5",
            "v1e-999 then 1e-999",
            &long_integer,
            &long_decimal,
        ] {
            let error = extract_figures(text).unwrap_err();
            assert!(
                text[error.offset..].starts_with(&error.as_written),
                "{text:?}: {error:?}"
            );
        }
        // Representable exponents stay figures.
        for text in ["1e-300", "1e300", "1e-3", "2.5E+4", "1e-300%"] {
            assert_eq!(extract_figures(text).unwrap().len(), 1, "{text:?}");
        }
    }

    #[test]
    fn no_figures() {
        check("", &[]);
        check("No figures here, just words and punctuation! １２ ٣", &[]);
    }
}
