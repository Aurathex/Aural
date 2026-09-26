//! Word error rate scoring.
//!
//! Normalization lowercases, keeps letters, digits and apostrophes, and splits on
//! everything else. It does not normalize numbers ("ten" vs "10"), so every engine
//! is scored under the same rule.

pub fn normalize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WerStats {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    pub reference_words: usize,
}

impl WerStats {
    pub fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }

    pub fn wer(&self) -> f64 {
        match (self.reference_words, self.errors()) {
            (0, 0) => 0.0,
            (0, _) => 1.0,
            (r, e) => e as f64 / r as f64,
        }
    }
}

/// Levenshtein alignment over normalized words; each cell carries
/// (cost, substitutions, deletions, insertions) so the error kinds fall out of the
/// cheapest path.
pub fn word_errors(reference: &str, hypothesis: &str) -> WerStats {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    type Cell = (usize, usize, usize, usize);
    let mut prev: Vec<Cell> = (0..=h.len()).map(|j| (j, 0, 0, j)).collect();
    for i in 1..=r.len() {
        let mut cur: Vec<Cell> = Vec::with_capacity(h.len() + 1);
        cur.push((i, 0, i, 0));
        for j in 1..=h.len() {
            let (dc, ds, dd, di) = prev[j - 1];
            let diag = if r[i - 1] == h[j - 1] {
                (dc, ds, dd, di)
            } else {
                (dc + 1, ds + 1, dd, di)
            };
            let (uc, us, ud, ui) = prev[j];
            let up = (uc + 1, us, ud + 1, ui);
            let (lc, ls, ld, li) = cur[j - 1];
            let left = (lc + 1, ls, ld, li + 1);
            let best = [diag, up, left]
                .into_iter()
                .min_by_key(|c| c.0)
                .unwrap_or(diag);
            cur.push(best);
        }
        prev = cur;
    }
    let (_, substitutions, deletions, insertions) = prev[h.len()];
    WerStats {
        substitutions,
        deletions,
        insertions,
        reference_words: r.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_zero() {
        let s = word_errors("hello world", "hello world");
        assert_eq!(s.wer(), 0.0);
    }

    #[test]
    fn case_and_punctuation_ignored() {
        let s = word_errors("Hello, world.", "hello world");
        assert_eq!(s.wer(), 0.0);
    }

    #[test]
    fn apostrophes_kept() {
        assert_eq!(normalize("Don't STOP!"), vec!["don't", "stop"]);
    }

    #[test]
    fn counts_substitution_deletion_insertion() {
        // ref: a b c d ; hyp: a x c d e  -> 1 sub (b->x), 1 ins (e)
        let s = word_errors("a b c d", "a x c d e");
        assert_eq!((s.substitutions, s.deletions, s.insertions), (1, 0, 1));
        assert!((s.wer() - 0.5).abs() < 1e-9);
        let d = word_errors("a b c", "a c");
        assert_eq!((d.substitutions, d.deletions, d.insertions), (0, 1, 0));
    }

    #[test]
    fn empty_reference_counts_insertions_without_div_by_zero() {
        let s = word_errors("", "noise");
        assert_eq!(s.insertions, 1);
        assert_eq!(s.wer(), 1.0);
    }
}
