//! Mixing notes into a track and setting its level, sample for sample the same every time.

use crate::SAMPLE_RATE;

/// Peak ceiling for the final mix, about -1 dBFS, which streaming platforms expect.
pub const PEAK_CEILING: f32 = 0.89;

/// How many threads to share work between.
pub(crate) fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 32)
}

/// Adds every note into `samples` (interleaved stereo). `place` gives a note's first frame,
/// its sound, its left and right gains, and how many frames at the end of the sound to fade
/// out (for notes cut short). The clip is cut into stretches that are mixed side by side on all
/// cores. Within a stretch the notes are added in their listed order, so each sample gets
/// exactly the same sum, in the same order, as mixing them one by one.
pub fn mix_notes<'w, N: Sync>(
    samples: &mut [f32],
    notes: &'w [N],
    place: impl Fn(&'w N) -> (usize, &'w [f32], f32, f32, usize) + Sync,
) {
    let frames = samples.len() / 2;
    let pieces = (threads() * 4).min(frames.div_ceil(4096)).max(1);
    let stretch = frames.div_ceil(pieces).max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let chunks: Vec<std::sync::Mutex<&mut [f32]>> =
        samples.chunks_mut(stretch * 2).map(std::sync::Mutex::new).collect();
    let work = || {
        loop {
            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(chunk) = chunks.get(k) else { break };
            let mut out = chunk.lock().unwrap();
            let (first, last) = (k * stretch, k * stretch + out.len() / 2);
            for note in notes {
                let (start, wave, left, right, fade) = place(note);
                let end = (start + wave.len()).min(frames);
                if start >= last || end <= first {
                    continue;
                }
                let (from, to) = (start.max(first), end.min(last));
                // The faded tail is mixed separately so plain notes keep the simplest loop.
                let fade_from = (start + wave.len()).saturating_sub(fade).clamp(from, to);
                let plain = &wave[from - start..fade_from - start];
                let target = &mut out[(from - first) * 2..(fade_from - first) * 2];
                for (frame, &s) in target.chunks_exact_mut(2).zip(plain) {
                    frame[0] += s * left;
                    frame[1] += s * right;
                }
                let tail = &wave[fade_from - start..to - start];
                let target = &mut out[(fade_from - first) * 2..(to - first) * 2];
                let remaining = start + wave.len() - fade_from;
                for (i, (frame, &s)) in target.chunks_exact_mut(2).zip(tail).enumerate() {
                    let s = s * (remaining - i) as f32 / fade.max(1) as f32;
                    frame[0] += s * left;
                    frame[1] += s * right;
                }
            }
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads().min(pieces) {
            scope.spawn(work);
        }
        work();
    });
}

/// Scales the mix so its loud passages sit at `target_db` RMS (roughly LUFS for this kind of
/// dense, even material). Uses the 90th percentile of 400 ms windows so quiet starts and
/// endings do not drag the level around.
pub fn normalize_loudness(samples: &mut [f32], target_db: f32) {
    let window = (SAMPLE_RATE as usize * 2 * 4) / 10;
    let mut levels: Vec<f32> = samples
        .chunks(window)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .filter(|&rms| rms > 1e-6)
        .collect();
    if levels.is_empty() {
        return;
    }
    levels.sort_by(f32::total_cmp);
    let reference = levels[(levels.len() * 9 / 10).min(levels.len() - 1)];
    let gain = 10f32.powf(target_db / 20.0) / reference;
    samples.iter_mut().for_each(|s| *s *= gain);
}

/// Lookahead peak limiter: never lets a sample exceed [`PEAK_CEILING`], recovering over ~80 ms.
pub fn limit_peaks(samples: &mut [f32]) {
    let frames = samples.len() / 2;
    let lookahead = (SAMPLE_RATE as usize) / 200;
    let needed: Vec<f32> = samples
        .chunks_exact(2)
        .map(|f| {
            let peak = f[0].abs().max(f[1].abs());
            if peak > PEAK_CEILING { PEAK_CEILING / peak } else { 1.0 }
        })
        .collect();
    // Minimum of `needed` over [i, i + lookahead], with a monotonic queue.
    let mut window_min = vec![1.0f32; frames];
    let mut queue = std::collections::VecDeque::new();
    for i in (0..frames).rev() {
        while queue.back().is_some_and(|&j: &usize| needed[j] >= needed[i]) {
            queue.pop_back();
        }
        queue.push_back(i);
        while queue.front().is_some_and(|&j| j > i + lookahead) {
            queue.pop_front();
        }
        window_min[i] = needed[queue[0]];
    }
    let release = 1.0 - (-1.0 / (0.08 * SAMPLE_RATE as f32)).exp();
    let mut gain = 1.0f32;
    for (frame, &target) in samples.chunks_exact_mut(2).zip(&window_min) {
        gain = if target < gain { target } else { gain + (target - gain) * release };
        frame[0] *= gain;
        frame[1] *= gain;
    }
}

/// SplitMix64: a small, well-mixed hash, so note choice is random-looking but repeatable.
pub fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_holds_the_ceiling_and_mixing_is_order_stable() {
        let mut samples = vec![0.0f32; 48_000 * 2];
        let notes = [(100usize, vec![1.0f32; 2000]), (150, vec![0.7f32; 2000]), (47_500, vec![0.5f32; 2000])];
        mix_notes(&mut samples, &notes, |(start, wave)| (*start, wave.as_slice(), 1.0, 0.5, 0));
        assert_eq!(samples[200 * 2], 1.7);
        assert_eq!(samples[200 * 2 + 1], 0.85);
        assert_eq!(samples[47_999 * 2], 0.5, "a note past the end is cut");
        let mut again = vec![0.0f32; 48_000 * 2];
        mix_notes(&mut again, &notes, |(start, wave)| (*start, wave.as_slice(), 1.0, 0.5, 0));
        assert_eq!(samples, again);
        normalize_loudness(&mut samples, -14.0);
        limit_peaks(&mut samples);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= PEAK_CEILING + 1e-4, "peak {peak}");
        assert_ne!(mix64(1), mix64(2));
    }
}
