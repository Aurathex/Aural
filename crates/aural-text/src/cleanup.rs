//! Light cleanup: small, predictable fixes that never change what was said.
//!
//! - hesitation sounds ("um", "uh", "erm", "hmm") are removed, with the comma after them;
//! - spaces before punctuation and doubled spaces or commas are fixed;
//! - the first letter is made a capital when the text starts a sentence.
//!
//! Words that carry meaning ("like", "you know", "so") are left alone, and protected
//! tokens (addresses, numbers, code) are never touched.

use crate::protect::{overlaps, protected, tokens};

const FILLERS: &[&str] = &[
    "um", "umm", "uh", "uhh", "uhm", "erm", "er", "hmm", "mm", "ah",
];

fn is_filler(word: &str) -> bool {
    FILLERS.contains(&word.to_lowercase().as_str())
}

pub fn light(text: &str) -> String {
    let keep = protected(text);
    // Remove fillers (with a comma that directly follows them).
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for w in tokens(text) {
        if !is_filler(&w.text) || overlaps(&keep, &w.range) {
            continue;
        }
        let mut end = w.range.end;
        let rest = &text[end..];
        if rest.starts_with(',') {
            end += 1;
        }
        out.push_str(&text[copied..w.range.start]);
        copied = end;
    }
    out.push_str(&text[copied..]);

    // Spacing: collapse runs of spaces, no space before , . ; : ! ?, no doubled commas,
    // no comma or space at the very start.
    let mut tidy = String::with_capacity(out.len());
    for ch in out.chars() {
        let last = tidy.chars().last();
        match ch {
            ' ' if last.is_none_or(|c| c == ' ') => {}
            ',' | '.' | ';' | ':' | '!' | '?' if last == Some(' ') => {
                tidy.pop();
                if !(ch == ',' && tidy.ends_with([',', '.', '!', '?'])) {
                    tidy.push(ch);
                }
            }
            ',' if last == Some(',') || last.is_none() => {}
            _ => tidy.push(ch),
        }
    }
    let tidy = tidy
        .trim()
        .trim_start_matches([',', ' ', '.', ';', ':'])
        .to_owned();

    // A capital at the start, unless the first word is protected (e.g. "iPhone").
    let first_protected = tokens(&tidy)
        .first()
        .is_some_and(|w| crate::protect::is_protected(&w.text));
    let mut chars = tidy.chars();
    match chars.next() {
        Some(c) if c.is_lowercase() && !first_protected => c.to_uppercase().chain(chars).collect(),
        _ => tidy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hesitations_go_meaning_stays() {
        assert_eq!(
            light("Um, so I think, uh, we should like ship it."),
            "So I think, we should like ship it."
        );
    }

    #[test]
    fn spacing_before_punctuation_is_fixed() {
        assert_eq!(light("hello  there , friend ."), "Hello there, friend.");
    }

    #[test]
    fn protected_tokens_are_left_exactly_as_they_were() {
        assert_eq!(
            light("uh iPhone 15 costs $999 at apple.com/uk"),
            "iPhone 15 costs $999 at apple.com/uk"
        );
    }

    #[test]
    fn clean_text_is_unchanged() {
        let t = "Meet me at 10:30, then call Sam.";
        assert_eq!(light(t), t);
    }

    #[test]
    fn only_fillers_leaves_nothing() {
        assert_eq!(light("Um, uh."), "");
    }
}
