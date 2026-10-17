//! The microphone and the recogniser, each on its own thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{Cut, Cutter, RATE, Settings, clean_line, resample};

/// What the captioner tells the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The model is loaded and the microphone is open.
    Ready { microphone: String },
    /// The phrase being spoken, recognised so far. Replaced by the next partial or line.
    Partial(String),
    /// A finished phrase.
    Line(String),
    Error(String),
}

enum Cmd {
    Prompt(String),
    Stop,
}

/// Live captions while it exists. Dropping it stops the microphone and the recogniser.
pub struct Captioner {
    cmd: Sender<Cmd>,
    events: Receiver<Event>,
    stop_audio: Arc<AtomicBool>,
}

impl Captioner {
    /// Load `model` and open the default microphone. Loading takes a few seconds on its own
    /// thread; [`Event::Ready`] or [`Event::Error`] says how it went.
    pub fn start(model: std::path::PathBuf, settings: Settings) -> Captioner {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (ev_tx, ev_rx) = channel::<Event>();
        let (audio_tx, audio_rx) = channel::<Vec<f32>>();
        let stop_audio = Arc::new(AtomicBool::new(false));

        // The microphone: a stream that must stay on the thread that made it.
        let stop = stop_audio.clone();
        let ev_mic = ev_tx.clone();
        let (mic_name_tx, mic_name_rx) = channel::<Result<String, String>>();
        let _ = std::thread::Builder::new().name("deckrt-captions-mic".into()).spawn(move || {
            let host = cpal::default_host();
            let Some(device) = host.default_input_device() else {
                let _ = mic_name_tx.send(Err("No microphone found. Plug one in or turn it on in Windows sound settings.".into()));
                return;
            };
            let name = device.name().unwrap_or_else(|_| "the microphone".into());
            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = mic_name_tx.send(Err(format!("The microphone can't be opened: {e}")));
                    return;
                }
            };
            let rate = config.sample_rate().0;
            let channels = config.channels().max(1) as usize;
            let err_tx = ev_mic.clone();
            let on_err = move |e: cpal::StreamError| {
                let _ = err_tx.send(Event::Error(format!("The microphone stopped: {e}")));
            };
            let tx = audio_tx.clone();
            let mono = move |data: &[f32]| -> Vec<f32> { data.chunks(channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect() };
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &config.clone().into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let _ = tx.send(resample(&mono(data), rate));
                    },
                    on_err,
                    None,
                ),
                cpal::SampleFormat::I16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        &config.clone().into(),
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let f: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                            let m: Vec<f32> = f.chunks(channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
                            let _ = tx.send(resample(&m, rate));
                        },
                        on_err,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        &config.clone().into(),
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let f: Vec<f32> = data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect();
                            let m: Vec<f32> = f.chunks(channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
                            let _ = tx.send(resample(&m, rate));
                        },
                        on_err,
                        None,
                    )
                }
                other => {
                    let _ = mic_name_tx.send(Err(format!("The microphone sends {other:?} samples, which deckrt can't read.")));
                    return;
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = mic_name_tx.send(Err(format!("The microphone can't be opened: {e}")));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = mic_name_tx.send(Err(format!("The microphone can't start: {e}")));
                return;
            }
            let _ = mic_name_tx.send(Ok(name));
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
            drop(stream);
        });

        // The recogniser.
        let _ = std::thread::Builder::new().name("deckrt-captions".into()).spawn(move || {
            let send = |e: Event| {
                let _ = ev_tx.send(e);
            };
            whisper_rs::install_logging_hooks();
            let path = model.to_string_lossy().to_string();
            let ctx = match WhisperContext::new_with_params(&path, WhisperContextParameters::default()) {
                Ok(c) => c,
                Err(e) => return send(Event::Error(format!("The speech model can't be loaded: {e}"))),
            };
            let mut state = match ctx.create_state() {
                Ok(s) => s,
                Err(e) => return send(Event::Error(format!("The speech model can't start: {e}"))),
            };
            let microphone = match mic_name_rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return send(Event::Error(e)),
                Err(_) => return send(Event::Error("The microphone didn't answer.".into())),
            };
            send(Event::Ready { microphone });
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(2).clamp(2, 8) as i32;
            let mut prompt = String::new();
            let mut cutter = Cutter::default();
            let mut last_partial = Instant::now();
            let lang: Option<String> = match settings.language.as_str() {
                "en" | "th" => Some(settings.language.clone()),
                _ => Some("auto".to_string()),
            };
            let recognise = |audio: &[f32], prompt: &str, state: &mut whisper_rs::WhisperState| -> Option<String> {
                // Whisper wants at least a second of sound.
                let mut pcm = audio.to_vec();
                if pcm.len() < RATE + RATE / 10 {
                    pcm.resize(RATE + RATE / 10, 0.0);
                }
                let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                p.set_n_threads(threads);
                p.set_language(lang.as_deref());
                p.set_translate(settings.english);
                p.set_no_context(true);
                p.set_print_special(false);
                p.set_print_progress(false);
                p.set_print_realtime(false);
                p.set_print_timestamps(false);
                p.set_suppress_blank(true);
                p.set_suppress_nst(true);
                if !prompt.is_empty() {
                    p.set_initial_prompt(prompt);
                }
                if let Err(e) = state.full(p, &pcm) {
                    log::warn!("deckrt captions: recognition failed: {e}");
                    return None;
                }
                let mut text = String::new();
                for seg in state.as_iter() {
                    if seg.no_speech_probability() > 0.6 {
                        continue;
                    }
                    if let Ok(t) = seg.to_str_lossy() {
                        text.push_str(&t);
                    }
                }
                let t = clean_line(&text);
                (!t.is_empty()).then_some(t)
            };
            loop {
                // Commands first.
                while let Ok(c) = cmd_rx.try_recv() {
                    match c {
                        Cmd::Prompt(p) => prompt = p,
                        Cmd::Stop => return,
                    }
                }
                let chunk = match audio_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(c) => c,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return,
                };
                // Everything waiting, so a slow phrase doesn't leave the sound piling up.
                let mut audio = chunk;
                while let Ok(more) = audio_rx.try_recv() {
                    audio.extend(more);
                }
                for cut in cutter.push(&audio) {
                    if let Cut::Phrase(p) = cut {
                        if let Some(t) = recognise(&p, &prompt, &mut state) {
                            send(Event::Line(t));
                        }
                        last_partial = Instant::now();
                    }
                }
                if last_partial.elapsed() >= Duration::from_millis(1800)
                    && let Some(so_far) = cutter.so_far()
                {
                    let so_far = so_far.to_vec();
                    if let Some(t) = recognise(&so_far, &prompt, &mut state) {
                        send(Event::Partial(t));
                    }
                    last_partial = Instant::now();
                }
            }
        });
        Captioner { cmd: cmd_tx, events: ev_rx, stop_audio }
    }

    /// Words from the current slide that help the recogniser.
    pub fn set_prompt(&self, prompt: String) {
        let _ = self.cmd.send(Cmd::Prompt(prompt));
    }

    /// Everything since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Captioner {
    fn drop(&mut self) {
        self.stop_audio.store(true, Ordering::Relaxed);
        let _ = self.cmd.send(Cmd::Stop);
    }
}
