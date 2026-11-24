/// A numeric token with its written precision and optional percent scaling.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub as_written: String,
    pub value: f64,
    pub tolerance: f64,
    pub raw: f64,
    pub raw_tolerance: f64,
    pub percent: bool,
}

/// Extract numeric tokens in order, excluding numbers embedded in words.
pub fn extract_figures(text: &str) -> Vec<Figure> {
    let bytes = text.as_bytes();
    let mut figures = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let ch = text[index..].chars().next().expect("index is in the text");
        if !ch.is_ascii_digit() {
            index += ch.len_utf8();
            continue;
        }

        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        while bytes.get(index) == Some(&b',') {
            let group_start = index + 1;
            let mut group_end = group_start;
            while bytes.get(group_end).is_some_and(u8::is_ascii_digit) {
                group_end += 1;
            }
            if group_end - group_start != 3 {
                break;
            }
            index = group_end;
        }

        let mut decimal_places = 0;
        if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
            index += 1;
            let decimal_start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            decimal_places = index - decimal_start;
        }
        let number_end = index;
        let percent = bytes.get(index) == Some(&b'%');
        if let Some(suffix @ ('%' | 'x' | '×')) = text[index..].chars().next() {
            index += suffix.len_utf8();
        }

        let mut before = text[..start].chars().rev();
        let previous = before.next();
        let blocked_before = previous.is_some_and(is_word_character)
            || (previous == Some('.') && before.next().is_some_and(char::is_alphabetic));
        let blocked_after = text[index..].chars().next().is_some_and(is_word_character);
        if blocked_before || blocked_after {
            continue;
        }

        let digits = text[start..number_end].replace(',', "");
        let raw = digits
            .parse::<f64>()
            .expect("numeric token has valid digits");
        let raw_tolerance = 0.5 * 10.0_f64.powf(-(decimal_places as f64));
        let scale = if percent { 100.0 } else { 1.0 };
        figures.push(Figure {
            as_written: text[start..index].to_string(),
            value: raw / scale,
            tolerance: raw_tolerance / scale,
            raw,
            raw_tolerance,
            percent,
        });
    }
    figures
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
    fn ordinal_and_unit_suffixes_are_ignored() {
        check("3rd 2b 10k 12.5kg 3xfaster 3×faster 65%of 2x_ 2×٣", &[]);
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
