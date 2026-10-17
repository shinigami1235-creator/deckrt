//! Polls: the tally, and where a poll leads once the answer is revealed. Nothing here moves the
//! show; the presenter view offers the winner's slide and the presenter clicks.

use std::collections::BTreeMap;

use deckcraft_model::Presentation;

use crate::object::PollConfig;

/// Votes per option from the phones' votes (phone id → option). A vote outside the range is not
/// counted.
pub fn tally(option_count: usize, votes: &BTreeMap<String, usize>) -> Vec<u32> {
    let mut counts = vec![0u32; option_count];
    for v in votes.values() {
        if let Some(c) = counts.get_mut(*v) {
            *c = c.saturating_add(1);
        }
    }
    counts
}

/// The options with the most votes: one index, several on a tie, none before anyone has voted.
pub fn winners(counts: &[u32]) -> Vec<usize> {
    let max = counts.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return vec![];
    }
    counts.iter().enumerate().filter(|(_, c)| **c == max).map(|(i, _)| i).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// Index into the poll's options.
    pub option: usize,
    pub label: String,
    /// The slide the target names, or none when the option has no target or its slide is gone.
    pub slide: Option<usize>,
    /// The option has a target, whether or not its slide still exists.
    pub has_target: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Branch {
    None,
    Winner(Offer),
    Tie(Vec<Offer>),
}

/// The slide index a target key points at, or none when that slide is gone.
pub fn target_slide(target: Option<&str>, doc: &Presentation) -> Option<usize> {
    let key = target?;
    doc.slides.iter().position(|s| crate::object::slide_key(s) == Some(key))
}

/// What to offer after the reveal: the winner's target, or on a tie each tied option's target.
/// Before the reveal, or with no votes, nothing.
pub fn branch(config: &PollConfig, counts: &[u32], revealed: bool, doc: &Presentation) -> Branch {
    if !revealed {
        return Branch::None;
    }
    let w = winners(&config.options.iter().enumerate().map(|(i, _)| counts.get(i).copied().unwrap_or(0)).collect::<Vec<_>>());
    if w.is_empty() {
        return Branch::None;
    }
    let offers: Vec<Offer> = w
        .into_iter()
        .map(|option| {
            let target = config.targets.get(option).and_then(|t| t.as_deref());
            Offer {
                option,
                label: config.options.get(option).cloned().unwrap_or_default(),
                slide: target_slide(target, doc),
                has_target: target.is_some(),
            }
        })
        .collect();
    if offers.len() == 1 {
        Branch::Winner(offers.into_iter().next().unwrap_or(Offer { option: 0, label: String::new(), slide: None, has_target: false }))
    } else {
        Branch::Tie(offers)
    }
}

/// The button's text, named the way the Next box names a slide.
pub fn go_to_label(slide: usize) -> String {
    format!("Go to slide {}", slide + 1)
}

/// Percentages that add up to 100 (largest remainder), for the bars.
pub fn percentages(counts: &[u32]) -> Vec<u32> {
    let total: u64 = counts.iter().map(|c| *c as u64).sum();
    if total == 0 {
        return vec![0; counts.len()];
    }
    let exact: Vec<f64> = counts.iter().map(|c| *c as f64 * 100.0 / total as f64).collect();
    let mut out: Vec<u32> = exact.iter().map(|e| e.floor() as u32).collect();
    let mut rest = 100u32.saturating_sub(out.iter().sum::<u32>());
    let mut order: Vec<usize> = (0..counts.len()).collect();
    order.sort_by(|a, b| (exact[*b] - exact[*b].floor()).partial_cmp(&(exact[*a] - exact[*a].floor())).unwrap_or(std::cmp::Ordering::Equal));
    for i in order {
        if rest == 0 {
            break;
        }
        out[i] += 1;
        rest -= 1;
    }
    out
}

/// Longest word or free-text answer kept, in characters.
pub const TEXT_MAX_CHARS: usize = 140;
/// Words one phone may give to a word cloud.
pub const CLOUD_WORDS: usize = 3;

/// A phone's text cleaned for the screen: one line, trimmed, at most [`TEXT_MAX_CHARS`].
pub fn clean_text(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().filter(|c| !c.is_control()).take(TEXT_MAX_CHARS).collect()
}

/// The words of a word-cloud answer: split on commas or new lines (a phrase may have spaces),
/// cleaned, at most [`CLOUD_WORDS`], empty ones dropped.
pub fn cloud_words(s: &str) -> Vec<String> {
    s.split([',', '\n', ';'])
        .map(clean_text)
        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation() && c != '-' && c != '+').to_string())
        .filter(|w| !w.is_empty())
        .take(CLOUD_WORDS)
        .collect()
}

/// Word cloud: each word with how many phones gave it, most first. Words match without regard
/// to case; the spelling shown is the first one given. Ties keep the order words first came in.
pub fn cloud(texts: &BTreeMap<String, Vec<String>>) -> Vec<(String, u32)> {
    let mut out: Vec<(String, String, u32)> = vec![];
    for words in texts.values() {
        let mut seen: Vec<String> = vec![];
        for w in words {
            let key = w.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            match out.iter_mut().find(|(k, _, _)| *k == key) {
                Some(e) => e.2 += 1,
                None => out.push((key, w.clone(), 1)),
            }
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out.into_iter().map(|(_, w, n)| (w, n)).collect()
}

/// Rating: how many phones gave each value from 1 to `scale`, and the average (none before
/// anyone has rated).
pub fn rating(scale: u32, votes: &BTreeMap<String, usize>) -> (Vec<u32>, Option<f64>) {
    let n = scale.clamp(2, 10) as usize;
    let counts = tally(n, votes);
    let total: u32 = counts.iter().sum();
    let avg = (total > 0).then(|| counts.iter().enumerate().map(|(i, c)| (i + 1) as f64 * *c as f64).sum::<f64>() / total as f64);
    (counts, avg)
}

/// Ranking: the answers in the room's order, each with its average place (1 is first). A phone
/// that left answers out ranks them after the ones it placed, in the order they are listed.
pub fn ranking(option_count: usize, ranks: &BTreeMap<String, Vec<usize>>) -> Vec<(usize, f64)> {
    let mut sum = vec![0f64; option_count];
    let n = ranks.len();
    for order in ranks.values() {
        let mut placed: Vec<usize> = order.iter().copied().filter(|&o| o < option_count).collect();
        placed.dedup();
        for o in 0..option_count {
            if !placed.contains(&o) {
                placed.push(o);
            }
        }
        for (place, o) in placed.iter().enumerate() {
            sum[*o] += (place + 1) as f64;
        }
    }
    let mut out: Vec<(usize, f64)> = (0..option_count).map(|o| (o, if n == 0 { 0.0 } else { sum[o] / n as f64 })).collect();
    if n > 0 {
        out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    }
    out
}

/// Free text: the answers to show, oldest phone first, without the ones the presenter hid.
pub fn answers<'a>(texts: &'a BTreeMap<String, Vec<String>>, hidden: &std::collections::BTreeSet<String>) -> Vec<(&'a str, &'a str)> {
    texts.iter().filter(|(id, _)| !hidden.contains(*id)).filter_map(|(id, t)| t.first().map(|a| (id.as_str(), a.as_str()))).collect()
}

/// How many phones have answered, whatever the kind.
pub fn answered(votes: &BTreeMap<String, usize>, texts: &BTreeMap<String, Vec<String>>, ranks: &BTreeMap<String, Vec<usize>>) -> usize {
    let mut ids: std::collections::BTreeSet<&String> = votes.keys().collect();
    ids.extend(texts.keys());
    ids.extend(ranks.keys());
    ids.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::SLIDE_TAG;
    use deckcraft_model::Slide;
    use std::sync::Arc;

    fn doc_with_keys(keys: &[&str]) -> Presentation {
        let mut d = Presentation::default();
        d.slides.clear();
        for k in keys {
            let mut s = Slide::default();
            if !k.is_empty() {
                s.tags.push((SLIDE_TAG.to_string(), k.to_string()));
            }
            d.slides.push(Arc::new(s));
        }
        d
    }

    #[test]
    fn tally_counts_in_range_votes_only() {
        let mut v = BTreeMap::new();
        v.insert("a".to_string(), 1usize);
        v.insert("b".to_string(), 1);
        v.insert("c".to_string(), 9);
        assert_eq!(tally(3, &v), vec![0, 2, 0]);
    }

    #[test]
    fn winner_tie_and_nothing() {
        assert_eq!(winners(&[0, 0]), Vec::<usize>::new());
        assert_eq!(winners(&[1, 3, 2]), vec![1]);
        assert_eq!(winners(&[3, 3, 2]), vec![0, 1]);
    }

    #[test]
    fn the_branch_follows_the_reveal_and_the_targets() {
        let doc = doc_with_keys(&["", "sA", "sB"]);
        let c = PollConfig {
            question: "Q".into(),
            options: vec!["Ulna".into(), "Radius".into(), "Humerus".into()],
            correct: Some(1),
            reveal_correct: true,
            targets: vec![Some("sA".into()), Some("gone".into()), None],
            ..Default::default()
        };
        assert_eq!(branch(&c, &[2, 1, 0], false, &doc), Branch::None);
        assert_eq!(branch(&c, &[0, 0, 0], true, &doc), Branch::None);
        match branch(&c, &[2, 1, 0], true, &doc) {
            Branch::Winner(o) => {
                assert_eq!(o.slide, Some(1));
                assert!(o.has_target);
            }
            other => panic!("{other:?}"),
        }
        match branch(&c, &[2, 2, 0], true, &doc) {
            Branch::Tie(o) => {
                assert_eq!(o.len(), 2);
                assert_eq!(o[1].slide, None, "a target whose slide left the deck");
                assert!(o[1].has_target);
            }
            other => panic!("{other:?}"),
        }
        match branch(&c, &[0, 0, 5], true, &doc) {
            Branch::Winner(o) => assert!(!o.has_target),
            other => panic!("{other:?}"),
        }
        assert_eq!(go_to_label(6), "Go to slide 7");
    }

    #[test]
    fn percentages_add_up() {
        assert_eq!(percentages(&[1, 1, 1]), vec![34, 33, 33]);
        assert_eq!(percentages(&[0, 0]), vec![0, 0]);
        assert_eq!(percentages(&[3, 1]), vec![75, 25]);
    }

    #[test]
    fn word_cloud_counts_each_phone_once_per_word() {
        let mut t = BTreeMap::new();
        t.insert("a".to_string(), cloud_words("Collagen, elastin"));
        t.insert("b".to_string(), cloud_words("collagen\nFibrillin, collagen"));
        t.insert("c".to_string(), cloud_words(" Elastin! "));
        let c = cloud(&t);
        assert_eq!(c[0], ("Collagen".to_string(), 2));
        assert_eq!(c[1], ("elastin".to_string(), 2));
        assert_eq!(c[2], ("Fibrillin".to_string(), 1));
        assert_eq!(cloud_words("a, b, c, d").len(), 3);
    }

    #[test]
    fn rating_and_ranking() {
        let mut v = BTreeMap::new();
        v.insert("a".to_string(), 4);
        v.insert("b".to_string(), 2);
        let (counts, avg) = rating(5, &v);
        assert_eq!(counts, vec![0, 0, 1, 0, 1]);
        assert_eq!(avg, Some(4.0));
        let mut r = BTreeMap::new();
        r.insert("a".to_string(), vec![2, 0, 1]);
        r.insert("b".to_string(), vec![2, 1]);
        let order = ranking(3, &r);
        assert_eq!(order[0].0, 2);
        assert_eq!(order[0].1, 1.0);
    }
}
