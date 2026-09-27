//! Tokens no text step may change: web and mail addresses, anything with a digit
//! (numbers, times, versions, prices), code and file paths, identifiers written in
//! mixed case or with underscores, acronyms, and anything in backticks.

use std::ops::Range;

/// A word as it appears in the text: its byte range, without surrounding punctuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub range: Range<usize>,
    pub text: String,
}

const EDGE: &[char] = &[
    '.', ',', ';', ':', '!', '?', '"', '\'', '(', ')', '[', ']', '{', '}', '“', '”', '‘', '’', '«',
    '»',
];

/// Whitespace-separated tokens with edge punctuation trimmed (a URL keeps its inner
/// dots and slashes; a sentence-final full stop is not part of the word).
pub fn tokens(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                let raw = &text[s..i];
                let trimmed_start = raw.len() - raw.trim_start_matches(EDGE).len();
                let core = raw.trim_matches(EDGE);
                if !core.is_empty() {
                    let a = s + trimmed_start;
                    out.push(Word {
                        range: a..a + core.len(),
                        text: core.to_owned(),
                    });
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// True for a token that must reach the user exactly as dictated.
pub fn is_protected(token: &str) -> bool {
    let t = token;
    let lower = t.to_ascii_lowercase();
    let has = |c: char| t.contains(c);
    let inner_upper = t
        .char_indices()
        .skip(1)
        .any(|(i, c)| c.is_uppercase() && t[..i].chars().last().is_some_and(char::is_lowercase));
    let letters: Vec<char> = t.chars().filter(|c| c.is_alphabetic()).collect();
    let acronym = letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase());
    lower.contains("://")
        || lower.starts_with("www.")
        || (has('@') && t.rsplit('@').next().is_some_and(|d| d.contains('.')))
        || t.chars().any(|c| c.is_ascii_digit())
        || has('_')
        || has('/')
        || has('\\')
        || has('`')
        || has('#')
        || has('=')
        || (has('.') && !t.ends_with('.') && t.split('.').all(|p| !p.is_empty()))
        || inner_upper
        || acronym
}

/// Byte ranges of the protected tokens in `text`.
pub fn protected(text: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = tokens(text)
        .into_iter()
        .filter(|w| is_protected(&w.text))
        .map(|w| w.range)
        .collect();
    // Backticked spans can hold spaces: protect the whole span.
    let mut from = 0;
    while let Some(a) = text[from..].find('`').map(|i| from + i) {
        let Some(b) = text[a + 1..].find('`').map(|i| a + 1 + i) else {
            break;
        };
        ranges.push(a..b + 1);
        from = b + 1;
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// The protected tokens themselves (for checking a rewrite kept every one).
pub fn protected_tokens(text: &str) -> Vec<String> {
    protected(text)
        .into_iter()
        .map(|r| text[r].to_owned())
        .collect()
}

pub fn overlaps(ranges: &[Range<usize>], r: &Range<usize>) -> bool {
    ranges.iter().any(|p| p.start < r.end && r.start < p.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_numbers_code_and_acronyms_are_protected() {
        for t in [
            "https://aurathex.com/x?y=1",
            "www.example.org",
            "dave@example.com",
            "3.14",
            "10:30",
            "v0.3",
            "$1,299",
            "snake_case",
            "C:\\Users\\me",
            "src/main.rs",
            "getElementById",
            "iPhone",
            "NASA",
            "API",
            "#42",
            "x=1",
            "`npm`",
            "example.com",
        ] {
            assert!(is_protected(t), "{t}");
        }
    }

    #[test]
    fn ordinary_words_are_not_protected() {
        for t in ["hello", "Hello", "don't", "I", "A", "well-known", "O'Brien"] {
            assert!(!is_protected(t), "{t}");
        }
    }

    #[test]
    fn tokens_drop_sentence_punctuation_but_keep_inner_characters() {
        let w = tokens("Visit example.com. (Then) call 555-0100!");
        let t: Vec<&str> = w.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(t, vec!["Visit", "example.com", "Then", "call", "555-0100"]);
        assert_eq!(
            &"Visit example.com. (Then)"[w[1].range.clone()],
            "example.com"
        );
    }

    #[test]
    fn backticked_phrases_are_protected_whole() {
        let text = "run `cargo test --lib` now";
        let p = protected(text);
        assert!(p.iter().any(|r| &text[r.clone()] == "`cargo test --lib`"));
    }
}
