//! Dictation statistics: counts only, no text. Kept in `stats.json` on this PC and
//! never sent anywhere; resettable.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub dictations: u64,
    pub words: u64,
    pub audio_ms: u64,
}

impl Totals {
    fn add(&mut self, words: u64, audio_ms: u64) {
        self.dictations += 1;
        self.words += words;
        self.audio_ms += audio_ms;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    /// Per day (days since 1970-01-01, UTC).
    pub days: BTreeMap<u64, Totals>,
    /// Dictations per model variant ("<model>@<backend>").
    pub variants: BTreeMap<String, u64>,
    /// Dictations per app program file.
    pub apps: BTreeMap<String, u64>,
    /// Dictations that showed live text.
    pub live: u64,
    /// Dictations that cleanup changed.
    pub cleaned: u64,
}

pub struct Record<'a> {
    pub at: u64,
    pub words: u64,
    pub audio_ms: u64,
    pub variant: &'a str,
    pub app: &'a str,
    pub live: bool,
    pub cleaned: bool,
}

/// A summary for the statistics page.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Summary {
    pub all_time: Totals,
    pub last_7_days: Totals,
    pub today: Totals,
    /// Words per minute of speaking, over all time.
    pub words_per_minute: Option<f64>,
    /// The last 14 days, oldest first: (day, totals).
    pub recent: Vec<(u64, Totals)>,
    pub variants: Vec<(String, u64)>,
    pub apps: Vec<(String, u64)>,
    pub live: u64,
    pub cleaned: u64,
}

impl Stats {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(t) => Ok(serde_json::from_str(&t).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record(&mut self, r: &Record) {
        self.days
            .entry(r.at / 86_400)
            .or_default()
            .add(r.words, r.audio_ms);
        if !r.variant.is_empty() {
            *self.variants.entry(r.variant.to_owned()).or_default() += 1;
        }
        let app = if r.app.is_empty() { "unknown" } else { r.app };
        *self.apps.entry(app.to_lowercase()).or_default() += 1;
        self.live += u64::from(r.live);
        self.cleaned += u64::from(r.cleaned);
    }

    pub fn summary(&self, now: u64) -> Summary {
        let today = now / 86_400;
        let mut s = Summary::default();
        for (&day, t) in &self.days {
            let add = |x: &mut Totals| {
                x.dictations += t.dictations;
                x.words += t.words;
                x.audio_ms += t.audio_ms;
            };
            add(&mut s.all_time);
            if day + 7 > today {
                add(&mut s.last_7_days);
            }
            if day == today {
                add(&mut s.today);
            }
        }
        s.words_per_minute = (s.all_time.audio_ms >= 1_000)
            .then(|| s.all_time.words as f64 * 60_000.0 / s.all_time.audio_ms as f64);
        s.recent = (today.saturating_sub(13)..=today)
            .map(|d| (d, self.days.get(&d).copied().unwrap_or_default()))
            .collect();
        let sorted = |m: &BTreeMap<String, u64>| {
            let mut v: Vec<(String, u64)> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v
        };
        s.variants = sorted(&self.variants);
        s.apps = sorted(&self.apps);
        s.live = self.live;
        s.cleaned = self.cleaned;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn rec(at: u64, words: u64, variant: &'static str, app: &'static str) -> Record<'static> {
        Record {
            at,
            words,
            audio_ms: words * 400,
            variant,
            app,
            live: true,
            cleaned: false,
        }
    }

    #[test]
    fn counts_add_up_by_day_model_and_app() {
        let mut s = Stats::default();
        let now = 100 * DAY + 50;
        s.record(&rec(now, 10, "parakeet@cpu", "Slack.exe"));
        s.record(&rec(now - 3 * DAY, 20, "parakeet@cpu", "notepad.exe"));
        s.record(&rec(now - 30 * DAY, 5, "whisper@vulkan", ""));
        let m = s.summary(now);
        assert_eq!(m.all_time.dictations, 3);
        assert_eq!(m.all_time.words, 35);
        assert_eq!(m.last_7_days.words, 30);
        assert_eq!(m.today.words, 10);
        assert_eq!(m.variants[0], ("parakeet@cpu".to_string(), 2));
        assert!(m.apps.contains(&("slack.exe".to_string(), 1)));
        assert!(m.apps.contains(&("unknown".to_string(), 1)));
        assert_eq!(m.recent.len(), 14);
        assert_eq!(m.recent.last().unwrap().1.words, 10);
        assert!((m.words_per_minute.unwrap() - 150.0).abs() < 1e-9);
        assert_eq!(m.live, 3);
    }

    #[test]
    fn saved_statistics_hold_no_text() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("stats.json");
        let mut s = Stats::default();
        s.record(&rec(DAY, 3, "parakeet@cpu", "slack.exe"));
        s.save(&p).unwrap();
        assert_eq!(Stats::load(&p).unwrap(), s);
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(!body.contains("hello"));
    }

    #[test]
    fn a_damaged_file_starts_over_instead_of_failing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("stats.json");
        std::fs::write(&p, "{broken").unwrap();
        assert_eq!(Stats::load(&p).unwrap(), Stats::default());
        assert_eq!(Stats::default().summary(0).words_per_minute, None);
    }
}
