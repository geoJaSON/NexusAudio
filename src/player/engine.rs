//! Audio engine: the proven spike (symphonia decode + accurate seek + cpal
//! output) promoted into a controllable, stateful player.
//!
//! Topology: the App sends `Cmd`s over a channel. A dedicated audio thread owns
//! the symphonia reader/decoder, a proper `rubato` sinc resampler, and the cpal
//! output stream. Decoded → resampled → interleaved samples go into a ring
//! buffer the cpal callback drains (applying volume). Shared atomics expose
//! position/duration/state back to the UI without locking on the hot path.

use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // bitrate_kbps/bits exposed for future status/details use
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub bitrate_kbps: Option<u32>,
    pub bits: Option<u8>,
}

#[derive(Debug)]
enum Cmd {
    Load {
        path: PathBuf,
        start_secs: f64,
        /// Authoritative duration override (audiobooks: mvhd vs HE-AAC half).
        duration: Option<f64>,
    },
    Play,
    #[allow(dead_code)] // explicit pause() API; UI currently uses TogglePause
    Pause,
    TogglePause,
    Stop,
    Seek(f64),
    SeekRel(f64),
    /// Pre-open the next file so when the current track hits EOF we can swap
    /// in zero-gap. `None` clears any pending preload (e.g. end of queue).
    PreloadNext(Option<PathBuf>),
    Shutdown,
}

struct Shared {
    playing: AtomicBool,
    has_track: AtomicBool,
    /// Set true when a track reaches its end naturally AND there was no
    /// gapless preload to swap in — the App polls this to advance the queue
    /// the old way (load+play next). Cleared via [`take_ended`].
    ended: AtomicBool,
    /// Set true when the engine has already swapped to a preloaded next track
    /// at the EOF boundary (gapless). The App polls this to advance the queue
    /// cursor / session history WITHOUT calling load() again. Cleared via
    /// [`take_advanced`].
    advanced: AtomicBool,
    position_ms: AtomicU64,
    duration_ms: AtomicU64,
    /// >0 overrides the decoded duration (audiobook authoritative duration).
    duration_override_ms: AtomicU64,
    volume: AtomicU32, // f32 bits, 0.0..=1.0
    info: Mutex<AudioInfo>,
}

pub struct Engine {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
}

impl Engine {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            has_track: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            advanced: AtomicBool::new(false),
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            duration_override_ms: AtomicU64::new(0),
            volume: AtomicU32::new(0.7f32.to_bits()),
            info: Mutex::new(AudioInfo::default()),
        });
        let (tx, rx) = mpsc::channel();
        {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("nexus-audio".into())
                .spawn(move || audio_thread(rx, shared))
                .expect("spawn audio thread");
        }
        Self { tx, shared }
    }

    pub fn load(&self, path: PathBuf, start_secs: f64) {
        let _ = self.tx.send(Cmd::Load { path, start_secs, duration: None });
    }
    /// Load an audiobook with an authoritative duration (codec-independent),
    /// resuming at `start_secs`.
    pub fn load_book(&self, path: PathBuf, start_secs: f64, duration: f64) {
        let _ = self.tx.send(Cmd::Load {
            path,
            start_secs,
            duration: Some(duration),
        });
    }
    pub fn play(&self) {
        let _ = self.tx.send(Cmd::Play);
    }
    #[allow(dead_code)] // public engine API; not yet called from the UI
    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn toggle_pause(&self) {
        let _ = self.tx.send(Cmd::TogglePause);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs));
    }
    pub fn seek_rel(&self, delta: f64) {
        let _ = self.tx.send(Cmd::SeekRel(delta));
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
    pub fn has_track(&self) -> bool {
        self.shared.has_track.load(Ordering::Relaxed)
    }
    pub fn position_secs(&self) -> f64 {
        self.shared.position_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }
    pub fn duration_secs(&self) -> f64 {
        let ov = self.shared.duration_override_ms.load(Ordering::Relaxed);
        let ms = if ov > 0 {
            ov
        } else {
            self.shared.duration_ms.load(Ordering::Relaxed)
        };
        ms as f64 / 1000.0
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }
    pub fn set_volume(&self, v: f32) {
        self.shared
            .volume
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn add_volume(&self, delta: f32) {
        self.set_volume(self.volume() + delta);
    }
    pub fn info(&self) -> AudioInfo {
        self.shared.info.lock().unwrap().clone()
    }
    /// Consume the natural-end flag; returns true exactly once per track end.
    pub fn take_ended(&self) -> bool {
        self.shared.ended.swap(false, Ordering::Relaxed)
    }
    /// Consume the gapless-advanced flag — returns true exactly once per
    /// gapless transition. The engine has already swapped in the next track;
    /// the App still needs to bump the queue cursor / session history.
    pub fn take_advanced(&self) -> bool {
        self.shared.advanced.swap(false, Ordering::Relaxed)
    }
    /// Pre-open the next file so the EOF boundary is gapless. Pass `None` to
    /// drop any pending preload (e.g. end of queue, repeat=None).
    pub fn preload_next(&self, path: Option<PathBuf>) {
        let _ = self.tx.send(Cmd::PreloadNext(path));
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

// ───────────────────────── audio thread ──────────────────────────

type Ring = Arc<Mutex<VecDeque<f32>>>;

/// Everything tied to the currently-loaded track.
struct Track {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    src_rate: u32,
    src_ch: usize,
    sample_buf: Option<SampleBuffer<f32>>,
    resampler: Option<Resamp>,
    /// Where this playback segment started (seek/load offset), in seconds.
    base_secs: f64,
    info: AudioInfo,
}

fn audio_thread(rx: Receiver<Cmd>, shared: Arc<Shared>) {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("engine: no output device");
        return;
    };
    let Ok(supported) = device.default_output_config() else {
        eprintln!("engine: no default output config");
        return;
    };
    let dev_rate = supported.sample_rate().0;
    let dev_ch = supported.channels() as usize;
    let cfg: cpal::StreamConfig = supported.config();

    let ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
    let played_frames = Arc::new(AtomicU64::new(0));

    let stream = {
        let ring = ring.clone();
        let shared = shared.clone();
        let played = played_frames.clone();
        device.build_output_stream(
            &cfg,
            move |out: &mut [f32], _| {
                let playing = shared.playing.load(Ordering::Relaxed);
                let vol = f32::from_bits(shared.volume.load(Ordering::Relaxed));
                if !playing {
                    out.iter_mut().for_each(|s| *s = 0.0);
                    return;
                }
                let mut buf = ring.lock().unwrap();
                let mut consumed = 0usize;
                for s in out.iter_mut() {
                    match buf.pop_front() {
                        Some(v) => {
                            *s = v * vol;
                            consumed += 1;
                        }
                        None => *s = 0.0, // underrun → silence, don't count
                    }
                }
                if consumed > 0 {
                    played.fetch_add((consumed / dev_ch.max(1)) as u64, Ordering::Relaxed);
                }
            },
            |e| eprintln!("engine cpal error: {e}"),
            None,
        )
    };
    let Ok(stream) = stream else {
        eprintln!("engine: could not build output stream");
        return;
    };
    let _ = stream.play(); // stream always running; silence when paused

    let mut track: Option<Track> = None;
    let mut next_track: Option<Track> = None;

    loop {
        // Drain all pending commands first.
        loop {
            match rx.try_recv() {
                Ok(Cmd::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return,
                Ok(cmd) => handle_cmd(
                    cmd,
                    &mut track,
                    &mut next_track,
                    &shared,
                    &ring,
                    &played_frames,
                    dev_rate,
                    dev_ch,
                ),
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        // Keep the ring topped up while playing.
        let playing = shared.playing.load(Ordering::Relaxed);
        if playing {
            let mut hit_eof = false;
            if let Some(t) = track.as_mut() {
                let want = (dev_rate as usize) * dev_ch; // ~1 s buffered
                while ring.lock().unwrap().len() < want {
                    if !decode_step(t, &ring, dev_rate, dev_ch) {
                        hit_eof = true;
                        break;
                    }
                }
                // Position = where this segment started + frames the device ate.
                // After a gapless swap, base_secs is set to a small negative
                // (the ring still holds the old track's tail), so position
                // clamps to 0 until the new track's audio actually starts.
                let pf = played_frames.load(Ordering::Relaxed);
                let pos = (t.base_secs + pf as f64 / dev_rate as f64).max(0.0);
                shared
                    .position_ms
                    .store((pos * 1000.0) as u64, Ordering::Relaxed);
            }
            if hit_eof {
                if next_track.is_some() {
                    gapless_swap(
                        &mut track,
                        &mut next_track,
                        &ring,
                        &played_frames,
                        &shared,
                        dev_rate,
                        dev_ch,
                    );
                } else {
                    shared.playing.store(false, Ordering::Relaxed);
                    shared.ended.store(true, Ordering::Relaxed);
                }
            }
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Promote `next_track` into `track` without touching the ring, so the cpal
/// callback never sees silence. The new track's `base_secs` is set to a small
/// negative value matching the buffered audio still in the ring, so the
/// `position_ms` calculation rests at 0 until the swap is audible.
fn gapless_swap(
    track: &mut Option<Track>,
    next_track: &mut Option<Track>,
    ring: &Ring,
    played: &Arc<AtomicU64>,
    shared: &Arc<Shared>,
    dev_rate: u32,
    dev_ch: usize,
) {
    let Some(mut new_t) = next_track.take() else {
        return;
    };
    // Frames still queued from the OLD track — the cpal callback will drain
    // those before any sample from the new track plays. Offset base_secs by
    // that lead time so `pos` reads 0 once we DO start hearing the new track.
    let ring_frames = ring.lock().unwrap().len() / dev_ch.max(1);
    let lead_secs = ring_frames as f64 / dev_rate as f64;
    new_t.base_secs = -lead_secs;
    // Reset the played counter so it represents frames since the swap point.
    played.store(0, Ordering::Relaxed);
    // Publish the new track's metadata + duration (the override is cleared —
    // gapless is music-only, audiobooks never preload).
    let dur = new_t
        .format
        .tracks()
        .iter()
        .find(|x| x.id == new_t.track_id)
        .and_then(|x| {
            x.codec_params
                .n_frames
                .zip(x.codec_params.sample_rate)
                .map(|(n, sr)| n as f64 / sr as f64)
        })
        .unwrap_or(0.0);
    shared
        .duration_ms
        .store((dur * 1000.0) as u64, Ordering::Relaxed);
    shared.duration_override_ms.store(0, Ordering::Relaxed);
    *shared.info.lock().unwrap() = new_t.info.clone();
    *track = Some(new_t);
    shared.advanced.store(true, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
fn handle_cmd(
    cmd: Cmd,
    track: &mut Option<Track>,
    next_track: &mut Option<Track>,
    shared: &Arc<Shared>,
    ring: &Ring,
    played: &Arc<AtomicU64>,
    dev_rate: u32,
    dev_ch: usize,
) {
    match cmd {
        Cmd::Load { path, start_secs, duration } => {
            shared.playing.store(false, Ordering::Relaxed);
            ring.lock().unwrap().clear();
            played.store(0, Ordering::Relaxed);
            // A fresh manual load invalidates any preloaded next — the App
            // re-sends `PreloadNext` afterward if appropriate.
            *next_track = None;
            match open_track(&path, dev_rate, dev_ch) {
                Ok(mut t) => {
                    let dur = t
                        .format
                        .tracks()
                        .iter()
                        .find(|x| x.id == t.track_id)
                        .and_then(|x| {
                            x.codec_params
                                .n_frames
                                .zip(x.codec_params.sample_rate)
                                .map(|(n, sr)| n as f64 / sr as f64)
                        })
                        .unwrap_or(0.0);
                    shared.duration_ms.store((dur * 1000.0) as u64, Ordering::Relaxed);
                    shared.duration_override_ms.store(
                        duration.map(|d| (d * 1000.0) as u64).unwrap_or(0),
                        Ordering::Relaxed,
                    );
                    if start_secs > 0.0 {
                        seek_track(&mut t, start_secs);
                    }
                    t.base_secs = start_secs.max(0.0);
                    shared
                        .position_ms
                        .store((t.base_secs * 1000.0) as u64, Ordering::Relaxed);
                    shared.has_track.store(true, Ordering::Relaxed);
                    shared.ended.store(false, Ordering::Relaxed);
                    *shared.info.lock().unwrap() = t.info.clone();
                    *track = Some(t);
                    shared.playing.store(true, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("engine: load failed: {e}");
                    shared.has_track.store(false, Ordering::Relaxed);
                    *track = None;
                }
            }
        }
        Cmd::Play => {
            if track.is_some() {
                shared.playing.store(true, Ordering::Relaxed);
            }
        }
        Cmd::Pause => shared.playing.store(false, Ordering::Relaxed),
        Cmd::TogglePause => {
            let p = shared.playing.load(Ordering::Relaxed);
            if track.is_some() {
                shared.playing.store(!p, Ordering::Relaxed);
            }
        }
        Cmd::Stop => {
            shared.playing.store(false, Ordering::Relaxed);
            shared.has_track.store(false, Ordering::Relaxed);
            ring.lock().unwrap().clear();
            played.store(0, Ordering::Relaxed);
            shared.position_ms.store(0, Ordering::Relaxed);
            *track = None;
            *next_track = None;
        }
        Cmd::Seek(secs) => seek_to(track, shared, ring, played, secs),
        Cmd::SeekRel(delta) => {
            let cur = shared.position_ms.load(Ordering::Relaxed) as f64 / 1000.0;
            seek_to(track, shared, ring, played, (cur + delta).max(0.0));
        }
        Cmd::PreloadNext(path) => match path {
            // Open the file eagerly so the EOF swap is a cheap pointer move.
            // Errors swallowed: if we can't preload, EOF falls back to ended.
            Some(p) => match open_track(&p, dev_rate, dev_ch) {
                Ok(t) => *next_track = Some(t),
                Err(e) => {
                    eprintln!("engine: preload failed: {e}");
                    *next_track = None;
                }
            },
            None => *next_track = None,
        },
        Cmd::Shutdown => {}
    }
}

fn seek_to(
    track: &mut Option<Track>,
    shared: &Arc<Shared>,
    ring: &Ring,
    played: &Arc<AtomicU64>,
    secs: f64,
) {
    if let Some(t) = track.as_mut() {
        seek_track(t, secs);
        t.base_secs = secs.max(0.0);
        ring.lock().unwrap().clear();
        played.store(0, Ordering::Relaxed);
        shared
            .position_ms
            .store((secs.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
    }
}

fn open_track(path: &PathBuf, dev_rate: u32, _dev_ch: usize) -> anyhow::Result<Track> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions { enable_gapless: true, ..Default::default() },
        &MetadataOptions::default(),
    )?;
    let format = probed.format;
    let st = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow::anyhow!("no audio track"))?
        .clone();
    let decoder = symphonia::default::get_codecs()
        .make(&st.codec_params, &DecoderOptions::default())?;
    let src_rate = st.codec_params.sample_rate.unwrap_or(dev_rate);
    let info = AudioInfo {
        codec: symphonia::default::get_codecs()
            .get_codec(st.codec_params.codec)
            .map(|d| d.short_name.to_uppercase())
            .unwrap_or_else(|| "AUDIO".into()),
        sample_rate: src_rate,
        bitrate_kbps: None, // symphonia doesn't expose this; UI uses the DB value
        bits: st.codec_params.bits_per_sample.map(|b| b as u8),
    };
    Ok(Track {
        format,
        decoder,
        track_id: st.id,
        src_rate,
        src_ch: st.codec_params.channels.map(|c| c.count()).unwrap_or(2).max(1),
        sample_buf: None,
        resampler: None,
        base_secs: 0.0,
        info,
    })
}

fn seek_track(t: &mut Track, secs: f64) {
    let _ = t.format.seek(
        SeekMode::Accurate,
        SeekTo::Time { time: Time::from(secs), track_id: Some(t.track_id) },
    );
    t.decoder.reset();
    if let Some(rs) = t.resampler.as_mut() {
        rs.reset();
    }
}

/// Decode one packet, resample to the device rate, push interleaved stereo into
/// the ring. Returns false at end of stream.
fn decode_step(t: &mut Track, ring: &Ring, dev_rate: u32, dev_ch: usize) -> bool {
    let packet = loop {
        match t.format.next_packet() {
            Ok(p) if p.track_id() == t.track_id => break p,
            Ok(_) => continue,
            Err(_) => return false,
        }
    };
    let decoded = match t.decoder.decode(&packet) {
        Ok(d) => d,
        Err(symphonia::core::errors::Error::DecodeError(_)) => return true, // skip glitch
        Err(_) => return false,
    };

    if t.sample_buf.is_none() {
        let spec = *decoded.spec();
        t.src_rate = spec.rate;
        t.src_ch = spec.channels.count().max(1);
        t.sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if t.src_rate != dev_rate {
            t.resampler = Resamp::new(t.src_rate, dev_rate).ok();
        }
    }
    let sb = t.sample_buf.as_mut().unwrap();
    sb.copy_interleaved_ref(decoded);

    // Normalize to stereo planar.
    let ch = t.src_ch;
    let samples = sb.samples();
    let frames = samples.len() / ch.max(1);
    let mut l = Vec::with_capacity(frames);
    let mut r = Vec::with_capacity(frames);
    for f in samples.chunks_exact(ch) {
        l.push(f[0]);
        r.push(if ch > 1 { f[1] } else { f[0] });
    }

    let mut out = ring.lock().unwrap();
    match t.resampler.as_mut() {
        Some(rs) => rs.process_into(&l, &r, dev_ch, &mut out),
        None => {
            for i in 0..frames {
                push_frame(&mut out, l[i], r[i], dev_ch);
            }
        }
    }
    true
}

fn push_frame(out: &mut VecDeque<f32>, l: f32, r: f32, dev_ch: usize) {
    match dev_ch {
        1 => out.push_back(0.5 * (l + r)),
        _ => {
            out.push_back(l);
            out.push_back(r);
            for _ in 2..dev_ch {
                out.push_back(0.0);
            }
        }
    }
}

// ─────────────────────── proper sinc resampler ───────────────────

const CHUNK: usize = 1024;

struct Resamp {
    rs: SincFixedIn<f32>,
    acc_l: Vec<f32>,
    acc_r: Vec<f32>,
}

impl Resamp {
    fn new(src_rate: u32, dev_rate: u32) -> anyhow::Result<Self> {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let rs = SincFixedIn::<f32>::new(
            dev_rate as f64 / src_rate as f64,
            2.0,
            params,
            CHUNK,
            2,
        )?;
        Ok(Self { rs, acc_l: Vec::new(), acc_r: Vec::new() })
    }

    fn reset(&mut self) {
        self.acc_l.clear();
        self.acc_r.clear();
        self.rs.reset();
    }

    /// Accumulate, process whole CHUNKs, push interleaved device frames.
    fn process_into(&mut self, l: &[f32], r: &[f32], dev_ch: usize, out: &mut VecDeque<f32>) {
        self.acc_l.extend_from_slice(l);
        self.acc_r.extend_from_slice(r);
        while self.acc_l.len() >= CHUNK {
            let inl: Vec<f32> = self.acc_l.drain(..CHUNK).collect();
            let inr: Vec<f32> = self.acc_r.drain(..CHUNK).collect();
            if let Ok(res) = self.rs.process(&[inl, inr], None) {
                let (ol, or) = (&res[0], &res[1]);
                for i in 0..ol.len() {
                    push_frame(out, ol[i], or[i], dev_ch);
                }
            }
        }
    }
}
