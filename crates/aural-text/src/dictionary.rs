//! The personal dictionary: words and names written the way you want them.
//!
//! An entry is the spelling to write (`write`), plus optional phrases the speech model
//! tends to hear instead (`heard`). A spelling alone fixes capitalization and spelling
//! of that exact word ("aurathex" becomes "Aurathex"); `heard` phrases are replaced by
//! it ("or a thex" becomes "Aurathex"). Matching is whole words, ignores case, and
//! never touches protected tokens (addresses, numbers, code).

use crate::protect::{overlaps, protected};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Entry {
    pub write: String,
    pub heard: Vec<String>,
}

/// A run of letters, digits and apostrophes, with its byte range.
fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let part = ch.is_alphanumeric() || ch == '\'' || ch == '’';
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn norm(s: &str) -> Vec<String> {
    let text = s.to_lowercase();
    words(&text)
        .into_iter()
        .map(|(a, b)| text[a..b].replace('’', "'"))
        .collect()
}

/// Apply the dictionary to `text`. Longer phrases win over shorter ones.
pub fn apply(entries: &[Entry], text: &str) -> String {
    let mut patterns: Vec<(Vec<String>, &str)> = entries
        .iter()
        .filter(|e| !e.write.trim().is_empty())
        .flat_map(|e| {
            e.heard
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(e.write.as_str()))
                .map(move |h| (norm(h), e.write.trim()))
        })
        .filter(|(p, _)| !p.is_empty())
        .collect();
    patterns.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    if patterns.is_empty() {
        return text.to_owned();
    }
    let keep = protected(text);
    let spans = words(text);
    let lower: Vec<String> = spans
        .iter()
        .map(|&(a, b)| text[a..b].to_lowercase().replace('’', "'"))
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < spans.len() {
        let hit = patterns.iter().find(|(p, _)| {
            i + p.len() <= spans.len()
                && lower[i..i + p.len()] == p[..]
                && !overlaps(&keep, &(spans[i].0..spans[i + p.len() - 1].1))
        });
        match hit {
            Some((p, write)) => {
                let (a, b) = (spans[i].0, spans[i + p.len() - 1].1);
                out.push_str(&text[copied..a]);
                out.push_str(write);
                copied = b;
                i += p.len();
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(write: &str, heard: &[&str]) -> Entry {
        Entry {
            write: write.into(),
            heard: heard.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_spelling_fixes_that_word_everywhere() {
        let d = [e("Aurathex", &[])];
        assert_eq!(
            apply(&d, "aurathex builds sites. Sites by aurathex!"),
            "Aurathex builds sites. Sites by Aurathex!"
        );
        // Written in capitals it reads as an acronym, which is left as dictated.
        assert_eq!(apply(&d, "AURATHEX"), "AURATHEX");
    }

    #[test]
    fn heard_phrases_become_the_spelling_keeping_punctuation() {
        let d = [e("Aurathex", &["or a thex", "aura thex"])];
        assert_eq!(
            apply(&d, "Email Or a thex, today."),
            "Email Aurathex, today."
        );
    }

    #[test]
    fn only_whole_words_match() {
        let d = [e("Kay", &["k"])];
        assert_eq!(apply(&d, "keep k okay"), "keep Kay okay");
    }

    #[test]
    fn longer_phrases_win() {
        let d = [e("New York", &["new york"]), e("NYC", &["new york city"])];
        assert_eq!(apply(&d, "in new york city now"), "in NYC now");
    }

    #[test]
    fn addresses_and_numbers_are_never_changed() {
        let d = [e("Aurathex", &[]), e("two", &["2"])];
        assert_eq!(
            apply(&d, "see aurathex.com and room 2 with aurathex"),
            "see aurathex.com and room 2 with Aurathex"
        );
    }

    #[test]
    fn an_empty_dictionary_changes_nothing() {
        assert_eq!(apply(&[], "Hello  there ."), "Hello  there .");
        assert_eq!(apply(&[e("  ", &["x"])], "x"), "x");
    }
}
