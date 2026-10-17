//! deckrt: live captions from the microphone, worked out on the laptop with whisper.cpp. No
//! internet is needed once the speech model is on disk; the model is downloaded once, on the
//! presenter's say-so, into the settings folder.
//!
//! - [`Model`]: which speech model, where it lives, and its download.
//! - [`Captioner`] (feature `engine`): the microphone, a speech detector that cuts the sound at
//!   pauses, and the recogniser on its own thread. The app reads [`Event`]s from it each frame.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The speech models offered. Both understand Thai and English.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    /// About 150 MB. Quick on any laptop, weaker on accents and Thai.
    Base,
    /// About 490 MB. Better words, needs a recent processor to keep up.
    #[default]
    Small,
}

impl Model {
    pub const ALL: [Model; 2] = [Model::Base, Model::Small];
    pub fn file_name(self) -> &'static str {
        match self {
            Model::Base => "ggml-base.bin",
            Model::Small => "ggml-small.bin",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Model::Base => "Faster (150 MB)",
            Model::Small => "More accurate (490 MB)",
        }
    }
    /// The download size in bytes, for the progress bar.
    pub fn bytes(self) -> u64 {
        match self {
            Model::Base => 147_951_465,
            Model::Small => 487_601_967,
        }
    }
    pub fn url(self) -> String {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", self.file_name())
    }
    /// Where the model lives under the settings folder.
    pub fn path(self, settings_dir: &Path) -> PathBuf {
        settings_dir.join("speech").join(self.file_name())
    }
    /// The model is on disk and looks like a whisper.cpp model.
    pub fn ready(self, settings_dir: &Path) -> bool {
        looks_like_model(&self.path(settings_dir))
    }
}

/// A whisper.cpp model file starts with the `ggml` magic, stored little-endian.
fn looks_like_model(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).is_ok() && magic == [0x6c, 0x6d, 0x67, 0x67] && f.metadata().map(|m| m.len() > 10_000_000).unwrap_or(false)
}

/// What the presenter chose in the Captions dialog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: Model,
    /// `auto`, `en` or `th`: the language spoken.
    pub language: String,
    /// Write the captions in English whatever is spoken.
    pub english: bool,
    /// Lines on the audience screen.
    pub on_screen: bool,
    /// Lines on the phones.
    pub on_phones: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: Model::Small, language: "auto".into(), english: false, on_screen: true, on_phones: true }
    }
}

// ---------------------------------------------------------------------------------------------
// The download

/// A model download running in `curl` (shipped with Windows 10 and 11, and macOS and Linux).
pub struct Download {
    child: std::process::Child,
    part: PathBuf,
    target: PathBuf,
    expected: u64,
}

impl Download {
    pub fn start(model: Model, settings_dir: &Path) -> Result<Download, String> {
        let target = model.path(settings_dir);
        let dir = target.parent().map(Path::to_path_buf).unwrap_or_else(|| settings_dir.to_path_buf());
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
        let part = target.with_extension("bin.part");
        let _ = std::fs::remove_file(&part);
        let mut cmd = std::process::Command::new("curl");
        cmd.args(["-L", "--fail", "--silent", "--show-error", "--retry", "3", "-o"]).arg(&part).arg(model.url());
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up.
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd.spawn().map_err(|e| format!("Couldn't start the download (curl): {e}"))?;
        Ok(Download { child, part, target, expected: model.bytes() })
    }

    /// 0 to 1.
    pub fn progress(&self) -> f32 {
        let got = std::fs::metadata(&self.part).map(|m| m.len()).unwrap_or(0);
        (got as f64 / self.expected.max(1) as f64).clamp(0.0, 1.0) as f32
    }

    /// `None` while it runs; then the model's path, or why it failed.
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        let status = match self.child.try_wait() {
            Ok(Some(s)) => s,
            Ok(None) => return None,
            Err(e) => return Some(Err(format!("The download stopped: {e}"))),
        };
        if !status.success() {
            let mut why = String::new();
            if let Some(mut err) = self.child.stderr.take() {
                use std::io::Read;
                let _ = err.read_to_string(&mut why);
            }
            let _ = std::fs::remove_file(&self.part);
            let why = why.trim();
            return Some(Err(if why.is_empty() { "The download failed. Check the internet connection.".into() } else { format!("The download failed: {why}") }));
        }
        if !looks_like_model(&self.part) {
            let _ = std::fs::remove_file(&self.part);
            return Some(Err("The downloaded file isn't a speech model.".into()));
        }
        if let Err(e) = std::fs::rename(&self.part, &self.target) {
            return Some(Err(format!("Couldn't keep the model: {e}")));
        }
        Some(Ok(self.target.clone()))
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.part);
    }
}

// ---------------------------------------------------------------------------------------------
// Text

/// The words that help the recogniser on this slide: the longer words of its text, once each,
/// as a short prompt (whisper.cpp reads it as text said just before).
pub fn prompt_from(texts: &[String]) -> String {
    let mut out: Vec<String> = vec![];
    for t in texts {
        for w in t.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '\'')) {
            let w = w.trim_matches('-');
            if w.chars().count() >= 5 && !out.iter().any(|o| o.eq_ignore_ascii_case(w)) {
                out.push(w.to_string());
            }
            if out.len() >= 40 {
                break;
            }
        }
    }
    out.join(", ")
}

/// A recognised line cleaned for the screen: sound labels (`[BLANK_AUDIO]`, `(music)`) and the
/// phrases whisper invents in silence are dropped.
pub fn clean_line(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = t.to_lowercase();
    const INVENTED: &[&str] = &["thank you.", "thanks for watching!", "thank you for watching.", "you", ".", "bye.", "subtitles by the amara.org community"];
    if INVENTED.contains(&lower.as_str()) { String::new() } else { t }
}

/// Speech detection on 16 kHz mono: frames of 30 ms, a speech frame louder than three times the
/// room's noise floor. Cuts a phrase at 0.7 s of quiet after speech, or at 10 s.
#[derive(Debug)]
pub struct Cutter {
    floor: f32,
    buf: Vec<f32>,
    speech_frames: usize,
    quiet_frames: usize,
    pending: Vec<f32>,
    /// Samples read so far, the clock for phrase times.
    pos: usize,
    /// Where the phrase being gathered starts and its last speech frame ends, in samples.
    start: usize,
    last_speech: usize,
}

pub const RATE: usize = 16_000;
const FRAME: usize = RATE * 30 / 1000;

/// What a chunk of sound did to the phrase in progress.
#[derive(Debug, PartialEq)]
pub enum Cut {
    /// A whole phrase, ready to recognise.
    Phrase(Phrase),
    Nothing,
}

/// A phrase's sound and when it was spoken, in seconds since the microphone opened.
#[derive(Debug, PartialEq)]
pub struct Phrase {
    pub audio: Vec<f32>,
    pub start: f64,
    /// The end of the last speech in it (the quiet after it left out).
    pub end: f64,
}

impl Default for Cutter {
    fn default() -> Self {
        Cutter { floor: 0.002, buf: vec![], speech_frames: 0, quiet_frames: 0, pending: vec![], pos: 0, start: 0, last_speech: 0 }
    }
}

impl Cutter {
    /// The phrase so far (for a partial line), when it holds some speech.
    pub fn so_far(&self) -> Option<&[f32]> {
        (self.speech_frames >= 15).then_some(&self.buf[..])
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Cut> {
        let mut out = vec![];
        self.pending.extend_from_slice(samples);
        let mut i = 0;
        while i + FRAME <= self.pending.len() {
            let f = &self.pending[i..i + FRAME];
            i += FRAME;
            self.pos += FRAME;
            let rms = (f.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let speech = rms > (self.floor * 3.0).max(0.006);
            if !speech {
                // The floor follows the quiet frames slowly and falls quickly.
                self.floor = if rms < self.floor { rms.max(0.0005) } else { self.floor * 0.995 + rms * 0.005 };
            }
            if speech || self.speech_frames > 0 {
                self.buf.extend_from_slice(f);
            }
            if speech {
                if self.speech_frames == 0 {
                    self.start = self.pos - FRAME;
                }
                self.speech_frames += 1;
                self.quiet_frames = 0;
                self.last_speech = self.pos;
            } else if self.speech_frames > 0 {
                self.quiet_frames += 1;
            }
            let long_pause = self.quiet_frames >= 23;
            let too_long = self.buf.len() >= RATE * 10;
            if self.speech_frames > 0 && (long_pause || too_long) {
                if self.speech_frames >= 12 {
                    let secs = |n: usize| n as f64 / RATE as f64;
                    out.push(Cut::Phrase(Phrase { audio: std::mem::take(&mut self.buf), start: secs(self.start), end: secs(self.last_speech) }));
                } else {
                    self.buf.clear();
                }
                self.speech_frames = 0;
                self.quiet_frames = 0;
            }
        }
        self.pending.drain(..i);
        if out.is_empty() { vec![Cut::Nothing] } else { out }
    }
}

/// Mono samples at `rate` to 16 kHz, linearly.
pub fn resample(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate as usize == RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = rate as f64 / RATE as f64;
    let n = (samples.len() as f64 / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let k = pos.floor() as usize;
            let t = (pos - k as f64) as f32;
            let a = samples[k.min(samples.len() - 1)];
            let b = samples[(k + 1).min(samples.len() - 1)];
            a + (b - a) * t
        })
        .collect()
}

pub mod coach;

#[cfg(feature = "engine")]
mod engine;
#[cfg(feature = "engine")]
pub use engine::{Captioner, Event};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cutter_finds_a_phrase_between_pauses() {
        let mut c = Cutter::default();
        let quiet = vec![0.0005f32; RATE];
        let tone: Vec<f32> = (0..RATE).map(|i| (i as f32 * 0.05).sin() * 0.2).collect();
        assert_eq!(c.push(&quiet), vec![Cut::Nothing]);
        assert_eq!(c.push(&tone), vec![Cut::Nothing]);
        assert!(c.so_far().is_some());
        let cuts = c.push(&quiet);
        match &cuts[0] {
            Cut::Phrase(p) => {
                assert!(p.audio.len() >= RATE && p.audio.len() < RATE * 2, "{}", p.audio.len());
                assert!((p.start - 1.0).abs() < 0.05 && (p.end - 2.0).abs() < 0.05, "{} {}", p.start, p.end);
            }
            Cut::Nothing => panic!("no phrase"),
        }
    }

    #[test]
    fn a_click_is_not_a_phrase() {
        let mut c = Cutter::default();
        let mut s = vec![0.0005f32; RATE];
        for x in s.iter_mut().skip(1000).take(1500) {
            *x = 0.3;
        }
        let cuts = c.push(&s);
        assert_eq!(cuts, vec![Cut::Nothing]);
    }

    #[test]
    fn lines_are_cleaned_and_prompts_built() {
        assert_eq!(clean_line(" [BLANK_AUDIO] "), "");
        assert_eq!(clean_line("Thank you."), "");
        assert_eq!(clean_line("Fibroblasts (music) make  collagen."), "Fibroblasts make collagen.");
        let p = prompt_from(&["Dermis, fibroblast and extracellular matrix".into(), "Fibroblast collagen".into()]);
        assert_eq!(p, "Dermis, fibroblast, extracellular, matrix, collagen");
        assert_eq!(resample(&[0.0, 1.0, 0.0, 1.0], 32_000).len(), 2);
    }
}
