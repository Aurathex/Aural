//! Local, searchable history of dictated text. Text only — audio is never stored. One
//! JSON line per dictation in `history.jsonl` under Aural's data folder; nothing is
//! sent anywhere. Entries older than the chosen number of days are removed, and single
//! entries or all of them can be deleted.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    /// Unix time, seconds.
    pub at: u64,
    /// What was typed.
    pub text: String,
    /// What the speech model wrote, when cleanup or the dictionary changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// Your own correction, made in History (what learning compares against).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrected: Option<String>,
    /// Program file of the app it went to ("" when unknown).
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub audio_ms: u64,
    #[serde(default)]
    pub words: u32,
}

pub struct History {
    path: PathBuf,
    entries: Vec<Entry>,
    next_id: u64,
}

pub fn word_count(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}

impl History {
    /// Load `path`; unreadable lines are skipped rather than losing the whole file.
    pub fn load(path: &Path) -> Result<Self> {
        let entries: Vec<Entry> = match std::fs::read_to_string(path) {
            Ok(t) => t
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let next_id = entries.iter().map(|e| e.id).max().map_or(1, |m| m + 1);
        Ok(Self {
            path: path.to_owned(),
            entries,
            next_id,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Append one dictation; returns its id.
    pub fn add(&mut self, mut e: Entry) -> Result<u64> {
        e.id = self.next_id;
        self.next_id += 1;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        writeln!(f, "{}", serde_json::to_string(&e)?)?;
        let id = e.id;
        self.entries.push(e);
        Ok(id)
    }

    fn rewrite(&self) -> Result<()> {
        let tmp = self.path.with_extension("jsonl.tmp");
        let mut body = String::new();
        for e in &self.entries {
            body.push_str(&serde_json::to_string(e)?);
            body.push('\n');
        }
        std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        Ok(())
    }

    /// Newest first; every word of `query` must appear (in any case) in the text, the
    /// correction or the app name. An empty query lists everything.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Entry> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                let hay = format!(
                    "{} {} {}",
                    e.text,
                    e.corrected.as_deref().unwrap_or(""),
                    e.app
                )
                .to_lowercase();
                terms.iter().all(|t| hay.contains(t.as_str()))
            })
            .take(limit)
            .collect()
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Store your correction of an entry; returns the text it corrected.
    pub fn correct(&mut self, id: u64, corrected: &str) -> Result<Option<String>> {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Ok(None);
        };
        let before = e.corrected.clone().unwrap_or_else(|| e.text.clone());
        e.corrected = Some(corrected.trim().to_owned());
        self.rewrite()?;
        Ok(Some(before))
    }

    pub fn delete(&mut self, id: u64) -> Result<bool> {
        let n = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == n {
            return Ok(false);
        }
        self.rewrite()?;
        Ok(true)
    }

    /// Delete everything, including the file.
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Remove entries older than `keep_days` (0 keeps everything); returns how many.
    pub fn prune(&mut self, keep_days: u32, now: u64) -> Result<usize> {
        if keep_days == 0 {
            return Ok(0);
        }
        let cutoff = now.saturating_sub(u64::from(keep_days) * 86_400);
        let n = self.entries.len();
        self.entries.retain(|e| e.at >= cutoff);
        let removed = n - self.entries.len();
        if removed > 0 {
            self.rewrite()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: u64, text: &str, app: &str) -> Entry {
        Entry {
            id: 0,
            at,
            text: text.into(),
            raw: None,
            corrected: None,
            app: app.into(),
            variant: "parakeet@cpu".into(),
            audio_ms: 2_000,
            words: word_count(text),
        }
    }

    #[test]
    fn entries_survive_a_restart_with_increasing_ids() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("history.jsonl");
        let mut h = History::load(&p).unwrap();
        assert_eq!(h.add(entry(10, "first note", "notepad.exe")).unwrap(), 1);
        assert_eq!(h.add(entry(20, "second note", "slack.exe")).unwrap(), 2);
        let mut h = History::load(&p).unwrap();
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.add(entry(30, "third", "")).unwrap(), 3);
    }

    #[test]
    fn search_is_newest_first_all_words_any_case() {
        let d = tempfile::tempdir().unwrap();
        let mut h = History::load(&d.path().join("h.jsonl")).unwrap();
        h.add(entry(1, "Invoice for Acme sent", "outlook.exe"))
            .unwrap();
        h.add(entry(2, "acme meeting moved", "slack.exe")).unwrap();
        h.add(entry(3, "lunch", "slack.exe")).unwrap();
        let hits: Vec<u64> = h.search("ACME", 10).iter().map(|e| e.at).collect();
        assert_eq!(hits, vec![2, 1]);
        assert_eq!(h.search("acme slack", 10).len(), 1);
        assert_eq!(h.search("", 2).len(), 2);
        assert!(h.search("nothing like this", 10).is_empty());
    }

    #[test]
    fn old_entries_are_removed_by_the_retention_setting() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("h.jsonl");
        let mut h = History::load(&p).unwrap();
        let day = 86_400;
        h.add(entry(0, "ancient", "")).unwrap();
        h.add(entry(40 * day, "recent", "")).unwrap();
        assert_eq!(h.prune(0, 41 * day).unwrap(), 0, "0 keeps everything");
        assert_eq!(h.prune(30, 41 * day).unwrap(), 1);
        let h = History::load(&p).unwrap();
        assert_eq!(h.entries().len(), 1);
        assert_eq!(h.entries()[0].text, "recent");
    }

    #[test]
    fn delete_one_and_delete_all() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("h.jsonl");
        let mut h = History::load(&p).unwrap();
        let a = h.add(entry(1, "keep", "")).unwrap();
        let b = h.add(entry(2, "remove", "")).unwrap();
        assert!(h.delete(b).unwrap());
        assert!(!h.delete(b).unwrap());
        assert_eq!(History::load(&p).unwrap().entries().len(), 1);
        assert_eq!(h.get(a).unwrap().text, "keep");
        h.clear().unwrap();
        assert!(!p.exists());
        assert!(History::load(&p).unwrap().entries().is_empty());
    }

    #[test]
    fn a_correction_is_kept_and_searchable() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("h.jsonl");
        let mut h = History::load(&p).unwrap();
        let id = h.add(entry(1, "email or a thex", "")).unwrap();
        assert_eq!(
            h.correct(id, "email Aurathex ").unwrap().as_deref(),
            Some("email or a thex")
        );
        let h = History::load(&p).unwrap();
        assert_eq!(
            h.get(id).unwrap().corrected.as_deref(),
            Some("email Aurathex")
        );
        assert_eq!(h.search("aurathex", 5).len(), 1);
    }

    #[test]
    fn a_damaged_line_does_not_lose_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("h.jsonl");
        let good = serde_json::to_string(&Entry {
            id: 4,
            ..entry(1, "fine", "")
        })
        .unwrap();
        std::fs::write(&p, format!("{good}\n{{not json\n")).unwrap();
        let h = History::load(&p).unwrap();
        assert_eq!(h.entries().len(), 1);
    }
}
