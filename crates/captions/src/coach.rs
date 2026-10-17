//! deckrt: Rehearse with Coach. What was heard during a rehearsal, laid against the slides
//! shown, gives each slide its time, pace, filler words, long pauses, and whether its text was
//! read out word for word.

use std::collections::{BTreeMap, HashSet};

/// A phrase heard, timed on the show's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// Slide `slide` was on screen from `from` to `to` (show clock).
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub slide: usize,
    pub from: f64,
    pub to: f64,
}

/// A quiet spell between phrases this long or longer is a pause worth seeing.
pub const PAUSE_SECS: f64 = 5.0;
/// Under this many words a slide gets no pace of its own.
const PACE_MIN_WORDS: usize = 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlideReport {
    pub slide: usize,
    /// Time on screen, every visit added up.
    pub secs: f64,
    /// Words counted for pace. Thai isn't split into words, so Thai speech adds none.
    pub words: usize,
    pub fillers: BTreeMap<String, usize>,
    /// Quiet spells of [`PAUSE_SECS`] or more between phrases.
    pub pauses: Vec<f64>,
    /// The share of the slide's own text said word for word, 0 to 1. None when the slide has
    /// too little text to tell.
    pub read_out: Option<f64>,
    /// What was heard, in order.
    pub said: Vec<String>,
    /// Most of what was said was Thai.
    pub thai: bool,
}

impl SlideReport {
    pub fn fillers_total(&self) -> usize {
        self.fillers.values().sum()
    }

    /// Words a minute over the time on the slide.
    pub fn pace(&self) -> Option<f64> {
        (!self.thai && self.words >= PACE_MIN_WORDS && self.secs >= 10.0).then(|| self.words as f64 / self.secs * 60.0)
    }

    /// The slide was mostly read out.
    pub fn read_aloud(&self) -> bool {
        self.read_out.is_some_and(|r| r >= 0.5)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub total: f64,
    /// One per slide shown, in slide order.
    pub slides: Vec<SlideReport>,
    pub words: usize,
    pub fillers: BTreeMap<String, usize>,
    pub thai: bool,
}

impl Report {
    pub fn fillers_total(&self) -> usize {
        self.fillers.values().sum()
    }

    /// Words a minute over the whole rehearsal.
    pub fn pace(&self) -> Option<f64> {
        (!self.thai && self.words >= PACE_MIN_WORDS && self.total >= 10.0).then(|| self.words as f64 / self.total * 60.0)
    }

    /// The filler words, most used first.
    pub fn fillers_ranked(&self) -> Vec<(&str, usize)> {
        let mut v: Vec<(&str, usize)> = self.fillers.iter().map(|(k, n)| (k.as_str(), *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v
    }
}

/// Lay `heard` against `spans`. `texts[i]` is slide `i`'s own text (title and body, no notes).
pub fn analyse(heard: &[Heard], spans: &[Span], texts: &[String]) -> Report {
    let mut by_slide: BTreeMap<usize, SlideReport> = BTreeMap::new();
    for s in spans {
        let r = by_slide.entry(s.slide).or_insert_with(|| SlideReport { slide: s.slide, ..Default::default() });
        r.secs += (s.to - s.from).max(0.0);
    }
    let mut heard: Vec<&Heard> = heard.iter().filter(|h| !h.text.trim().is_empty()).collect();
    heard.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    let mut thai_chars = 0usize;
    let mut latin_chars = 0usize;
    // The span each phrase belongs to, so pauses are only counted within one visit to a slide.
    let mut prev: Option<(usize, f64)> = None;
    for h in heard {
        let Some(span) = span_of(spans, (h.start + h.end) / 2.0) else { continue };
        let slide = spans[span].slide;
        let r = by_slide.entry(slide).or_insert_with(|| SlideReport { slide, ..Default::default() });
        if let Some((ps, pend)) = prev
            && ps == span
        {
            let gap = h.start - pend;
            if gap >= PAUSE_SECS {
                r.pauses.push(gap);
            }
        }
        prev = Some((span, h.end));
        r.words += latin_words(&h.text).len();
        for f in fillers(&h.text) {
            *r.fillers.entry(f.to_string()).or_default() += 1;
        }
        r.said.push(h.text.clone());
        thai_chars += h.text.chars().filter(|c| is_thai(*c)).count();
        latin_chars += h.text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    }
    let mut report = Report { total: spans.iter().map(|s| (s.to - s.from).max(0.0)).sum(), ..Default::default() };
    for (slide, mut r) in by_slide {
        let said = r.said.join(" ");
        let t = said.chars().filter(|c| is_thai(*c)).count();
        let l = said.chars().filter(|c| c.is_ascii_alphabetic()).count();
        r.thai = t > l;
        r.read_out = texts.get(slide).and_then(|text| read_out(text, &said));
        report.words += r.words;
        for (k, n) in &r.fillers {
            *report.fillers.entry(k.clone()).or_default() += n;
        }
        report.slides.push(r);
    }
    report.thai = thai_chars > latin_chars;
    report
}

/// The span on screen at `t`; before the first one, the first; after a gap, the last one begun.
fn span_of(spans: &[Span], t: f64) -> Option<usize> {
    if spans.is_empty() {
        return None;
    }
    if let Some(i) = spans.iter().position(|s| t >= s.from && t < s.to) {
        return Some(i);
    }
    spans.iter().rposition(|s| s.from <= t).or(Some(0))
}

fn is_thai(c: char) -> bool {
    ('\u{0E00}'..='\u{0E7F}').contains(&c)
}

/// Lower-cased Latin-script words.
fn latin_words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'') || is_thai(c))
        .filter(|w| w.chars().any(|c| c.is_ascii_alphabetic()))
        .map(|w| w.to_lowercase())
        .collect()
}

/// Sounds that stand in for words, and the name each counts under.
const SOUNDS: &[(&str, &str)] = &[
    ("um", "um"),
    ("umm", "um"),
    ("uhm", "um"),
    ("uh", "uh"),
    ("uhh", "uh"),
    ("er", "er"),
    ("erm", "er"),
    ("ah", "ah"),
    ("ahh", "ah"),
    ("hmm", "hmm"),
    ("mm", "hmm"),
];
const PHRASES: &[&str] = &["you know", "i mean", "sort of", "basically", "actually", "literally"];
/// Thai sounds that must stand on their own (อ่า is also the start of อ่าน).
const THAI_SOUNDS: &[&str] = &["เอ่อ", "อ่า", "อืม"];
/// Thai phrases counted wherever they appear.
const THAI_PHRASES: &[&str] = &["แบบว่า", "ประมาณว่า"];

/// The filler words in one phrase, by the name each counts under.
pub fn fillers(text: &str) -> Vec<&'static str> {
    let mut out = vec![];
    let lower = text.to_lowercase();
    let words = latin_words(&lower);
    for w in &words {
        if let Some((_, name)) = SOUNDS.iter().find(|(s, _)| s == w) {
            out.push(*name);
        }
    }
    let joined = format!(" {} ", words.join(" "));
    for p in PHRASES {
        out.extend(std::iter::repeat_n(*p, joined.matches(&format!(" {p} ")).count()));
    }
    // "like" only when set off by a comma: "and, like, the dermis".
    let b = lower.as_bytes();
    let mut from = 0;
    while let Some(i) = lower[from..].find("like") {
        let at = from + i;
        let end = at + 4;
        from = end;
        let word_start = at == 0 || !(b[at - 1] as char).is_alphanumeric();
        let word_end = end >= b.len() || !(b[end] as char).is_alphanumeric();
        if !(word_start && word_end) {
            continue;
        }
        let before = lower[..at].trim_end().ends_with(',');
        let after = lower[end..].trim_start().starts_with(',');
        if before || after {
            out.push("like");
        }
    }
    for s in THAI_SOUNDS {
        let mut from = 0;
        while let Some(i) = text[from..].find(s) {
            let at = from + i;
            let end = at + s.len();
            from = end;
            let last = s.chars().last();
            let before_ok = text[..at].chars().next_back().is_none_or(|c| !is_thai(c));
            // Drawn out (อ่าาา) still counts; a Thai letter after it makes it part of a word.
            let rest = text[end..].trim_start_matches(|c| Some(c) == last);
            let after_ok = rest.chars().next().is_none_or(|c| !is_thai(c));
            if before_ok && after_ok {
                out.push(s);
            }
        }
    }
    for p in THAI_PHRASES {
        out.extend(std::iter::repeat_n(*p, text.matches(p).count()));
    }
    out
}

/// How much of a slide's text was said word for word: the share of its runs of four words
/// (eight letters for Thai) that turn up in what was said. None when the slide has too little
/// text to tell.
fn read_out(slide: &str, said: &str) -> Option<f64> {
    let thai = slide.chars().filter(|c| is_thai(*c)).count() > slide.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let (slide_runs, said_runs): (Vec<String>, HashSet<String>) = if thai {
        let letters = |s: &str| s.chars().filter(|c| is_thai(*c)).collect::<Vec<char>>();
        let (a, b) = (letters(slide), letters(said));
        if a.len() < 40 {
            return None;
        }
        let runs = |v: &[char]| v.windows(8).map(|w| w.iter().collect::<String>()).collect::<Vec<_>>();
        (runs(&a), runs(&b).into_iter().collect())
    } else {
        let (a, b) = (latin_words(slide), latin_words(said));
        if a.len() < 12 {
            return None;
        }
        let runs = |v: &[String]| v.windows(4).map(|w| w.join(" ")).collect::<Vec<_>>();
        (runs(&a), runs(&b).into_iter().collect())
    };
    let hit = slide_runs.iter().filter(|r| said_runs.contains(*r)).count();
    Some(hit as f64 / slide_runs.len().max(1) as f64)
}

/// Seconds as m:ss.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(text: &str, start: f64, end: f64) -> Heard {
        Heard { text: text.into(), start, end }
    }

    #[test]
    fn fillers_are_found_and_words_that_look_like_them_are_not() {
        assert_eq!(fillers("Um, so the dermis, uh, holds collagen."), vec!["um", "uh"]);
        assert_eq!(fillers("It's, like, a scaffold. I like elastin."), vec!["like"]);
        assert_eq!(fillers("You know, it's basically type one."), vec!["you know", "basically"]);
        assert!(fillers("Read the dermis section, Umberto.").is_empty());
        assert_eq!(fillers("เอ่อ คอลลาเจน แบบว่า สำคัญ"), vec!["เอ่อ", "แบบว่า"]);
        assert!(fillers("อ่านหนังสือ").is_empty(), "อ่า starts อ่าน");
        assert_eq!(fillers("อ่าาา ไฟโบรบลาสต์"), vec!["อ่า"]);
    }

    #[test]
    fn phrases_land_on_their_slides_with_pauses_and_pace() {
        let spans = vec![Span { slide: 0, from: 0.0, to: 30.0 }, Span { slide: 1, from: 30.0, to: 40.0 }, Span { slide: 0, from: 40.0, to: 50.0 }];
        let words20 = "fibroblasts make most of the collagen in the dermis and type one and three are the main ones in adult skin";
        let heard = vec![
            h(words20, 1.0, 9.0),
            h("um, elastin sits on fibrillin", 16.0, 19.0), // 7 s of quiet before it
            h("the next slide", 31.0, 33.0),
            h("back again", 41.0, 42.0),
        ];
        let r = analyse(&heard, &spans, &[]);
        assert_eq!(r.slides.len(), 2);
        let s0 = &r.slides[0];
        assert_eq!(s0.secs, 40.0);
        assert_eq!(s0.pauses, vec![7.0]);
        assert_eq!(s0.fillers.get("um"), Some(&1));
        assert_eq!(s0.said.len(), 3);
        assert_eq!(s0.words, 21 + 5 + 2);
        assert!((s0.pace().unwrap_or(0.0) - 42.0).abs() < 0.01);
        assert_eq!(r.slides[1].pace(), None, "too few words");
        assert_eq!(r.total, 50.0);
        assert_eq!(r.fillers_total(), 1);
    }

    #[test]
    fn a_slide_read_out_is_noticed() {
        let text = "Collagen type one gives the dermis its strength while elastin lets the skin spring back".to_string();
        let spans = vec![Span { slide: 0, from: 0.0, to: 20.0 }, Span { slide: 1, from: 20.0, to: 40.0 }];
        let heard = vec![
            h("So, collagen type one gives the dermis its strength, while elastin lets the skin spring back.", 1.0, 8.0),
            h("Here the fibroblasts slow down with age and sun.", 21.0, 25.0),
        ];
        let r = analyse(&heard, &spans, &[text.clone(), text]);
        assert!(r.slides[0].read_aloud(), "{:?}", r.slides[0].read_out);
        assert!(!r.slides[1].read_aloud());
        assert_eq!(clock(250.4), "4:10");
    }
}
