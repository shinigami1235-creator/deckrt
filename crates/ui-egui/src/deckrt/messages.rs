//! deckrt: command errors in plain words. The engine's messages are written for the command line
//! ("invalid parameters for `table.merge`: missing `from`"); the status line shows the reason
//! alone, as a sentence, and names what to do where the reason is the selection.

/// The status line's version of an engine error.
pub fn plain_error(e: &str) -> String {
    let mut reason = e.trim();
    for prefix in ["is not available right now: ", "`: "] {
        if (reason.starts_with("command `") || reason.starts_with("invalid parameters for `"))
            && let Some(i) = reason.find(prefix)
        {
            reason = &reason[i + prefix.len()..];
            break;
        }
    }
    // "shape 363 has no animations to copy": the id means nothing to a person.
    let words: Vec<&str> = reason.split_whitespace().collect();
    let owned;
    if words.len() > 2 && words[0].eq_ignore_ascii_case("shape") && words[1].chars().all(|c| c.is_ascii_digit()) {
        owned = format!("this object {}", words[2..].join(" "));
        reason = &owned;
    }
    let lower = reason.to_lowercase();
    let known = [
        ("nothing selected", "Select something on the slide first."),
        ("no selection", "Select something on the slide first."),
        ("select text or a shape", "Select some text or a shape first."),
        ("no animation to change", "Select an object that has an animation first."),
        ("missing `from`", "Select the cells to merge first."),
        ("that cell isn't merged", "Select a merged cell to split it."),
        ("nothing to regroup", "There's no group that was ungrouped here to put back together."),
    ];
    for (k, plain) in known {
        if lower.starts_with(k) {
            return plain.to_string();
        }
    }
    // Drop parameter names from what's left: "(select an animated object or give `index`)".
    let mut out = String::new();
    let mut depth = 0;
    for c in reason.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            '`' => {}
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let mut out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(first) = out.chars().next() {
        out = first.to_uppercase().collect::<String>() + &out[first.len_utf8()..];
    }
    if !out.is_empty() && !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::plain_error;

    #[test]
    fn engine_errors_read_as_sentences() {
        assert_eq!(plain_error("command `animation.set` is not available right now: nothing selected"), "Select something on the slide first.");
        assert_eq!(plain_error("invalid parameters for `table.merge`: missing `from`"), "Select the cells to merge first.");
        assert_eq!(plain_error("invalid parameters for `comment.next`: no comments"), "No comments.");
        assert_eq!(
            plain_error("invalid parameters for `animation`: no animation to change (select an animated object or give `index`)"),
            "Select an object that has an animation first."
        );
        assert_eq!(plain_error("This slide has no transition."), "This slide has no transition.");
        assert_eq!(plain_error("invalid parameters for `x`: the `id` is wrong (see docs)"), "The id is wrong.");
        assert_eq!(
            plain_error("invalid parameters for `animation.painter`: shape 363 has no animations to copy"),
            "This object has no animations to copy."
        );
    }
}
