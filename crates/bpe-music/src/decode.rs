//! Reading mp3, wav, ogg and flac files.

use crate::{Pcm, SAMPLE_RATE};
use anyhow::{Context, anyhow};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decodes a song to 48 kHz stereo.
pub fn decode(path: impl AsRef<Path>) -> anyhow::Result<Pcm> {
    let path = path.as_ref();
    let file = std::fs::File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .with_context(|| format!("{} isn't a song file this program can read", path.display()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("{} has no audio in it", path.display()))?;
    let track_id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(SAMPLE_RATE);
    let mut channels = track.codec_params.channels.map_or(0, |c| c.count());
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut samples = Vec::new();
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                rate = spec.rate;
                channels = spec.channels.count();
                let needed = decoded.capacity() as u64;
                let buffer = match &mut buffer {
                    Some(b) if b.capacity() as u64 >= needed * channels as u64 => b,
                    _ => buffer.insert(SampleBuffer::new(needed, spec)),
                };
                buffer.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buffer.samples());
            }
            // A damaged packet: skip it, like a player would.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(to_stereo_48k(Pcm { rate, channels: channels.max(1) as u16, samples }))
}

/// Converts any channel count and rate to 48 kHz stereo. Mono is copied to both sides; extra
/// channels beyond the first two are dropped.
pub fn to_stereo_48k(pcm: Pcm) -> Pcm {
    let channels = pcm.channels.max(1) as usize;
    let frames = pcm.samples.len() / channels;
    let side = |c: usize| -> Vec<f32> {
        let c = c.min(channels - 1);
        (0..frames).map(|f| pcm.samples[f * channels + c]).collect()
    };
    let (left, right) = (side(0), side(1));
    let (left, right) = if pcm.rate == SAMPLE_RATE {
        (left, right)
    } else {
        (resample(&left, pcm.rate, SAMPLE_RATE), resample(&right, pcm.rate, SAMPLE_RATE))
    };
    let mut samples = Vec::with_capacity(left.len() * 2);
    for (l, r) in left.into_iter().zip(right) {
        samples.extend_from_slice(&[l, r]);
    }
    Pcm { rate: SAMPLE_RATE, channels: 2, samples }
}

/// Cubic (Catmull-Rom) resampling. Plenty for analysis and for a song under the bounces.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 {
        return Vec::new();
    }
    let ratio = from as f64 / to as f64;
    let len = (input.len() as f64 / ratio).floor() as usize;
    let at = |i: isize| input[i.clamp(0, input.len() as isize - 1) as usize];
    (0..len)
        .map(|k| {
            let x = k as f64 * ratio;
            let i = x.floor() as isize;
            let t = (x - i as f64) as f32;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            p1 + 0.5
                * t
                * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)))
        })
        .collect()
}
