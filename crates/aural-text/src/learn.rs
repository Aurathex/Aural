//! Personal learning, from corrections you make yourself.
//!
//! When you fix a dictation in History, Aural compares what it wrote with your fix and
//! notes each small replacement (up to three words, e.g. "or a thex" → "Aurathex").
//! Nothing changes on its own: each noticed replacement is a suggestion you can add to
//! your dictionary or dismiss. Suggestions are stored on this PC and can be reset.

use crate::dictionary::Entry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// What Aural wrote.
    pub heard: String,
    /// What you changed it to.
    pub write: String,
    /// How many of your corrections made this change.
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Learned {
    pub suggestions: Vec<Suggestion>,
    /// Replacements you dismissed; not suggested again.
    pub dismissed: Vec<(String, String)>,
}

/// Longest replacement noticed, in words (longer edits are rewrites, not slips).
const MAX_WORDS: usize = 3;

fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_owned()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Replacements between `original` and `corrected`: runs of changed words bounded by
/// words that stayed the same, up to [`MAX_WORDS`] on each side.
pub fn replacements(original: &str, corrected: &str) -> Vec<(String, String)> {
    let a = words(original);
    let b = words(corrected);
    let (n, m) = (a.len(), b.len());
    // Longest common subsequence on words (case-sensitive: a capital is a correction).
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut from_a, mut from_b) = (0, 0);
    let flush = |i: usize, j: usize, fa: usize, fb: usize, out: &mut Vec<(String, String)>| {
        let (x, y) = (&a[fa..i], &b[fb..j]);
        if !x.is_empty() && !y.is_empty() && x.len() <= MAX_WORDS && y.len() <= MAX_WORDS {
            out.push((x.join(" "), y.join(" ")));
        }
    };
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(i, j, from_a, from_b, &mut out);
            i += 1;
            j += 1;
            from_a = i;
            from_b = j;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    flush(n, m, from_a, from_b, &mut out);
    out
}

impl Learned {
    /// Note a correction you made.
    pub fn observe(&mut self, original: &str, corrected: &str) {
        for (heard, write) in replacements(original, corrected) {
            if self
                .dismissed
                .iter()
                .any(|(h, w)| h.eq_ignore_ascii_case(&heard) && *w == write)
            {
                continue;
            }
            match self
                .suggestions
                .iter_mut()
                .find(|s| s.heard.eq_ignore_ascii_case(&heard) && s.write == write)
            {
                Some(s) => s.count += 1,
                None => self.suggestions.push(Suggestion {
                    heard,
                    write,
                    count: 1,
                }),
            }
        }
        self.suggestions
            .sort_by(|x, y| y.count.cmp(&x.count).then(x.write.cmp(&y.write)));
    }

    /// Turn suggestion `i` into a dictionary entry (merged into `dictionary`).
    pub fn accept(&mut self, i: usize, dictionary: &mut Vec<Entry>) -> bool {
        if i >= self.suggestions.len() {
            return false;
        }
        let s = self.suggestions.remove(i);
        let same_word = s.heard.eq_ignore_ascii_case(&s.write);
        match dictionary.iter_mut().find(|e| e.write == s.write) {
            Some(e) => {
                if !same_word && !e.heard.iter().any(|h| h.eq_ignore_ascii_case(&s.heard)) {
                    e.heard.push(s.heard);
                }
            }
            None => dictionary.push(Entry {
                write: s.write,
                heard: if same_word { vec![] } else { vec![s.heard] },
            }),
        }
        true
    }

    pub fn dismiss(&mut self, i: usize) -> bool {
        if i >= self.suggestions.len() {
            return false;
        }
        let s = self.suggestions.remove(i);
        self.dismissed.push((s.heard, s.write));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_replacements_are_noticed() {
        assert_eq!(
            replacements("email or a thex about it", "email Aurathex about it"),
            vec![("or a thex".to_string(), "Aurathex".to_string())]
        );
        assert_eq!(
            replacements("meet kate at noon", "meet Cate at noon."),
            vec![("kate".to_string(), "Cate".to_string())]
        );
    }

    #[test]
    fn rewrites_and_additions_are_not_learned() {
        assert!(replacements("send it", "please could you send it over to me").is_empty());
        assert!(
            replacements("the quick brown fox jumps", "a slow red dog walks around").is_empty()
        );
        assert!(replacements("same text", "same text").is_empty());
    }

    #[test]
    fn nothing_is_applied_until_you_accept_it() {
        let mut l = Learned::default();
        l.observe("email or a thex", "email Aurathex");
        l.observe("call or a thex", "call Aurathex");
        assert_eq!(l.suggestions.len(), 1);
        assert_eq!(l.suggestions[0].count, 2);
        let mut dict = Vec::new();
        assert!(l.accept(0, &mut dict));
        assert_eq!(
            dict,
            vec![Entry {
                write: "Aurathex".into(),
                heard: vec!["or a thex".into()]
            }]
        );
        assert!(l.suggestions.is_empty());
    }

    #[test]
    fn a_dismissed_suggestion_is_not_suggested_again() {
        let mut l = Learned::default();
        l.observe("the cat", "the Cat");
        assert!(l.dismiss(0));
        l.observe("the cat", "the Cat");
        assert!(l.suggestions.is_empty());
        assert!(!l.dismiss(0));
    }

    #[test]
    fn a_capital_fix_becomes_a_spelling_entry() {
        let mut l = Learned::default();
        l.observe("thanks aurathex", "thanks Aurathex");
        let mut dict = Vec::new();
        l.accept(0, &mut dict);
        assert_eq!(dict[0].heard, Vec::<String>::new());
        assert_eq!(dict[0].write, "Aurathex");
    }
}
