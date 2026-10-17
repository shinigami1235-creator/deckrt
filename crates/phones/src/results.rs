//! `<deckname>.results.json`: the polls' tallies after a show, next to the
//! deck. Votes live in the show's state while it runs; this file is the
//! only thing that survives the show, and the deck itself is never touched.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollResult {
    /// The poll object's id in the sidecar.
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    /// One count per option.
    pub counts: Vec<u32>,
    #[serde(default)]
    pub correct: Option<usize>,
    /// Phones that voted.
    pub voters: u32,
    /// Whether the presenter revealed the answer during the show.
    #[serde(default)]
    pub revealed: bool,
    /// `choice`, `word_cloud`, `free_text`, `rating` or `ranking`.
    #[serde(default = "choice")]
    pub kind: String,
    /// Typed answers: each word of a word cloud with its count, or each free-text answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
    /// A rating's average.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average: Option<f64>,
    /// A ranking's answers in the room's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
}

fn choice() -> String {
    "choice".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShowResults {
    pub deck: String,
    /// RFC 3339, UTC, when the file was written.
    pub written_at: String,
    /// Phones that joined at any point.
    pub phones: u32,
    pub polls: Vec<PollResult>,
    /// The questions phones asked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<serde_json::Value>,
    /// The feedback form's answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<Feedback>,
    /// Reactions sent on each slide: `{"slide": 3, "heart": 12, "lost": 2}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    pub answers: u32,
    /// The ratings' average, when any phone gave one.
    pub average: Option<f64>,
    /// How many gave each rating, 1 to 5.
    pub ratings: [u32; 5],
    /// `{"name", "rating", "useful", "better"}` for each phone that answered.
    pub comments: Vec<serde_json::Value>,
}

/// `deck.pptx` becomes `deck.results.json` in the same directory.
pub fn results_path_for(deck: &Path) -> PathBuf {
    let stem = deck.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "deck".into());
    deck.with_file_name(format!("{stem}.results.json"))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Write the results beside the deck. The timestamp is filled in here. A
/// temp file and a rename, so a crash mid-write leaves the previous file.
pub fn write_results(deck: &Path, mut results: ShowResults) -> Result<PathBuf, String> {
    let path = results_path_for(deck);
    results.written_at = now_rfc3339();
    if results.deck.is_empty() {
        results.deck = deck.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    let json = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, json).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        // Windows refuses to rename over a file another program holds open.
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path).map_err(|_| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_sit_next_to_the_deck() {
        #[cfg(windows)]
        {
            let p = results_path_for(Path::new("C:\\talks\\quarterly review.pptx"));
            assert_eq!(p, PathBuf::from("C:\\talks\\quarterly review.results.json"));
        }
        let p = results_path_for(Path::new("quarterly review.pptx"));
        assert_eq!(p, PathBuf::from("quarterly review.results.json"));
        let p = results_path_for(Path::new("/home/x/deck.pptx"));
        assert_eq!(p, PathBuf::from("/home/x/deck.results.json"));
    }

    #[test]
    fn write_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("my deck.pptx");
        std::fs::write(&deck, b"").unwrap();
        let r = ShowResults {
            deck: String::new(),
            written_at: String::new(),
            phones: 3,
            polls: vec![PollResult {
                id: "obj_poll".into(),
                question: "Which bone?".into(),
                options: vec!["Ulna".into(), "Radius".into()],
                counts: vec![2, 1],
                correct: Some(1),
                voters: 3,
                revealed: true,
                kind: "choice".into(),
                answers: vec![],
                average: None,
                order: vec![],
            }],
            questions: vec![],
            feedback: None,
            reactions: vec![],
        };
        let path = write_results(&deck, r).unwrap();
        assert_eq!(path, dir.path().join("my deck.results.json"));
        let back: ShowResults = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.deck, "my deck.pptx");
        assert!(back.written_at.ends_with('Z'));
        assert_eq!(back.polls[0].counts, vec![2, 1]);
        // A second write replaces the first.
        let r2 = ShowResults { deck: String::new(), written_at: String::new(), phones: 0, polls: vec![], questions: vec![], feedback: None, reactions: vec![] };
        write_results(&deck, r2).unwrap();
        let back: ShowResults = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(back.polls.is_empty());
        assert!(!dir.path().join(format!("my deck.results.json.{}.tmp", std::process::id())).exists());
    }
}
