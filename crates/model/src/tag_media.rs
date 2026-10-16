//! Media named inside add-in tag values.
//!
//! deckrt's interactive objects keep their settings as JSON in a shape tag whose name starts with
//! [`PREFIX`]. Pictures those settings use are written as `"media:<id>"` strings. This module
//! finds and rewrites those tokens, so the PPTX writer can turn them into relationships (and the
//! reader back into media ids) and the engine's media walker can count and remap them, without
//! either knowing the JSON's shape.

/// Tag names that may carry media tokens.
pub const PREFIX: &str = "DECKRT.";

/// Whether a tag may carry media tokens.
pub fn carries_media(name: &str) -> bool {
    name.len() >= PREFIX.len() && name.is_char_boundary(PREFIX.len()) && name[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
}

/// Every `"<scheme>:<value>"` token in a JSON string value. Tokens are matched as whole JSON
/// strings: a quote, the scheme, a colon, the value up to the closing quote.
fn tokens<'a>(value: &'a str, scheme: &str) -> Vec<(usize, usize, &'a str)> {
    let needle = format!("\"{scheme}:");
    let mut out = vec![];
    let mut from = 0;
    while let Some(i) = value[from..].find(&needle) {
        let start = from + i;
        let vstart = start + needle.len();
        let Some(len) = value[vstart..].find('"') else { break };
        let inner = &value[vstart..vstart + len];
        if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            out.push((start, vstart + len + 1, inner));
        }
        from = vstart + len + 1;
    }
    out
}

/// The media ids a tag value names.
pub fn media_ids(value: &str) -> Vec<crate::MediaId> {
    tokens(value, "media").into_iter().filter_map(|(_, _, v)| v.parse::<u32>().ok()).map(crate::MediaId).collect()
}

/// Rewrite every `"<from>:<value>"` token through `f`, which returns the new scheme and value;
/// tokens `f` declines are left as they are.
pub fn rewrite(value: &str, from: &str, mut f: impl FnMut(&str) -> Option<(String, String)>) -> String {
    let toks = tokens(value, from);
    if toks.is_empty() {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut at = 0;
    for (start, end, inner) in toks {
        out.push_str(&value[at..start]);
        match f(inner) {
            Some((scheme, v)) => {
                out.push('"');
                out.push_str(&scheme);
                out.push(':');
                out.push_str(&v);
                out.push('"');
            }
            None => out.push_str(&value[start..end]),
        }
        at = end;
    }
    out.push_str(&value[at..]);
    out
}

/// Replace media ids in place (the engine's media walker).
pub fn remap(value: &str, mut f: impl FnMut(crate::MediaId) -> crate::MediaId) -> String {
    rewrite(value, "media", |v| {
        let id = v.parse::<u32>().ok()?;
        let new = f(crate::MediaId(id));
        Some(("media".into(), new.0.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaId;

    #[test]
    fn tokens_are_found_and_rewritten() {
        let v = r#"{"images":["media:7","media:9"],"text":"not media:3 here","x":"media:"}"#;
        assert_eq!(media_ids(v), vec![MediaId(7), MediaId(9)]);
        let out = rewrite(v, "media", |n| Some(("rel".into(), format!("rId{n}"))));
        assert_eq!(out, r#"{"images":["rel:rId7","rel:rId9"],"text":"not media:3 here","x":"media:"}"#);
        let back = rewrite(&out, "rel", |r| r.strip_prefix("rId").map(|n| ("media".into(), n.to_string())));
        assert_eq!(back, v);
        assert_eq!(remap(v, |m| MediaId(m.0 + 1)), r#"{"images":["media:8","media:10"],"text":"not media:3 here","x":"media:"}"#);
        assert_eq!(rewrite("", "media", |_| None), "");
        assert!(carries_media("DECKRT.OBJECT") && carries_media("deckrt.object") && !carries_media("OTHER") && !carries_media("DECK"));
    }
}
