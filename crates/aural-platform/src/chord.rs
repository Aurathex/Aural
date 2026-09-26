//! Hotkey chords: parsing user-facing key names, validating against Windows-reserved
//! shortcuts, and matching the physical key stream from the low-level hook. Pure logic;
//! the hook itself lives in `hook.rs`.

use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeSet;

const VK_ESCAPE: u16 = 0x1B;

/// One key of a chord. Plain modifiers match either side; `Right*` match one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeySpec {
    Ctrl,
    Alt,
    Shift,
    Win,
    RightCtrl,
    RightAlt,
    RightShift,
    Vk(u16),
}

impl KeySpec {
    fn matches(self, vk: u16) -> bool {
        match self {
            KeySpec::Ctrl => matches!(vk, 0x11 | 0xA2 | 0xA3),
            KeySpec::Alt => matches!(vk, 0x12 | 0xA4 | 0xA5),
            KeySpec::Shift => matches!(vk, 0x10 | 0xA0 | 0xA1),
            KeySpec::Win => matches!(vk, 0x5B | 0x5C),
            KeySpec::RightCtrl => vk == 0xA3,
            KeySpec::RightAlt => vk == 0xA5,
            KeySpec::RightShift => vk == 0xA1,
            KeySpec::Vk(v) => vk == v,
        }
    }

    fn is_modifier(self) -> bool {
        !matches!(self, KeySpec::Vk(_))
    }

    fn name(self) -> String {
        match self {
            KeySpec::Ctrl => "Ctrl".into(),
            KeySpec::Alt => "Alt".into(),
            KeySpec::Shift => "Shift".into(),
            KeySpec::Win => "Win".into(),
            KeySpec::RightCtrl => "RightCtrl".into(),
            KeySpec::RightAlt => "RightAlt".into(),
            KeySpec::RightShift => "RightShift".into(),
            KeySpec::Vk(v) => NAMED
                .iter()
                .find(|(_, code)| *code == v)
                .map(|(n, _)| n.to_string())
                .or_else(|| match v {
                    0x41..=0x5A | 0x30..=0x39 => Some(char::from(v as u8).to_string()),
                    0x70..=0x87 => Some(format!("F{}", v - 0x6F)),
                    _ => None,
                })
                .unwrap_or_else(|| format!("0x{v:02X}")),
        }
    }

    fn display_name(self) -> String {
        match self {
            KeySpec::RightCtrl => "Right Ctrl".into(),
            KeySpec::RightAlt => "Right Alt".into(),
            KeySpec::RightShift => "Right Shift".into(),
            other => other.name(),
        }
    }
}

const NAMED: &[(&str, u16)] = &[
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Delete", 0x2E),
    ("Insert", 0x2D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Pause", 0x13),
    ("ScrollLock", 0x91),
];

fn parse_key(name: &str) -> Option<KeySpec> {
    let lower = name.trim().to_ascii_lowercase();
    let spec = match lower.as_str() {
        "ctrl" | "control" => KeySpec::Ctrl,
        "alt" => KeySpec::Alt,
        "shift" => KeySpec::Shift,
        "win" | "windows" | "meta" => KeySpec::Win,
        "rightctrl" => KeySpec::RightCtrl,
        "rightalt" | "altgr" => KeySpec::RightAlt,
        "rightshift" => KeySpec::RightShift,
        _ => {
            if let Some((_, vk)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(&lower)) {
                KeySpec::Vk(*vk)
            } else if lower.len() == 1 && lower.chars().all(|c| c.is_ascii_alphanumeric()) {
                KeySpec::Vk(lower.to_ascii_uppercase().as_bytes()[0] as u16)
            } else {
                let n = lower
                    .strip_prefix('f')
                    .and_then(|n| n.parse::<u16>().ok())?;
                if (1..=24).contains(&n) {
                    KeySpec::Vk(0x6F + n)
                } else {
                    return None;
                }
            }
        }
    };
    Some(spec)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    keys: BTreeSet<KeySpec>,
}

impl Chord {
    pub fn parse(names: &[String]) -> Result<Self> {
        if names.is_empty() {
            bail!("a hotkey needs at least one key");
        }
        let mut keys = BTreeSet::new();
        for n in names {
            match parse_key(n) {
                Some(k) => {
                    keys.insert(k);
                }
                None => bail!("unknown key {n:?}"),
            }
        }
        Ok(Self { keys })
    }

    /// Stable key names for settings.json.
    pub fn names(&self) -> Vec<String> {
        self.keys.iter().map(|k| k.name()).collect()
    }

    pub fn display(&self) -> String {
        self.keys
            .iter()
            .map(|k| k.display_name())
            .collect::<Vec<_>>()
            .join(" + ")
    }

    fn contains(&self, vk: u16) -> bool {
        self.keys.iter().any(|k| k.matches(vk))
    }

    fn is_trigger_key(&self, vk: u16) -> bool {
        self.keys.iter().any(|k| !k.is_modifier() && k.matches(vk))
    }

    /// Releasing Win or Alt alone makes Windows open Start or focus the menu bar;
    /// an unassigned "mask" key press prevents that.
    pub fn needs_mask_key(&self) -> bool {
        self.keys
            .iter()
            .any(|k| matches!(k, KeySpec::Win | KeySpec::Alt | KeySpec::RightAlt))
    }

    fn is(&self, names: &[&str]) -> bool {
        let other: BTreeSet<KeySpec> = names.iter().filter_map(|n| parse_key(n)).collect();
        self.keys == other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", content = "message", rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    Warn(String),
    Reject(String),
}

pub fn validate(chord: &Chord) -> Verdict {
    const RESERVED: &[(&[&str], &str)] = &[
        (&["Win", "L"], "Win + L locks Windows"),
        (&["Win", "H"], "Win + H is Windows voice typing"),
        (&["Win", "G"], "Win + G opens the Xbox Game Bar"),
        (&["Win", "D"], "Win + D shows the desktop"),
        (
            &["Ctrl", "Alt", "Delete"],
            "Ctrl + Alt + Delete is reserved by Windows",
        ),
        (
            &["Ctrl", "Win", "Enter"],
            "Ctrl + Win + Enter starts Narrator",
        ),
        (
            &["Ctrl", "Win", "O"],
            "Ctrl + Win + O opens the on-screen keyboard",
        ),
    ];
    const COMMON: &[&[&str]] = &[
        &["Ctrl", "C"],
        &["Ctrl", "V"],
        &["Ctrl", "X"],
        &["Ctrl", "Z"],
        &["Ctrl", "A"],
        &["Ctrl", "S"],
        &["Alt", "Tab"],
        &["Alt", "F4"],
    ];
    for (keys, why) in RESERVED {
        if chord.is(keys) {
            return Verdict::Reject(format!("{why}."));
        }
    }
    let modifiers: Vec<KeySpec> = chord
        .keys
        .iter()
        .copied()
        .filter(|k| k.is_modifier())
        .collect();
    let plain = chord.keys.iter().filter(|k| !k.is_modifier()).count();
    if plain == 0
        && modifiers.len() == 1
        && matches!(
            modifiers[0],
            KeySpec::Ctrl | KeySpec::Alt | KeySpec::Shift | KeySpec::Win
        )
    {
        return Verdict::Reject(
            "A single Ctrl, Alt, Shift or Win key is used by too many shortcuts. Try Ctrl + Win or Right Ctrl.".into(),
        );
    }
    let typing = chord.keys.iter().any(|k| match k {
        KeySpec::Vk(v) => matches!(v, 0x41..=0x5A | 0x30..=0x39 | 0x20 | 0x0D | 0x09),
        _ => false,
    });
    let only_shift = modifiers
        .iter()
        .all(|k| matches!(k, KeySpec::Shift | KeySpec::RightShift));
    if typing && only_shift {
        return Verdict::Reject("That key is used for typing. Add Ctrl, Alt or Win.".into());
    }
    if COMMON.iter().any(|keys| chord.is(keys)) {
        return Verdict::Warn(format!(
            "{} is a common shortcut in other apps; they will stop receiving it.",
            chord.display()
        ));
    }
    Verdict::Ok
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchOutput {
    pub event: Option<HotkeyEvent>,
    /// Hide this key event from other apps.
    pub swallow: bool,
}

/// Tracks physical key state and turns it into hotkey Down/Up/Cancel.
#[derive(Debug, Clone)]
pub struct ChordMatcher {
    chord: Chord,
    pressed: BTreeSet<u16>,
    active: bool,
    /// Set after a cancel until every chord key is released.
    latched: bool,
    swallowed: BTreeSet<u16>,
}

impl ChordMatcher {
    pub fn new(chord: Chord) -> Self {
        Self {
            chord,
            pressed: BTreeSet::new(),
            active: false,
            latched: false,
            swallowed: BTreeSet::new(),
        }
    }

    pub fn chord(&self) -> &Chord {
        &self.chord
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn all_held(&self) -> bool {
        self.chord
            .keys
            .iter()
            .all(|k| self.pressed.iter().any(|&vk| k.matches(vk)))
    }

    fn only_chord_keys_held(&self) -> bool {
        self.pressed.iter().all(|&vk| self.chord.contains(vk))
    }

    pub fn on_key(&mut self, vk: u16, down: bool, injected: bool) -> MatchOutput {
        self.on_key_checked(vk, down, injected, &|_| true)
    }

    /// Like `on_key`, but first forgets keys Windows says are no longer held.
    /// `physically_down(vk)` reports a key's state *before* this event (as
    /// GetAsyncKeyState does inside a low-level hook). Key-ups that happen on the lock
    /// screen or the Ctrl+Alt+Del screen never reach the hook, so without this a key
    /// can stay "held" forever.
    pub fn on_key_checked(
        &mut self,
        vk: u16,
        down: bool,
        injected: bool,
        physically_down: &dyn Fn(u16) -> bool,
    ) -> MatchOutput {
        if injected {
            return MatchOutput::default();
        }
        if down {
            // A key we think is held but that was up before this event is a real new
            // press (its key-up was lost), not auto-repeat.
            if self.pressed.contains(&vk) && !physically_down(vk) {
                self.pressed.remove(&vk);
                self.swallowed.remove(&vk);
            }
            let before = self.pressed.len();
            self.pressed.retain(|&k| k == vk || physically_down(k));
            if self.pressed.len() != before {
                self.swallowed.retain(|k| self.pressed.contains(k));
                if self.latched && !self.pressed.iter().any(|&k| self.chord.contains(k)) {
                    self.latched = false;
                }
            }
            let repeat = !self.pressed.insert(vk);
            if repeat {
                return MatchOutput {
                    event: None,
                    swallow: self.swallowed.contains(&vk),
                };
            }
            if self.active {
                if vk == VK_ESCAPE || !self.chord.contains(vk) {
                    self.active = false;
                    self.latched = true;
                    let swallow = vk == VK_ESCAPE;
                    if swallow {
                        self.swallowed.insert(vk);
                    }
                    return MatchOutput {
                        event: Some(HotkeyEvent::Cancel),
                        swallow,
                    };
                }
                return MatchOutput::default();
            }
            if !self.latched && self.all_held() && self.only_chord_keys_held() {
                self.active = true;
                let swallow = self.chord.is_trigger_key(vk);
                if swallow {
                    self.swallowed.insert(vk);
                }
                return MatchOutput {
                    event: Some(HotkeyEvent::Down),
                    swallow,
                };
            }
            MatchOutput::default()
        } else {
            self.pressed.remove(&vk);
            let swallow = self.swallowed.remove(&vk);
            let mut event = None;
            if self.active && self.chord.contains(vk) {
                self.active = false;
                event = Some(HotkeyEvent::Up);
            }
            if self.latched && !self.pressed.iter().any(|&k| self.chord.contains(k)) {
                self.latched = false;
            }
            MatchOutput { event, swallow }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u16 = 0xA2;
    const RCTRL: u16 = 0xA3;
    const LWIN: u16 = 0x5B;
    const RIGHT: u16 = 0x27;
    const ESC: u16 = 0x1B;
    const F9: u16 = 0x78;
    const SPACE: u16 = 0x20;
    const LSHIFT: u16 = 0xA0;

    fn chord(keys: &[&str]) -> Chord {
        Chord::parse(&keys.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    fn feed(m: &mut ChordMatcher, events: &[(u16, bool)]) -> Vec<HotkeyEvent> {
        events
            .iter()
            .filter_map(|&(vk, down)| m.on_key(vk, down, false).event)
            .collect()
    }

    #[test]
    fn parses_names_case_insensitively_and_rejects_unknown() {
        assert!(Chord::parse(&["ctrl".into(), "WIN".into()]).is_ok());
        assert!(Chord::parse(&["Ctrl".into(), "Hyper".into()]).is_err());
        assert!(Chord::parse(&[]).is_err());
    }

    #[test]
    fn display_uses_canonical_names() {
        assert_eq!(chord(&["win", "ctrl"]).display(), "Ctrl + Win");
        assert_eq!(chord(&["F9"]).display(), "F9");
    }

    #[test]
    fn ctrl_win_down_emits_down_once_and_release_emits_up() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        let ev = feed(
            &mut m,
            &[
                (LCTRL, true),
                (LCTRL, true),
                (LWIN, true),
                (LWIN, true),
                (LWIN, false),
            ],
        );
        assert_eq!(ev, vec![HotkeyEvent::Down, HotkeyEvent::Up]);
    }

    #[test]
    fn either_ctrl_side_matches_generic_ctrl() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        assert_eq!(
            feed(&mut m, &[(RCTRL, true), (LWIN, true)]),
            vec![HotkeyEvent::Down]
        );
    }

    #[test]
    fn right_ctrl_alone_does_not_match_left_ctrl() {
        let mut m = ChordMatcher::new(chord(&["RightCtrl"]));
        assert!(feed(&mut m, &[(LCTRL, true), (LCTRL, false)]).is_empty());
        assert_eq!(
            feed(&mut m, &[(RCTRL, true), (RCTRL, false)]),
            vec![HotkeyEvent::Down, HotkeyEvent::Up]
        );
    }

    #[test]
    fn extra_key_cancels_and_passes_through() {
        // Ctrl+Win+Right is Windows' "next desktop": dictation must get out of the way.
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        feed(&mut m, &[(LCTRL, true), (LWIN, true)]);
        let out = m.on_key(RIGHT, true, false);
        assert_eq!(out.event, Some(HotkeyEvent::Cancel));
        assert!(!out.swallow);
        // Releasing the chord afterwards must not emit Up for a cancelled press.
        assert!(feed(&mut m, &[(RIGHT, false), (LWIN, false), (LCTRL, false)]).is_empty());
    }

    #[test]
    fn chord_does_not_fire_if_another_key_was_already_held() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        assert!(feed(&mut m, &[(LSHIFT, true), (LCTRL, true), (LWIN, true)]).is_empty());
    }

    /// Key-ups that happen on the lock screen never reach a low-level hook: after
    /// Win + L the matcher still thinks Win is held.
    #[test]
    fn stuck_win_from_the_lock_screen_does_not_turn_ctrl_into_the_hotkey() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        m.on_key(LWIN, true, false); // Win + L: Win's key-up is never seen
        let physically_down = |vk: u16| vk == LCTRL; // only Ctrl is really held now
        let out = m.on_key_checked(LCTRL, true, false, &physically_down);
        assert_eq!(out.event, None, "plain Ctrl must not start dictation");
    }

    #[test]
    fn stale_keys_from_ctrl_alt_del_do_not_block_the_hotkey() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        let lalt = 0xA4;
        let del = 0x2E;
        for vk in [LCTRL, lalt, del] {
            m.on_key(vk, true, false); // secure desktop swallows the key-ups
        }
        m.on_key(LCTRL, false, false);
        let down = |vk: u16| vk == LCTRL || vk == LWIN;
        m.on_key_checked(LCTRL, true, false, &down);
        let out = m.on_key_checked(LWIN, true, false, &down);
        assert_eq!(out.event, Some(HotkeyEvent::Down));
    }

    #[test]
    fn a_re_pressed_stuck_key_is_evaluated_again() {
        let mut m = ChordMatcher::new(chord(&["F9"]));
        m.on_key(F9, true, false); // key-up lost
        let up_now = |_: u16| false;
        m.on_key_checked(LSHIFT, true, false, &up_now); // any key prunes stale F9
        m.on_key_checked(LSHIFT, false, false, &up_now);
        let out = m.on_key_checked(F9, true, false, &|vk| vk == F9);
        assert_eq!(out.event, Some(HotkeyEvent::Down));
    }

    #[test]
    fn escape_while_active_cancels_and_is_swallowed() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        feed(&mut m, &[(LCTRL, true), (LWIN, true)]);
        let out = m.on_key(ESC, true, false);
        assert_eq!(out.event, Some(HotkeyEvent::Cancel));
        assert!(out.swallow);
    }

    #[test]
    fn non_modifier_trigger_key_is_swallowed_while_matched() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Space"]));
        assert!(!m.on_key(LCTRL, true, false).swallow);
        let down = m.on_key(SPACE, true, false);
        assert_eq!(down.event, Some(HotkeyEvent::Down));
        assert!(down.swallow);
        let up = m.on_key(SPACE, false, false);
        assert_eq!(up.event, Some(HotkeyEvent::Up));
        assert!(up.swallow);
    }

    #[test]
    fn injected_events_are_ignored() {
        let mut m = ChordMatcher::new(chord(&["F9"]));
        assert_eq!(m.on_key(F9, true, true).event, None);
        assert!(!m.on_key(F9, true, true).swallow);
    }

    #[test]
    fn modifier_only_chords_are_never_swallowed() {
        let mut m = ChordMatcher::new(chord(&["Ctrl", "Win"]));
        assert!(!m.on_key(LCTRL, true, false).swallow);
        assert!(!m.on_key(LWIN, true, false).swallow);
        assert!(!m.on_key(LWIN, false, false).swallow);
    }

    #[test]
    fn validation_rejects_reserved_and_typing_keys() {
        for bad in [
            vec!["Win", "L"],
            vec!["Win", "H"],
            vec!["Win", "G"],
            vec!["Ctrl", "Alt", "Delete"],
            vec!["Win"],
            vec!["A"],
            vec!["Space"],
            vec!["Shift", "A"],
            vec!["Ctrl", "Win", "Enter"],
        ] {
            assert!(
                matches!(validate(&chord(&bad)), Verdict::Reject(_)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn validation_warns_on_common_app_shortcuts() {
        for w in [
            vec!["Ctrl", "C"],
            vec!["Ctrl", "V"],
            vec!["Alt", "Tab"],
            vec!["Alt", "F4"],
        ] {
            assert!(matches!(validate(&chord(&w)), Verdict::Warn(_)), "{w:?}");
        }
    }

    #[test]
    fn validation_accepts_good_chords() {
        for ok in [
            vec!["Ctrl", "Win"],
            vec!["RightCtrl"],
            vec!["F9"],
            vec!["Ctrl", "Shift", "Space"],
        ] {
            assert_eq!(validate(&chord(&ok)), Verdict::Ok, "{ok:?}");
        }
    }

    #[test]
    fn mask_needed_when_win_or_alt_released() {
        assert!(chord(&["Ctrl", "Win"]).needs_mask_key());
        assert!(chord(&["Alt", "Space"]).needs_mask_key());
        assert!(!chord(&["RightCtrl"]).needs_mask_key());
    }
}
