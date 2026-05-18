//! PHASE 1 PLAYBACK SPIKE — the engine-selection gate.
//!
//! The single hard requirement of NEXUS//AUDIO is: open a long audiobook
//! (M4B/AAC, possibly many hours), jump to an arbitrary saved offset, and have
//! playback continue *correctly* from there. Interactive music seeking does not
//! matter; this does. If symphonia + cpal passes this test, it is the engine.
//!
//! Usage:
//!     cargo run --bin spike -- "C:\path\to\book.m4b" 15123
//!     cargo run --bin spike -- "C:\path\to\vbr.mp3"  600
//!
//! Arg 1: audio file path. Arg 2: seek target in seconds (default 60).
//! Plays ~12 s starting at the seek point, then reports what it actually did.
//!
//! The output stage queries the device's *own* preferred config and resamples
//! the decoded stream to it (linear, streaming). cpal won't force an arbitrary
//! sample rate onto a device — the real engine needs this resampler too, so
//! the spike models it rather than faking it.

use std::collections::VecDeque;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// Decoded audio normalized to interleaved stereo f32 at the *source* rate.
/// The cpal callback resamples this to the device rate on the fly.
type Ring = Arc<Mutex<VecDeque<[f32; 2]>>>;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or_else(|| anyhow!("usage: spike <audio-file> [seek-secs]"))?;
    let seek_secs: f64 = args.next().map(|s| s.parse().unwrap_or(60.0)).unwrap_or(60.0);

    println!("── NEXUS//AUDIO PLAYBACK SPIKE ───────────────────────────────");
    println!("file       : {path}");
    println!("seek target: {seek_secs:.3} s");

    // ---- symphonia: probe + decoder ----------------------------------------
    let file = File::open(&path).with_context(|| format!("open {path}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(&path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions { enable_gapless: true, ..Default::default() },
            &MetadataOptions::default(),
        )
        .context("symphonia could not probe/identify this file")?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no decodable audio track"))?
        .clone();
    let track_id = track.id;

    let codec_name = symphonia::default::get_codecs()
        .get_codec(track.codec_params.codec)
        .map(|d| d.short_name)
        .unwrap_or("?");
    let tb = track.codec_params.time_base;
    let total = track
        .codec_params
        .n_frames
        .zip(track.codec_params.sample_rate)
        .map(|(n, sr)| n as f64 / sr as f64);
    println!(
        "codec      : {codec_name}  | sample_rate(hdr): {:?}  | duration(hdr): {}",
        track.codec_params.sample_rate,
        total.map(|s| format!("{s:.1} s")).unwrap_or_else(|| "unknown".into())
    );

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("no decoder for this codec")?;

    // ---- the actual test: seek to an arbitrary offset ----------------------
    let seeked = format
        .seek(
            SeekMode::Accurate,
            SeekTo::Time { time: Time::from(seek_secs), track_id: Some(track_id) },
        )
        .context("FORMAT SEEK FAILED — this engine cannot satisfy audiobook resume")?;
    decoder.reset();
    let landed_secs = tb
        .map(|tb| {
            let t = tb.calc_time(seeked.actual_ts);
            t.seconds as f64 + t.frac
        })
        .unwrap_or(f64::NAN);
    println!("seek landed: {landed_secs:.3} s (requested {seek_secs:.3} s)");

    // ---- decode helpers ----------------------------------------------------
    let ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
    let mut spec: Option<SignalSpec> = None;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut decoded_frames: u64 = 0;

    // Push one decoded buffer into the ring, normalized to stereo.
    let mut push = |sb: &SampleBuffer<f32>, ch: usize, ring: &Ring, frames: &mut u64| {
        let s = sb.samples();
        let mut r = ring.lock().unwrap();
        match ch {
            1 => {
                for &m in s {
                    r.push_back([m, m]);
                }
            }
            _ => {
                for f in s.chunks_exact(ch) {
                    r.push_back([f[0], f[1]]);
                }
            }
        }
        *frames += (s.len() / ch.max(1)) as u64;
    };

    // Pre-roll ~1 s so the output stream never starts starved.
    let preroll_frames = 48_000;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                if spec.is_none() {
                    let s = *audio.spec();
                    spec = Some(s);
                    sample_buf = Some(SampleBuffer::<f32>::new(audio.capacity() as u64, s));
                }
                let sb = sample_buf.as_mut().unwrap();
                sb.copy_interleaved_ref(audio);
                let ch = spec.unwrap().channels.count().max(1);
                push(sb, ch, &ring, &mut decoded_frames);
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // recoverable
            Err(e) => return Err(anyhow!("decode error after seek: {e}")),
        }
        if ring.lock().unwrap().len() >= preroll_frames {
            break;
        }
    }
    let spec = spec.ok_or_else(|| {
        anyhow!("FAIL: decoded zero audio after seek — engine unfit for resume")
    })?;
    let src_rate = spec.rate as f64;
    let src_ch = spec.channels.count().max(1);
    println!(
        "decoded    : {} Hz, {} ch  | pre-rolled {} frames",
        spec.rate,
        src_ch,
        ring.lock().unwrap().len()
    );

    // ---- cpal output: use the DEVICE's own preferred config ----------------
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default audio output device"))?;
    let supported = device
        .default_output_config()
        .context("query device default output config")?;
    println!(
        "device     : {} @ {} Hz, {} ch, {:?}",
        device.name().unwrap_or_else(|_| "?".into()),
        supported.sample_rate().0,
        supported.channels(),
        supported.sample_format()
    );

    let dev_rate = supported.sample_rate().0 as f64;
    let dev_ch = supported.channels() as usize;
    let cfg: cpal::StreamConfig = supported.config();
    let fmt = supported.sample_format();

    // Streaming linear resampler, source-rate stereo -> device-rate N-channel.
    let step = src_rate / dev_rate;
    let ring_cb = ring.clone();
    let mut frac = 0.0f64;
    let mut cur = [0.0f32; 2];
    let mut nxt = [0.0f32; 2];
    let mut next_frame = move || -> [f32; 2] {
        ring_cb.lock().unwrap().pop_front().unwrap_or([0.0, 0.0])
    };
    nxt = next_frame();
    let mut fill = move |out: &mut [f32]| {
        for frame in out.chunks_mut(dev_ch) {
            let l = cur[0] + (nxt[0] - cur[0]) * frac as f32;
            let r = cur[1] + (nxt[1] - cur[1]) * frac as f32;
            match dev_ch {
                1 => frame[0] = 0.5 * (l + r),
                _ => {
                    frame[0] = l;
                    frame[1] = r;
                    for c in frame.iter_mut().skip(2) {
                        *c = 0.0;
                    }
                }
            }
            frac += step;
            while frac >= 1.0 {
                cur = nxt;
                nxt = next_frame();
                frac -= 1.0;
            }
        }
    };

    let err_fn = |e| eprintln!("cpal stream error: {e}");
    let stream = match fmt {
        SampleFormat::F32 => build::<f32>(&device, &cfg, err_fn, move |o| {
            let mut tmp = vec![0.0f32; o.len()];
            fill(&mut tmp);
            for (d, s) in o.iter_mut().zip(tmp) {
                *d = s;
            }
        })?,
        SampleFormat::I16 => build::<i16>(&device, &cfg, err_fn, move |o: &mut [i16]| {
            let mut tmp = vec![0.0f32; o.len()];
            fill(&mut tmp);
            for (d, s) in o.iter_mut().zip(tmp) {
                *d = i16::from_sample(s);
            }
        })?,
        SampleFormat::U16 => build::<u16>(&device, &cfg, err_fn, move |o: &mut [u16]| {
            let mut tmp = vec![0.0f32; o.len()];
            fill(&mut tmp);
            for (d, s) in o.iter_mut().zip(tmp) {
                *d = u16::from_sample(s);
            }
        })?,
        other => return Err(anyhow!("unsupported device sample format: {other:?}")),
    };
    stream.play()?;

    // ---- keep decoding while we audibly verify ~12 s -----------------------
    let play_for = Duration::from_secs(12);
    let start = Instant::now();
    println!("playing    : ~12 s from the seek point — LISTEN: is this coherent audio?");
    while start.elapsed() < play_for {
        if ring.lock().unwrap().len() < 48_000 {
            match format.next_packet() {
                Ok(packet) if packet.track_id() == track_id => {
                    if let Ok(audio) = decoder.decode(&packet) {
                        let sb = sample_buf.as_mut().unwrap();
                        sb.copy_interleaved_ref(audio);
                        push(sb, src_ch, &ring, &mut decoded_frames);
                    }
                }
                Ok(_) => {}
                Err(_) => break, // EOF
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    println!("──────────────────────────────────────────────────────────────");
    println!(
        "RESULT: decoded {:.1} s of audio starting at {landed_secs:.3} s.",
        decoded_frames as f64 / src_rate
    );
    println!(
        "VERDICT: PASS if (a) seek landed within ~1 s of target and (b) the\n\
         12 s you just heard was coherent speech/music, not noise/silence.\n\
         If PASS on a long .m4b → lock symphonia+cpal as the engine."
    );
    Ok(())
}

fn build<T>(
    device: &cpal::Device,
    cfg: &cpal::StreamConfig,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    mut fill: impl FnMut(&mut [T]) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    Ok(device.build_output_stream(cfg, move |o: &mut [T], _| fill(o), err_fn, None)?)
}
