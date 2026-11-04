//! Writing float WAV files.

use anyhow::Context;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// A 32-bit float stereo 48 kHz WAV being written.
pub(crate) struct WavOut {
    writer: hound::WavWriter<BufWriter<File>>,
}

impl WavOut {
    pub fn create(path: &Path) -> anyhow::Result<Self> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: crate::SAMPLE_RATE as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let writer =
            hound::WavWriter::create(path, spec).with_context(|| format!("could not write {}", path.display()))?;
        Ok(Self { writer })
    }

    /// Adds interleaved stereo samples.
    pub fn write(&mut self, samples: &[f32]) -> anyhow::Result<()> {
        for &s in samples {
            self.writer.write_sample(s).context("could not write the sound")?;
        }
        Ok(())
    }

    /// A 24-bit stereo 48 kHz WAV (a stem): samples are clipped to full scale.
    pub fn create_24(path: &Path) -> anyhow::Result<Self> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: crate::SAMPLE_RATE as u32,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let writer =
            hound::WavWriter::create(path, spec).with_context(|| format!("could not write {}", path.display()))?;
        Ok(Self { writer })
    }

    /// Adds interleaved stereo samples to a 24-bit file.
    pub fn write_24(&mut self, samples: &[f32]) -> anyhow::Result<()> {
        const FULL: f32 = 8_388_607.0;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * FULL).round() as i32;
            self.writer.write_sample(v).context("could not write the sound")?;
        }
        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<()> {
        self.writer.finalize().context("could not finish the sound file")
    }
}
