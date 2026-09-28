//! Light cleanup: small, predictable fixes that never change what was said.
//!
//! - hesitation sounds ("um", "uh", "erm", "hmm") are removed, with the comma after them;
//! - spaces before punctuation and doubled spaces or commas are fixed;
//! - the first letter is made a capital when the text starts a sentence.
//!
//! Words that carry meaning ("like", "you know", "so") are left alone, and protected
//! tokens (addresses, numbers, code) are never touched.

use crate::protect::{is_protected, overlaps, protected, tokens};

/// Not "mm" (millimetres) or "er": too often real words.
fn is_filler(word: &str) -> bool {
    crate::rewrite::FILLERS.contains(&word.to_lowercase().as_str())
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
    // Protected spans (code in backticks, addresses) keep their own spacing.
    let keep = protected(&out);
    let mut tidy = String::with_capacity(out.len());
    for (i, ch) in out.char_indices() {
        let last = tidy.chars().last();
        if keep.iter().any(|r| r.contains(&i)) {
            tidy.push(ch);
            continue;
        }
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
    // Stray punctuation left at the start by removed hesitations goes; a full stop that
    // begins a word or number (".NET", ".5") stays.
    let mut tidy = tidy.trim();
    while let Some(c) = tidy.chars().next() {
        let next = tidy[c.len_utf8()..].chars().next();
        let starts_word = c == '.' && next.is_some_and(char::is_alphanumeric);
        if !matches!(c, ',' | ' ' | '.' | ';' | ':') || starts_word {
            break;
        }
        tidy = &tidy[c.len_utf8()..];
    }
    let tidy = tidy.to_owned();

    let tidy = drop_stutters(&tidy);
    let tidy = sentence_capitals(&tidy);
    end_sentence(&tidy)
}

/// Small words said twice by accident ("to the the team"). Words that can be doubled on
/// purpose ("very very", "had had", "that that", "bye bye") are not in the list.
const STUTTERS: &[&str] = &[
    "the", "a", "an", "to", "of", "and", "or", "but", "i", "we", "you", "they", "he", "she", "my",
    "your", "our", "in", "on", "at", "for", "with", "from",
];

fn drop_stutters(text: &str) -> String {
    let words = tokens(text);
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for pair in words.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let between = &text[a.range.end..b.range.start];
        let same = a.text.eq_ignore_ascii_case(&b.text)
            && STUTTERS.contains(&b.text.to_lowercase().as_str());
        // Only a bare space between them: "the, the" or "the. The" is left alone.
        if same && !between.is_empty() && between.chars().all(char::is_whitespace) {
            out.push_str(&text[copied..a.range.end]);
            copied = b.range.end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Words after which a full stop does not end the sentence.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "st", "vs", "etc", "jr", "sr", "inc", "ltd", "no", "approx",
    "dept", "fig",
];

/// A capital at the start of each sentence, unless that word is protected ("iPhone").
fn sentence_capitals(text: &str) -> String {
    let words = tokens(text);
    let mut out = text.to_owned();
    for (i, w) in words.iter().enumerate() {
        let starts_sentence = match i.checked_sub(1).map(|p| &words[p]) {
            None => true,
            Some(prev) => {
                let between = &text[prev.range.end..w.range.start];
                between.trim_end().ends_with(['.', '!', '?'])
                    && !between.contains("..")
                    && !is_protected(&prev.text)
                    && !ABBREVIATIONS.contains(&prev.text.to_lowercase().as_str())
            }
        };
        if (!starts_sentence && !is_pronoun_i(text, w)) || is_protected(&w.text) {
            continue;
        }
        let first = w.text.chars().next().unwrap_or(' ');
        if first.is_lowercase() {
            let upper: String = first.to_uppercase().collect();
            // Same byte length for the letters that reach here; replace in place.
            if upper.len() == first.len_utf8() {
                out.replace_range(w.range.start..w.range.start + first.len_utf8(), &upper);
            }
        }
    }
    out
}

/// "i", "i'm", "i'll", "i've", "i'd" as words, not a variable in code ("i = 0").
fn is_pronoun_i(text: &str, w: &crate::protect::Word) -> bool {
    let lower = w.text.to_lowercase().replace('’', "'");
    let is_i = matches!(lower.as_str(), "i" | "i'm" | "i'll" | "i've" | "i'd");
    let code_next = text[w.range.end..]
        .trim_start()
        .starts_with(['=', '+', '-', '*', '/', '<', '>', '[', '(']);
    let code_before = text[..w.range.start]
        .trim_end()
        .ends_with(['=', '+', '*', '/', '<', '>', '[', '(']);
    is_i && !code_next && !code_before
}

/// A dictation shorter than this is often a name, a search or a one-word answer; it gets
/// no full stop.
const MIN_WORDS_FOR_END: usize = 3;

/// A token that is text to copy exactly (a link, an address, a path, code); nothing is
/// written straight after it.
fn is_literal(token: &str) -> bool {
    let t = token;
    t.contains("://")
        || t.contains(['@', '/', '\\', '`', '=', '#', '_'])
        || (t.contains('.') && t.split('.').any(|p| p.chars().any(char::is_alphabetic)))
}

const QUESTION_WORDS: &[&str] = &[
    "what", "why", "how", "when", "where", "who", "which", "whose", "whom",
];
const AUXILIARIES: &[&str] = &[
    "is",
    "are",
    "am",
    "was",
    "were",
    "can",
    "could",
    "would",
    "should",
    "will",
    "shall",
    "may",
    "might",
    "must",
    "do",
    "does",
    "did",
    "have",
    "has",
    "had",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "can't",
    "couldn't",
    "wouldn't",
    "shouldn't",
    "won't",
    "don't",
    "doesn't",
    "didn't",
    "haven't",
    "hasn't",
];
const SUBJECTS: &[&str] = &[
    "i",
    "you",
    "we",
    "they",
    "he",
    "she",
    "it",
    "there",
    "this",
    "that",
    "these",
    "those",
    "anyone",
    "anybody",
    "someone",
    "somebody",
    "everyone",
    "everybody",
    "y'all",
];

/// Only the clear question forms: "can you …", "is it …", "do we …", "what time is it",
/// "how are you". "When I get home …", "do the dishes" and "what a day" are statements.
fn is_question(sentence: &str) -> bool {
    let w: Vec<String> = tokens(sentence)
        .iter()
        .take(3)
        .map(|t| t.text.to_lowercase().replace('’', "'"))
        .collect();
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let (w0, w1, w2) = (at(0), at(1), at(2));
    (AUXILIARIES.contains(&w0) && SUBJECTS.contains(&w1))
        || (QUESTION_WORDS.contains(&w0)
            && (AUXILIARIES.contains(&w1)
                || (AUXILIARIES.contains(&w2) && !SUBJECTS.contains(&w1))))
}

/// End the last sentence with a full stop or question mark when it has none. Existing
/// end punctuation is kept (an exclamation is never invented), a trailing comma or
/// semicolon becomes the end, and nothing is added after a link, path or code.
fn end_sentence(text: &str) -> String {
    let t = text.trim_end();
    // Look past closing quotes and brackets: `said "call me later."` is already ended.
    let core = t.trim_end_matches(['"', '”', '’', '\'', ')', ']']);
    if core.is_empty() || core.ends_with(['.', '!', '?', '…', ':']) {
        return t.to_owned();
    }
    let body = if core.len() == t.len() {
        t.trim_end_matches([',', ';']).trim_end()
    } else {
        t
    };
    let words = tokens(body);
    let Some(last) = words.last() else {
        return body.to_owned();
    };
    // Also covers text ending in a backticked span: its last token holds the backtick.
    if words.len() < MIN_WORDS_FOR_END || is_literal(&last.text) || body.ends_with('`') {
        return body.to_owned();
    }
    let sentence_start = words
        .windows(2)
        .rev()
        .find(|p| {
            body[p[0].range.end..p[1].range.start]
                .trim_end()
                .ends_with(['.', '!', '?'])
        })
        .map_or(0, |p| p[1].range.start);
    let mark = if is_question(&body[sentence_start..]) {
        '?'
    } else {
        '.'
    };
    format!("{body}{mark}")
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
    fn units_and_protected_spacing_survive() {
        assert_eq!(light("the bolt is 5 mm long"), "The bolt is 5 mm long.");
        assert_eq!(light(".NET is fine"), ".NET is fine.");
        assert_eq!(light(".5 mg twice"), ".5 mg twice.");
        assert_eq!(light("run `a  b` now"), "Run `a  b` now.");
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

    #[test]
    fn a_plain_sentence_gets_a_full_stop() {
        assert_eq!(
            light("um so the meeting is at 3pm"),
            "So the meeting is at 3pm."
        );
        assert_eq!(
            light("please send the report to Maria"),
            "Please send the report to Maria."
        );
    }

    #[test]
    fn questions_get_a_question_mark_only_when_clearly_asked() {
        assert_eq!(
            light("can you send me the report"),
            "Can you send me the report?"
        );
        assert_eq!(
            light("what time is it in Tokyo"),
            "What time is it in Tokyo?"
        );
        assert_eq!(light("how are you doing today"), "How are you doing today?");
        assert_eq!(light("do you want the red one"), "Do you want the red one?");
        // A question word that starts a statement, and commands, stay statements.
        assert_eq!(
            light("when I get home I will call"),
            "When I get home I will call."
        );
        assert_eq!(
            light("do the dishes before dinner"),
            "Do the dishes before dinner."
        );
        assert_eq!(
            light("what a great idea that was"),
            "What a great idea that was."
        );
    }

    #[test]
    fn existing_end_punctuation_is_kept_and_never_doubled() {
        assert_eq!(light("that was amazing!"), "That was amazing!");
        assert_eq!(light("is it done?"), "Is it done?");
        assert_eq!(light("and then it stopped..."), "And then it stopped...");
        assert_eq!(light("we shipped it."), "We shipped it.");
        assert_eq!(
            light("she said \"call me later.\""),
            "She said \"call me later.\""
        );
        // Nothing invents an exclamation.
        assert_eq!(light("wow that is great news"), "Wow that is great news.");
    }

    #[test]
    fn a_trailing_comma_becomes_the_end_of_the_sentence() {
        assert_eq!(light("and then we left, uh,"), "And then we left.");
        assert_eq!(light("send it to the team;"), "Send it to the team.");
    }

    #[test]
    fn several_sentences_start_with_capitals_and_end_once() {
        assert_eq!(
            light("the build failed. um can you check it. it needs to ship today"),
            "The build failed. Can you check it. It needs to ship today."
        );
        // Abbreviations are not sentence ends.
        assert_eq!(
            light("bring fruit e.g. apples and pears for the team"),
            "Bring fruit e.g. apples and pears for the team."
        );
        assert_eq!(
            light("talk to dr. smith about it"),
            "Talk to dr. smith about it."
        );
    }

    #[test]
    fn numbers_addresses_code_and_paths_are_never_changed_or_punctuated_into() {
        assert_eq!(
            light("the total is $1,299.50 for 3 units"),
            "The total is $1,299.50 for 3 units."
        );
        assert_eq!(
            light("the docs are at https://aurathex.com/docs"),
            "The docs are at https://aurathex.com/docs"
        );
        assert_eq!(
            light("email me at dave@example.com"),
            "Email me at dave@example.com"
        );
        assert_eq!(
            light("open the file src/main.rs"),
            "Open the file src/main.rs"
        );
        assert_eq!(
            light("then run `cargo test --lib`"),
            "Then run `cargo test --lib`"
        );
        assert_eq!(
            light("rename it to fetch_user_id"),
            "Rename it to fetch_user_id"
        );
        assert_eq!(
            light("uh version 1.2.3 is out now"),
            "Version 1.2.3 is out now."
        );
    }

    #[test]
    fn technical_terms_keep_their_case() {
        assert_eq!(
            light("the iPhone uses USB-C now"),
            "The iPhone uses USB-C now."
        );
        assert_eq!(
            light("iOS and macOS both updated"),
            "iOS and macOS both updated."
        );
    }

    #[test]
    fn hesitation_letters_inside_real_words_are_left_alone() {
        assert_eq!(
            light("the umbrella is by the door"),
            "The umbrella is by the door."
        );
        assert_eq!(
            light("we rented a U-Haul and a hummer"),
            "We rented a U-Haul and a hummer."
        );
        assert_eq!(light("the drum uh broke"), "The drum broke.");
    }

    #[test]
    fn stutters_on_small_words_go_but_meaningful_doubles_stay() {
        assert_eq!(
            light("send it to the the team today"),
            "Send it to the team today."
        );
        assert_eq!(light("I I think we should go"), "I think we should go.");
        assert_eq!(light("that was very very good"), "That was very very good.");
        assert_eq!(
            light("she had had enough of it"),
            "She had had enough of it."
        );
        assert_eq!(light("bye bye for now"), "Bye bye for now.");
    }

    #[test]
    fn the_word_i_is_always_a_capital() {
        assert_eq!(
            light("why does it crash when i rotate the phone"),
            "Why does it crash when I rotate the phone?"
        );
        assert_eq!(
            light("i'm sure i'll be there if i can"),
            "I'm sure I'll be there if I can."
        );
        assert_eq!(
            light("i’ve read it and i'd sign"),
            "I’ve read it and I'd sign."
        );
        // Not inside other words, code or addresses.
        assert_eq!(light("set i = 0 in the loop"), "Set i = 0 in the loop.");
        assert_eq!(light("the file is i/o bound"), "The file is i/o bound.");
    }

    #[test]
    fn very_short_input_is_not_given_a_full_stop() {
        assert_eq!(light("yes"), "Yes");
        assert_eq!(light("um okay"), "Okay");
        assert_eq!(light("thanks so"), "Thanks so");
        assert_eq!(light("Maria"), "Maria");
    }

    #[test]
    fn empty_and_silent_input_stays_empty() {
        assert_eq!(light(""), "");
        assert_eq!(light("   "), "");
        assert_eq!(light("um"), "");
    }

    #[test]
    fn light_is_stable_when_run_twice() {
        for t in [
            "um so the meeting is at 3pm",
            "can you send me the report",
            "the docs are at https://aurathex.com/docs",
            "send it to the the team today",
        ] {
            let once = light(t);
            assert_eq!(light(&once), once, "{t}");
        }
    }
}
