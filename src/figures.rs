//! Numbers stated in a sentence, with the precision they were written with.

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
}

/// Extract numeric tokens in order, excluding numbers that are part of identifiers
/// (`r2`, `v1.2.3`, `gpt-3.5-turbo`, `UTF-8`), ordinals (`3rd`) and long glued suffixes.
/// A short unit suffix of up to three ASCII letters (`12.5ms`, `10k`, `3x`) is kept in
/// `as_written` and does not change the value.
pub fn extract_figures(text: &str) -> Vec<Figure> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let byte = |i: usize| chars.get(i).map_or(text.len(), |&(b, _)| b);
    let digits_from = |mut i: usize| {
        let begin = i;
        while at(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        i - begin
    };

    let mut figures = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !at(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        let start = i;
        i += digits_from(i);
        while at(i) == Some(',') && digits_from(i + 1) == 3 {
            i += 4;
        }
        let mut decimal_places = 0;
        if at(i) == Some('.') && at(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            decimal_places = digits_from(i + 1);
            i += 1 + decimal_places;
        }
        let number_end = i;

        // A further `.digit` makes this a dotted identifier such as 1.2.3: skip all of it.
        if at(i) == Some('.') && at(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            while at(i).is_some_and(|c| c.is_ascii_digit() || c == '.') {
                i += 1;
            }
            continue;
        }

        let previous = start.checked_sub(1).and_then(at);
        let before_previous = start.checked_sub(2).and_then(at);
        let mut negative = false;
        let blocked_before = match previous {
            Some(c) if is_word_character(c) => true,
            Some('.') => before_previous.is_some_and(char::is_alphanumeric),
            Some('-' | '\u{2212}') => match before_previous {
                None => {
                    negative = true;
                    false
                }
                Some(c) if c.is_alphabetic() || c == '_' => true,
                // A digit before the hyphen is a range ("10-20") unless that digit belongs
                // to an identifier ("x86-64").
                Some(c) if c.is_ascii_digit() => run_follows_letter(&chars, start - 2),
                Some(c) => {
                    negative = !c.is_alphanumeric();
                    false
                }
            },
            _ => false,
        };

        let mut end = i;
        let mut percent = false;
        let mut blocked_after = false;
        match at(end) {
            Some('%') => {
                percent = true;
                end += 1;
                blocked_after = at(end).is_some_and(is_word_character);
            }
            Some('\u{00D7}') => {
                end += 1;
                blocked_after = at(end).is_some_and(is_word_character);
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let mut k = end;
                while at(k).is_some_and(|c| c.is_ascii_alphabetic()) {
                    k += 1;
                }
                let suffix = text[byte(end)..byte(k)].to_ascii_lowercase();
                blocked_after = k - end > 3
                    || at(k).is_some_and(|c| is_word_character(c) || c == '-')
                    || matches!(suffix.as_str(), "st" | "nd" | "rd" | "th");
                end = k;
            }
            Some('-') => blocked_after = at(end + 1).is_some_and(char::is_alphabetic),
            Some(c) => blocked_after = is_word_character(c),
            None => {}
        }
        if blocked_before || blocked_after {
            i = end.max(i);
            continue;
        }
        if end == number_end && !percent {
            percent = followed_by_points(&text[byte(end)..]);
        }

        let digits = text[byte(start)..byte(number_end)].replace(',', "");
        let magnitude = digits
            .parse::<f64>()
            .expect("numeric token has valid digits");
        let raw = if negative { -magnitude } else { magnitude };
        let raw_tolerance = 0.5 * 10.0_f64.powf(-(decimal_places as f64));
        let scale = if percent { 100.0 } else { 1.0 };
        let written_start = if negative { start - 1 } else { start };
        figures.push(Figure {
            as_written: text[byte(written_start)..byte(end)].to_string(),
            value: raw / scale,
            tolerance: raw_tolerance / scale,
            raw,
            raw_tolerance,
            percent,
        });
        i = end;
    }
    figures
}

/// Whether the run of digits and dots ending at `index` starts right after a letter.
fn run_follows_letter(chars: &[(usize, char)], index: usize) -> bool {
    let mut k = index;
    while k > 0 && (chars[k - 1].1.is_ascii_digit() || chars[k - 1].1 == '.') {
        k -= 1;
    }
    k > 0 && (chars[k - 1].1.is_alphabetic() || chars[k - 1].1 == '_')
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

fn is_word_character(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, expected: &[(&str, f64, f64, f64, f64, bool)]) {
        let figures = extract_figures(text);
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

    #[test]
    fn no_figures() {
        check("", &[]);
        check("No figures here, just words and punctuation! １２ ٣", &[]);
    }
}
