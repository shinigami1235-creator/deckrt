//! deckrt's video export: pictures and sound in, an MP4 with H.264 video and AAC sound out.
//!
//! On Windows the encoding is Media Foundation's (the encoders that come with Windows, using the
//! graphics card's encoder when there is one), so nothing extra is installed. Elsewhere
//! [`Writer::create`] says the export works on Windows.
//!
//! - [`Writer`]: frames as NV12 ([`nv12`] turns RGBA into it), sound as 16-bit mono PCM.
//! - [`blend`]: a cross-fade between two NV12 frames, for transitions.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::Path;

#[cfg(windows)]
mod mf;

/// What the video is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Even numbers.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// 1 to 100: the encoder's quality setting. Slides with text want 70 or more.
    pub quality: u32,
    /// Sound: mono 16-bit PCM at this rate (44 100 or 48 000).
    pub audio_rate: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options { width: 1920, height: 1080, fps: 10, quality: 75, audio_rate: 48_000 }
    }
}

/// Times in the video are in units of 100 ns, Media Foundation's clock.
pub const SECOND: i64 = 10_000_000;

/// An MP4 being written.
pub struct Writer {
    #[cfg(windows)]
    inner: mf::MfWriter,
    opts: Options,
}

impl Writer {
    pub fn create(path: &Path, opts: &Options) -> Result<Writer, String> {
        if opts.width < 16 || opts.height < 16 || opts.width % 2 != 0 || opts.height % 2 != 0 || opts.width > 4096 || opts.height > 4096 {
            return Err("The video size must be even, from 16 to 4096 pixels.".into());
        }
        if opts.fps == 0 || opts.fps > 60 {
            return Err("The frame rate must be 1 to 60.".into());
        }
        #[cfg(windows)]
        {
            let inner = mf::MfWriter::create(path, opts)?;
            Ok(Writer { inner, opts: opts.clone() })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err("Export to Video works on Windows.".into())
        }
    }

    pub fn options(&self) -> &Options {
        &self.opts
    }

    /// One frame (NV12, `width × height × 3 / 2` bytes) shown from `time` for `duration`.
    pub fn frame(&mut self, nv12: &[u8], time: i64, duration: i64) -> Result<(), String> {
        let want = (self.opts.width * self.opts.height * 3 / 2) as usize;
        if nv12.len() != want {
            return Err(format!("A frame must be {want} bytes, not {}.", nv12.len()));
        }
        #[cfg(windows)]
        {
            self.inner.frame(nv12, time, duration)
        }
        #[cfg(not(windows))]
        {
            let _ = (time, duration);
            Ok(())
        }
    }

    /// Sound starting at `time`.
    pub fn audio(&mut self, pcm: &[i16], time: i64) -> Result<(), String> {
        if pcm.is_empty() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            self.inner.audio(pcm, time)
        }
        #[cfg(not(windows))]
        {
            let _ = time;
            Ok(())
        }
    }

    /// Write the end of the file. Without this the MP4 can't be played.
    pub fn finish(self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.inner.finish()
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
}

/// Straight RGBA (`w × h × 4`) to NV12, BT.709, video range. `w` and `h` must be even.
pub fn nv12(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 3 / 2];
    if rgba.len() < w * h * 4 || w % 2 != 0 || h % 2 != 0 {
        return out;
    }
    let (y_plane, uv_plane) = out.split_at_mut(w * h);
    let px = |x: usize, y: usize| -> (f32, f32, f32) {
        let i = (y * w + x) * 4;
        let a = rgba[i + 3] as f32 / 255.0;
        // Over white: a transparent corner of a slide picture is the paper, not black.
        let c = |v: u8| v as f32 * a + 255.0 * (1.0 - a);
        (c(rgba[i]), c(rgba[i + 1]), c(rgba[i + 2]))
    };
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, y);
            let yy = 16.0 + (0.2126 * r + 0.7152 * g + 0.0722 * b) * 219.0 / 255.0;
            y_plane[y * w + x] = yy.round().clamp(16.0, 235.0) as u8;
        }
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut acc = (0.0, 0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (r, g, b) = px(x + dx, y + dy);
                acc.0 += r;
                acc.1 += g;
                acc.2 += b;
            }
            let (r, g, b) = (acc.0 / 4.0, acc.1 / 4.0, acc.2 / 4.0);
            let u = 128.0 + (-0.1146 * r - 0.3854 * g + 0.5 * b) * 224.0 / 255.0;
            let v = 128.0 + (0.5 * r - 0.4542 * g - 0.0458 * b) * 224.0 / 255.0;
            let i = (y / 2) * w + x;
            uv_plane[i] = u.round().clamp(16.0, 240.0) as u8;
            uv_plane[i + 1] = v.round().clamp(16.0, 240.0) as u8;
        }
    }
    out
}

/// `a` faded into `b` by `t` (0 to 1). Both frames the same size.
pub fn blend(a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
    let t = t.clamp(0.0, 1.0);
    a.iter().zip(b).map(|(x, y)| (*x as f32 + (*y as f32 - *x as f32) * t).round() as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_become_nv12() {
        let white = vec![255u8; 4 * 4 * 4];
        let f = nv12(&white, 4, 4);
        assert_eq!(f.len(), 24);
        assert!(f[..16].iter().all(|v| *v == 235));
        assert!(f[16..].iter().all(|v| *v == 128));
        let mut red = vec![0u8; 4 * 4 * 4];
        for p in red.chunks_mut(4) {
            p[0] = 255;
            p[3] = 255;
        }
        let f = nv12(&red, 4, 4);
        assert_eq!(f[0], 63, "BT.709 red's luma");
        assert_eq!(f[17], 240, "red's V");
        let clear = vec![0u8; 4 * 4 * 4];
        assert_eq!(nv12(&clear, 4, 4)[0], 235, "transparent is over white");
        assert_eq!(blend(&[0, 100], &[100, 200], 0.5), vec![50, 150]);
    }

    #[test]
    fn bad_sizes_are_refused() {
        let dir = std::env::temp_dir().join("deckrt-video-size.mp4");
        assert!(Writer::create(&dir, &Options { width: 15, ..Default::default() }).is_err());
        assert!(Writer::create(&dir, &Options { fps: 0, ..Default::default() }).is_err());
    }

    /// Windows only: a second of video and sound, then read back.
    #[cfg(windows)]
    #[test]
    fn writes_an_mp4_windows_can_play() {
        let path = std::env::temp_dir().join(format!("deckrt-video-test-{}.mp4", std::process::id()));
        let opts = Options { width: 320, height: 240, fps: 10, ..Default::default() };
        let mut w = Writer::create(&path, &opts).expect("writer");
        let mut rgba = vec![255u8; 320 * 240 * 4];
        for p in rgba.chunks_mut(4).take(320 * 120) {
            p[0] = 30;
            p[1] = 60;
            p[2] = 200;
        }
        let frame = nv12(&rgba, 320, 240);
        for i in 0..10 {
            w.frame(&frame, i * SECOND / 10, SECOND / 10).expect("frame");
        }
        let tone: Vec<i16> = (0..48_000).map(|i| ((i as f32 * 0.06).sin() * 6000.0) as i16).collect();
        w.audio(&tone, 0).expect("audio");
        w.finish().expect("finish");
        let bytes = std::sync::Arc::new(std::fs::read(&path).expect("read"));
        let info = deckcraft_media::probe(&bytes).expect("probe");
        let v = info.video.expect("a video track");
        assert_eq!((v.width, v.height), (320, 240));
        assert!(v.codec.to_lowercase().contains("264") || v.codec.to_lowercase().contains("avc"), "{}", v.codec);
        let a = info.audio.expect("a sound track");
        assert!(a.codec.to_lowercase().contains("aac"), "{}", a.codec);
        assert!((800..=1300).contains(&info.duration_ms), "{}", info.duration_ms);
        let _ = std::fs::remove_file(&path);
    }
}
