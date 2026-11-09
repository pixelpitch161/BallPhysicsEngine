//! Sound files (W11): reading the samples and backing tracks a scene keeps ([`bpe_core::Scene::audio`])
//! into [`SampleData`] at the engine's rate, and the shared [`SampleBank`] that decodes each file
//! once. WAV, FLAC, MP3, OGG Vorbis and AIFF are read with symphonia (gapless, so an MP3's
//! encoder delay is trimmed and a song lines up); any rate is resampled to 48 kHz with rubato.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use bpe_dsp::SampleData;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The largest sound file the program reads, in bytes.
pub const MAX_FILE_BYTES: usize = 200 * 1024 * 1024;
/// The file types the program reads, for a file picker.
pub const EXTENSIONS: [&str; 7] = ["wav", "flac", "mp3", "ogg", "aif", "aiff", "wave"];
/// What the program says about a file it can't read.
pub const NOT_A_SOUND: &str = "isn't a sound file the program can read (WAV, FLAC, MP3, OGG or AIFF).";

/// What a quick look at a sound file finds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInfo {
    /// Its length in seconds, when the file says.
    pub seconds: Option<f64>,
    pub channels: usize,
    pub rate: u32,
}

/// The file's format reader and the track to play.
fn open(bytes: &Arc<[u8]>) -> Result<(Box<dyn FormatReader>, u32), String> {
    if bytes.is_empty() {
        return Err("The sound file is empty.".into());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err("The sound file is too big (over 200 MB).".into());
    }
    let source = std::io::Cursor::new(bytes.clone());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let format = FormatOptions { enable_gapless: true, ..FormatOptions::default() };
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), stream, &format, &MetadataOptions::default())
        .map_err(|_| format!("The file {NOT_A_SOUND}"))?;
    let reader = probed.format;
    let track = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "The file has no sound in it.".to_string())?;
    let id = track.id;
    Ok((reader, id))
}

/// Runs `f` on a thread of its own, so a damaged file that makes the decoder panic is only a
/// file the program can't read (and never closes the editor, whose crash hook watches its own
/// thread).
fn guarded<T: Send>(f: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    std::thread::scope(|s| match std::thread::Builder::new().name("sound file check".into()).spawn_scoped(s, f) {
        Ok(handle) => handle.join().unwrap_or_else(|_| Err(format!("The file {NOT_A_SOUND}"))),
        Err(_) => Err("Couldn't start reading the sound file.".into()),
    })
}

/// A quick look at `bytes` (no decoding): whether it is a sound file the program reads, and its
/// length, channels and rate.
pub fn probe(bytes: &Arc<[u8]>) -> Result<AudioInfo, String> {
    guarded(|| probe_here(bytes))
}

fn probe_here(bytes: &Arc<[u8]>) -> Result<AudioInfo, String> {
    let (reader, id) = open(bytes)?;
    let track = reader.tracks().iter().find(|t| t.id == id).expect("the track found");
    let p = &track.codec_params;
    let rate = p.sample_rate.ok_or_else(|| format!("The file {NOT_A_SOUND}"))?;
    let channels = p.channels.map_or(1, |c| c.count());
    let seconds = p.n_frames.map(|n| n as f64 / rate as f64);
    symphonia::default::get_codecs()
        .make(p, &DecoderOptions::default())
        .map_err(|_| format!("The file {NOT_A_SOUND}"))?;
    Ok(AudioInfo { seconds, channels, rate })
}

/// Decodes `bytes` to left and right at 48 kHz (mono on both sides; of more than two channels,
/// the first two), with the copies and peaks [`SampleData`] makes.
pub fn decode(bytes: &Arc<[u8]>) -> Result<SampleData, String> {
    guarded(|| decode_here(bytes))
}

fn decode_here(bytes: &Arc<[u8]>) -> Result<SampleData, String> {
    let (mut reader, id) = open(bytes)?;
    let params = reader.tracks().iter().find(|t| t.id == id).expect("the track found").codec_params.clone();
    let rate = params.sample_rate.ok_or_else(|| format!("The file {NOT_A_SOUND}"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|_| format!("The file {NOT_A_SOUND}"))?;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged packet is skipped, as players do.
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let need = decoded.capacity() as u64;
        if buffer.as_ref().is_none_or(|b| b.capacity() < need as usize * channels) {
            buffer = Some(SampleBuffer::new(need, spec));
        }
        let buf = buffer.as_mut().expect("made above");
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks(channels) {
            left.push(frame[0]);
            right.push(if channels > 1 { frame[1] } else { frame[0] });
        }
    }
    if left.is_empty() {
        return Err("The file has no sound in it.".into());
    }
    let (left, right) = if rate == bpe_dsp::SAMPLE_RATE as u32 {
        (left, right)
    } else {
        resample(left, right, rate as usize).map_err(|e| format!("The sound couldn't be converted: {e}"))?
    };
    Ok(SampleData::new(left, right))
}

/// `left`/`right` at `rate` Hz, at 48 kHz instead: the same length in time, lined up (the
/// resampler's delay taken back out).
fn resample(left: Vec<f32>, right: Vec<f32>, rate: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
    use rubato::{FftFixedIn, Resampler};
    let out_rate = bpe_dsp::SAMPLE_RATE as usize;
    let want = (left.len() as u64 * out_rate as u64).div_ceil(rate as u64) as usize;
    let mut r = FftFixedIn::<f32>::new(rate, out_rate, 1024, 2, 2).map_err(|e| e.to_string())?;
    let delay = r.output_delay();
    let mut out = [Vec::with_capacity(want + delay + 2048), Vec::with_capacity(want + delay + 2048)];
    let mut at = 0;
    let push = |out: &mut [Vec<f32>; 2], made: Vec<Vec<f32>>| {
        for (o, m) in out.iter_mut().zip(made) {
            o.extend_from_slice(&m);
        }
    };
    while at < left.len() {
        let need = r.input_frames_next();
        if at + need <= left.len() {
            let made = r.process(&[&left[at..at + need], &right[at..at + need]], None).map_err(|e| e.to_string())?;
            push(&mut out, made);
        } else {
            let made = r.process_partial(Some(&[&left[at..], &right[at..]]), None).map_err(|e| e.to_string())?;
            push(&mut out, made);
        }
        at += need;
    }
    while out[0].len() < want + delay {
        let made = r.process_partial::<&[f32]>(None, None).map_err(|e| e.to_string())?;
        if made[0].is_empty() {
            break;
        }
        push(&mut out, made);
    }
    let [mut l, mut rr] = out;
    for c in [&mut l, &mut rr] {
        c.drain(..delay.min(c.len()));
        c.resize(want, 0.0);
    }
    Ok((l, rr))
}

/// The note a sound file's name ends in ("Piano C5", "Pad_F#4.wav", "Bass-Eb2"), counted as the
/// editor counts them (C5 is 60).
pub fn guess_root(name: &str) -> Option<u8> {
    let stem = name.rsplit_once('.').filter(|(_, ext)| EXTENSIONS.contains(&ext.to_lowercase().as_str()));
    let stem = stem.map_or(name, |(s, _)| s);
    stem.split(|c: char| c.is_whitespace() || "_-.()[]".contains(c)).rev().find_map(note_of)
}

/// `C5`, `F#4`, `Eb2` as a note number.
fn note_of(token: &str) -> Option<u8> {
    let mut chars = token.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (shift, octave) = match rest.strip_prefix('#') {
        Some(o) => (1, o),
        None => match rest.strip_prefix('b') {
            Some(o) if !o.is_empty() => (-1, o),
            _ => (0, rest.as_str()),
        },
    };
    if octave.is_empty() || octave.len() > 2 || !octave.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n = 12 * octave.parse::<i32>().ok()? + base + shift;
    (0..=127).contains(&n).then_some(n as u8)
}

/// One decoded file, known by its bytes' allocation.
struct Entry {
    bytes: Weak<[u8]>,
    data: Result<Arc<SampleData>, String>,
}

#[derive(Default)]
struct Bank {
    entries: Vec<Entry>,
    /// The files decoding in the background.
    decoding: Vec<Weak<[u8]>>,
}

impl Bank {
    fn find(&mut self, bytes: &Arc<[u8]>) -> Option<Result<Arc<SampleData>, String>> {
        // Files nothing holds any more go (their decodes with them).
        self.entries.retain(|e| e.bytes.strong_count() > 0);
        self.entries.iter().find(|e| e.bytes.upgrade().is_some_and(|b| Arc::ptr_eq(&b, bytes))).map(|e| e.data.clone())
    }

    fn insert(&mut self, bytes: &Arc<[u8]>, data: Result<Arc<SampleData>, String>) {
        if self.find(bytes).is_none() {
            self.entries.push(Entry { bytes: Arc::downgrade(bytes), data });
        }
    }
}

static BANK: Mutex<Option<Bank>> = Mutex::new(None);
static DECODED: AtomicU64 = AtomicU64::new(0);

/// The shared decodes of the sound files in use: each file (the same bytes, however many scenes
/// and copies hold them) is decoded once.
pub struct SampleBank;

impl SampleBank {
    fn with<R>(f: impl FnOnce(&mut Bank) -> R) -> R {
        let mut lock = BANK.lock().unwrap_or_else(|p| p.into_inner());
        f(lock.get_or_insert_with(Bank::default))
    }

    /// The decode of `bytes`, decoded there and then if need be (the export and tests).
    pub fn get(bytes: &Arc<[u8]>) -> Result<Arc<SampleData>, String> {
        if let Some(d) = Self::with(|b| b.find(bytes)) {
            return d;
        }
        let data = decode(bytes).map(Arc::new);
        Self::with(|b| b.insert(bytes, data.clone()));
        data
    }

    /// The decode of `bytes` if it is done; otherwise `None`, and it starts decoding on a thread
    /// of its own ([`SampleBank::decoded`] counts up when it is in). The editor, which never waits.
    pub fn ready(bytes: &Arc<[u8]>) -> Option<Result<Arc<SampleData>, String>> {
        Self::with(|bank| {
            if let Some(d) = bank.find(bytes) {
                return Some(d);
            }
            bank.decoding.retain(|w| w.strong_count() > 0);
            let busy = bank.decoding.iter().any(|w| w.upgrade().is_some_and(|b| Arc::ptr_eq(&b, bytes)));
            if !busy {
                bank.decoding.push(Arc::downgrade(bytes));
                let mine = bytes.clone();
                let started = std::thread::Builder::new().name("sound file decode".into()).spawn(move || {
                    let data = decode(&mine).map(Arc::new);
                    Self::with(|bank| {
                        bank.insert(&mine, data);
                        // Counted before it stops being busy, so the editor never sees neither.
                        DECODED.fetch_add(1, Ordering::SeqCst);
                        bank.decoding.retain(|w| !w.upgrade().is_some_and(|b| Arc::ptr_eq(&b, &mine)));
                    });
                });
                if started.is_err() {
                    bank.decoding.retain(|w| !w.upgrade().is_some_and(|b| Arc::ptr_eq(&b, bytes)));
                }
            }
            None
        })
    }

    /// How many background decodes have finished: the editor builds the sound again when it moves.
    pub fn decoded() -> u64 {
        DECODED.load(Ordering::SeqCst)
    }

    /// Whether some file is decoding in the background.
    pub fn busy() -> bool {
        Self::with(|bank| {
            bank.decoding.retain(|w| w.strong_count() > 0);
            !bank.decoding.is_empty()
        })
    }
}

/// A WAV of a sine at `hz` at half level, `seconds` long, at `rate`, 16-bit, `channels` channels
/// (the second one upside down), for tests here and in the editor.
pub fn sine_wav(hz: f64, seconds: f64, rate: u32, channels: u16) -> Arc<[u8]> {
    let spec =
        hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for i in 0..(seconds * rate as f64) as usize {
            let v = (0.5 * (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin() * 32767.0) as i16;
            for c in 0..channels {
                w.write_sample(if c == 1 { -v } else { v }).unwrap();
            }
        }
        w.finalize().unwrap();
    }
    out.into_inner().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(hz: f64, seconds: f64, rate: u32, channels: u16) -> Arc<[u8]> {
        sine_wav(hz, seconds, rate, channels)
    }

    fn rises(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn a_wav_at_another_rate_comes_out_at_48k_the_same_length_and_pitch() {
        let bytes = wav(441.0, 1.0, 44_100, 1);
        let info = probe(&bytes).unwrap();
        assert_eq!((info.rate, info.channels), (44_100, 1));
        assert!((info.seconds.unwrap() - 1.0).abs() < 1e-3);
        let d = decode(&bytes).unwrap();
        assert_eq!(d.frames(), 48_000);
        let (l, r) = d.channels();
        assert_eq!(l, r, "mono on both sides");
        let n = rises(l);
        assert!((440..=441).contains(&n), "{n}");
        let peak = l[1000..47_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
        // Lined up: the first rise is where the sine's first rise is.
        let first = l.iter().position(|v| *v > 0.05).unwrap();
        assert!(first < 6, "{first}");
    }

    #[test]
    fn stereo_keeps_its_sides() {
        let d = decode(&wav(1000.0, 0.1, 48_000, 2)).unwrap();
        let (l, r) = d.channels();
        assert_eq!(d.frames(), 4800);
        assert!(l.iter().zip(r).skip(10).all(|(a, b)| (a + b).abs() < 1e-3), "right is the left turned over");
    }

    #[test]
    fn flac_mp3_ogg_and_aiff_decode_to_the_tone_they_hold() {
        let files: [(&str, &[u8]); 4] = [
            ("flac", include_bytes!("../tests/data/tone.flac")),
            ("mp3", include_bytes!("../tests/data/tone.mp3")),
            ("ogg", include_bytes!("../tests/data/tone.ogg")),
            ("aiff", include_bytes!("../tests/data/tone.aiff")),
        ];
        for (kind, bytes) in files {
            let bytes: Arc<[u8]> = Arc::from(bytes);
            assert!(probe(&bytes).is_ok(), "{kind}");
            let d = decode(&bytes).unwrap_or_else(|e| panic!("{kind}: {e}"));
            // A quarter second (an MP3's encoder delay trimmed, give or take a frame of padding).
            assert!((11_500..13_300).contains(&d.frames()), "{kind}: {}", d.frames());
            let (l, _) = d.channels();
            let n = rises(&l[1000..11_000]);
            // 10 000 frames of 440 Hz at 48 kHz: 91 or 92 rises.
            assert!((90..=93).contains(&n), "{kind}: {n}");
        }
    }

    #[test]
    fn rubbish_is_not_a_sound() {
        let junk: Arc<[u8]> = Arc::from(&b"this is not a sound file at all, just words"[..]);
        assert!(probe(&junk).is_err());
        assert!(decode(&junk).is_err());
        assert!(decode(&Arc::from(&b""[..])).is_err());
    }

    #[test]
    fn a_wav_saying_zero_hz_is_refused_not_a_crash() {
        // The decoder panics on a sample rate of 0; that is a file the program can't read.
        let mut bytes = wav(440.0, 0.05, 48_000, 1).to_vec();
        bytes[24..28].copy_from_slice(&0u32.to_le_bytes());
        let bytes: Arc<[u8]> = bytes.into();
        assert!(probe(&bytes).is_err());
        assert!(decode(&bytes).is_err());
        let held = bytes.clone();
        let start = std::time::Instant::now();
        while SampleBank::ready(&held).is_none() {
            assert!(start.elapsed().as_secs() < 10, "the background read ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(SampleBank::ready(&held).unwrap().is_err());
    }

    #[test]
    fn the_bank_decodes_each_file_once_and_in_the_background() {
        let bytes = wav(300.0, 0.05, 22_050, 1);
        assert!(SampleBank::ready(&bytes).is_none(), "not yet");
        let start = std::time::Instant::now();
        let got = loop {
            if let Some(d) = SampleBank::ready(&bytes) {
                break d.unwrap();
            }
            assert!(start.elapsed().as_secs() < 30, "the decode never came");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(Arc::ptr_eq(&got, &SampleBank::get(&bytes).unwrap()), "one decode, shared");
        let copy = bytes.clone();
        assert!(Arc::ptr_eq(&got, &SampleBank::get(&copy).unwrap()));
        assert!(SampleBank::decoded() >= 1);
    }

    #[test]
    fn a_name_ending_in_a_note_gives_the_root() {
        assert_eq!(guess_root("Piano C5.wav"), Some(60));
        assert_eq!(guess_root("Pad_F#4.flac"), Some(54));
        assert_eq!(guess_root("Bass-Eb2"), Some(27));
        assert_eq!(guess_root("Kick.wav"), None);
        assert_eq!(guess_root("Big room"), None);
        assert_eq!(guess_root("Brass A3 (soft).mp3"), Some(45));
    }
}
