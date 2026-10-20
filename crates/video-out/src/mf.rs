//! Media Foundation's sink writer: NV12 frames to H.264, PCM to AAC, both into an MP4.

use std::path::Path;

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::HSTRING;

use super::Options;

pub struct MfWriter {
    /// Taken out before Media Foundation shuts down.
    writer: Option<IMFSinkWriter>,
    video: u32,
    audio: u32,
    rate: u32,
    com: bool,
    finished: bool,
}

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {}", e.message())
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

impl MfWriter {
    pub fn create(path: &Path, o: &Options) -> Result<MfWriter, String> {
        // SAFETY: plain Media Foundation calls on this thread. Every interface is a windows-rs
        // smart pointer that releases itself; the buffers are locked and unlocked in pairs.
        unsafe {
            // S_FALSE (already started on this thread) is fine; a different mode is not.
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
                .map_err(err("Media Foundation didn't start (Windows N editions need the Media Feature Pack)"))?;
            let out = (|| -> windows::core::Result<(IMFSinkWriter, u32, u32)> {
                let mut attrs: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut attrs, 3)?;
                let attrs = attrs.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
                attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
                attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
                attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
                let url = HSTRING::from(path);
                let writer = MFCreateSinkWriterFromURL(&url, None, &attrs)?;

                // Video: H.264 out, NV12 in.
                let vout = MFCreateMediaType()?;
                vout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                vout.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
                // A ceiling for the quality setting below; slides need far less most of the time.
                vout.SetUINT32(&MF_MT_AVG_BITRATE, (o.width * o.height * 4).max(1_000_000))?;
                vout.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                vout.SetUINT64(&MF_MT_FRAME_SIZE, pack(o.width, o.height))?;
                vout.SetUINT64(&MF_MT_FRAME_RATE, pack(o.fps, 1))?;
                vout.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                vout.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
                let video = writer.AddStream(&vout)?;
                let vin = MFCreateMediaType()?;
                vin.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                vin.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
                vin.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                vin.SetUINT64(&MF_MT_FRAME_SIZE, pack(o.width, o.height))?;
                vin.SetUINT64(&MF_MT_FRAME_RATE, pack(o.fps, 1))?;
                vin.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                vin.SetUINT32(&MF_MT_DEFAULT_STRIDE, o.width)?;
                vin.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
                // Quality, not a fixed bitrate: a still slide costs almost nothing after its
                // first frame, and text stays sharp.
                let mut enc: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut enc, 2)?;
                let enc = enc.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
                enc.SetUINT32(&CODECAPI_AVEncCommonRateControlMode, eAVEncCommonRateControlMode_Quality.0 as u32)?;
                enc.SetUINT32(&CODECAPI_AVEncCommonQuality, o.quality.clamp(1, 100))?;
                if writer.SetInputMediaType(video, &vin, &enc).is_err() {
                    // Some encoders take no settings this way; their defaults will do.
                    writer.SetInputMediaType(video, &vin, None)?;
                }

                // Sound: AAC out, 16-bit mono PCM in.
                let aout = MFCreateMediaType()?;
                aout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                aout.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
                aout.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                aout.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, o.audio_rate)?;
                aout.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)?;
                // 96 kbit/s, one of the rates Windows' AAC encoder takes.
                aout.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 12_000)?;
                let audio = writer.AddStream(&aout)?;
                let ain = MFCreateMediaType()?;
                ain.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                ain.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                ain.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                ain.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, o.audio_rate)?;
                ain.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)?;
                ain.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2)?;
                ain.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, o.audio_rate * 2)?;
                writer.SetInputMediaType(audio, &ain, None)?;

                writer.BeginWriting()?;
                Ok((writer, video, audio))
            })();
            match out {
                Ok((writer, video, audio)) => Ok(MfWriter { writer: Some(writer), video, audio, rate: o.audio_rate.max(1), com, finished: false }),
                Err(e) => {
                    let _ = MFShutdown();
                    if com {
                        CoUninitialize();
                    }
                    Err(format!("The video couldn't be started: {}", e.message()))
                }
            }
        }
    }

    fn sample(&self, stream: u32, data: &[u8], time: i64, duration: i64) -> windows::core::Result<()> {
        let len = u32::try_from(data.len()).map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
        // SAFETY: the buffer is `len` bytes; it is written only between Lock and Unlock.
        unsafe {
            let buf = MFCreateMemoryBuffer(len)?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None)?;
            if ptr.is_null() {
                let _ = buf.Unlock();
                return Err(windows::core::Error::from(windows::Win32::Foundation::E_POINTER));
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            buf.Unlock()?;
            buf.SetCurrentLength(len)?;
            let s = MFCreateSample()?;
            s.AddBuffer(&buf)?;
            s.SetSampleTime(time)?;
            s.SetSampleDuration(duration)?;
            match &self.writer {
                Some(w) => w.WriteSample(stream, &s),
                None => Err(windows::core::Error::from(windows::Win32::Foundation::E_POINTER)),
            }
        }
    }

    pub fn frame(&mut self, nv12: &[u8], time: i64, duration: i64) -> Result<(), String> {
        self.sample(self.video, nv12, time, duration).map_err(err("A frame couldn't be written"))
    }

    pub fn audio(&mut self, pcm: &[i16], time: i64) -> Result<(), String> {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let duration = pcm.len() as i64 * super::SECOND / self.rate as i64;
        self.sample(self.audio, &bytes, time, duration).map_err(err("The sound couldn't be written"))
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.finished = true;
        // SAFETY: Finalize once; Drop then shuts Media Foundation down.
        let r = match &self.writer {
            Some(w) => unsafe { w.Finalize() },
            None => Ok(()),
        };
        r.map_err(err("The video couldn't be finished"))
    }
}

impl Drop for MfWriter {
    fn drop(&mut self) {
        // SAFETY: pairs with MFStartup and CoInitializeEx in `create`; the writer is released
        // before Media Foundation shuts down.
        unsafe {
            if let Some(w) = self.writer.take() {
                if !self.finished {
                    let _ = w.Finalize();
                }
                drop(w);
            }
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}
