//! Recording the presenter's voice for narration: the microphone into memory, mono, at
//! [`Recorder::RATE`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use super::engine::spawn_mic;

/// The microphone while it records.
pub struct Recorder {
    audio: Receiver<Vec<f32>>,
    name: Receiver<Result<String, String>>,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    samples: Vec<f32>,
    paused: bool,
    /// The loudest sample of the last chunk, ×1000, for the level meter.
    level: Arc<AtomicU32>,
    /// The microphone's name once open, or why it couldn't be.
    pub status: Option<Result<String, String>>,
}

impl Recorder {
    /// Speech rate: clear voice at about 2.6 MB a minute.
    pub const RATE: u32 = 22_050;
    /// Two hours, so a forgotten recording can't fill the memory.
    const MAX: usize = Self::RATE as usize * 60 * 120;

    pub fn start() -> Recorder {
        let error = Arc::new(Mutex::new(None));
        let e = error.clone();
        let (audio, name, stop) = match std::env::var_os("DECKRT_RECORD_TEST") {
            Some(p) => play_file(std::path::PathBuf::from(p)),
            None => spawn_mic(Self::RATE, move |msg| {
                if let Ok(mut g) = e.lock() {
                    *g = Some(msg);
                }
            }),
        };
        Recorder { audio, name, stop, error, samples: vec![], paused: false, level: Arc::new(AtomicU32::new(0)), status: None }
    }

    /// Take in what the microphone sent since the last call. Call it every frame.
    pub fn poll(&mut self) {
        if self.status.is_none()
            && let Ok(s) = self.name.try_recv()
        {
            self.status = Some(s);
        }
        if let Ok(mut g) = self.error.lock()
            && let Some(e) = g.take()
        {
            self.status = Some(Err(e));
        }
        loop {
            match self.audio.try_recv() {
                Ok(chunk) => {
                    let peak = chunk.iter().fold(0f32, |m, s| m.max(s.abs()));
                    self.level.store((peak.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                    if !self.paused && self.samples.len() < Self::MAX {
                        let room = Self::MAX - self.samples.len();
                        self.samples.extend_from_slice(&chunk[..chunk.len().min(room)]);
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    /// Samples recorded so far: the recording's clock.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn secs(&self) -> f64 {
        self.samples.len() as f64 / Self::RATE as f64
    }

    /// 0 to 1.
    pub fn level(&self) -> f32 {
        self.level.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// While paused the microphone stays open and what it hears is let go.
    pub fn set_paused(&mut self, p: bool) {
        self.paused = p;
    }

    /// The samples from `from` to `to`.
    pub fn slice(&self, from: usize, to: usize) -> &[f32] {
        let to = to.min(self.samples.len());
        self.samples.get(from.min(to)..to).unwrap_or(&[])
    }
}

/// `DECKRT_RECORD_TEST`: a 16-bit PCM WAV file played in as if it were the microphone, at
/// its own pace, for testing without one.
fn play_file(path: std::path::PathBuf) -> (Receiver<Vec<f32>>, Receiver<Result<String, String>>, Arc<AtomicBool>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (name_tx, name_rx) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let _ = std::thread::Builder::new().name("deckrt-mic-test".into()).spawn(move || {
        let Some((rate, samples)) = std::fs::read(&path).ok().and_then(|b| read_wav(&b)) else {
            let _ = name_tx.send(Err(format!("{} isn't a 16-bit WAV file", path.display())));
            return;
        };
        let _ = name_tx.send(Ok("test file".into()));
        let audio = super::resample_to(&samples, rate, Recorder::RATE);
        let chunk = Recorder::RATE as usize / 10;
        let mut i = 0;
        while !s.load(Ordering::Relaxed) {
            let end = (i + chunk).min(audio.len());
            // Silence once the file runs out, as a quiet room would give.
            let part = if i < audio.len() { audio[i..end].to_vec() } else { vec![0.0; chunk] };
            i = end.max(i + chunk);
            if tx.send(part).is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    (rx, name_rx, stop)
}

/// The rate and mono samples of a 16-bit PCM WAV.
pub fn read_wav(b: &[u8]) -> Option<(u32, Vec<f32>)> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = b.get(i + 8..(i + 8).saturating_add(len).min(b.len()))?;
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            let ch = channels as usize;
            let s: Vec<f32> = body
                .chunks_exact(2 * ch)
                .map(|f| f.chunks_exact(2).map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0).sum::<f32>() / ch as f32)
                .collect();
            return Some((rate, s));
        }
        i += 8 + len + (len & 1);
    }
    None
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
