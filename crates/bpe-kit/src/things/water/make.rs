//! Make final (Water mode part 8): the scene's water worked out once at Final detail, from the
//! start or from the restart point before its earliest change, frame by frame, keeping each frame's
//! moments (more inside it where it plays in slow motion) and a restart point every 2 s, into the
//! take the finals folder keeps. The card steps on while a writer thread packs and writes.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Context;
use bpe_core::{Resolved, Scene};
use bpe_water::drops::capture::{Capture, Captured};
use bpe_water::take::{ChunkWriter, FrameCodec, chunk};
use bpe_water::{SolidField, Water, WaterGpu};
use rayon::prelude::*;

use super::draft::{FINAL_MOST, Speeds, WaterSetup};
use super::finals::{Finals, POINT_EVERY, ROOM_LEFT, Status, TakeDir, TakeHead, Wanted, free_space, room_needed};

/// What a make is doing, for a progress bar (every number as the make goes).
#[derive(Debug, Default)]
pub struct MakeProgress {
    /// The water frame it is at, the frame it started from and the frame it stops at.
    pub at: AtomicU32,
    pub from: AtomicU32,
    pub to: AtomicU32,
    /// The drops in the body now, and the bytes written so far.
    pub drops: AtomicU32,
    pub bytes: AtomicU64,
    /// Milliseconds the field took to build at Final detail.
    pub field_ms: AtomicU32,
    /// 0 building the course, 1 making, 2 done.
    pub phase: AtomicU32,
}

impl MakeProgress {
    /// How far through it is, 0 to 1.
    pub fn share(&self) -> f32 {
        let (from, to, at) =
            (self.from.load(Ordering::Relaxed), self.to.load(Ordering::Relaxed), self.at.load(Ordering::Relaxed));
        if to <= from { 0.0 } else { (at.saturating_sub(from) as f32 / (to - from) as f32).clamp(0.0, 1.0) }
    }
}

/// The card's name and driver, as a take's head keeps them.
pub fn card_name(device: &wgpu::Device) -> String {
    let info = device.adapter_info();
    format!("{} ({}, {})", info.name.trim(), info.driver.trim(), info.driver_info.trim())
}

/// The card finals are made on here (the first a window-less device takes, as the make and the
/// command line take it), as a take's head keeps it.
pub fn this_card() -> &'static str {
    static CARD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CARD.get_or_init(|| match bpe_gfx::headless_adapter(wgpu::PowerPreference::HighPerformance) {
        Ok(adapter) => {
            let info = adapter.get_info();
            format!("{} ({}, {})", info.name.trim(), info.driver.trim(), info.driver_info.trim())
        }
        Err(_) => "no card".into(),
    })
}

/// A scene's final to make.
#[derive(Clone, Debug)]
pub struct MakeFinal {
    /// The scene at Final detail.
    pub setup: Arc<WaterSetup>,
    pub speeds: Speeds,
    /// The water frames the scene plays (its Length).
    pub frames: u32,
    /// The most drops it holds.
    pub most: u32,
    /// The disk's free room, for tests (`None`: asked of the disk).
    pub free_room: Option<u64>,
}

impl MakeFinal {
    /// `scene`'s final, `length` water seconds long.
    pub fn of(scene: &Scene, resolved: &Resolved, length: f64) -> MakeFinal {
        MakeFinal {
            setup: Arc::new(WaterSetup::final_of(scene, resolved)),
            speeds: Speeds::of(scene, resolved, length),
            frames: (length.max(0.0) * 60.0).ceil() as u32,
            most: FINAL_MOST,
            free_room: None,
        }
    }

    /// The take's print: the scene's physics at its detail, and the engine and program.
    pub fn print(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        (self.setup.print, bpe_water::take::engine_print(), super::finals::PROGRAM).hash(&mut h);
        h.finish()
    }

    /// Where the scene's final is in `finals`, made on `card`.
    pub fn status(&self, finals: &Finals, card: &str) -> Status {
        let inputs = self.setup.inputs();
        finals.find(&Wanted {
            print: self.print(),
            inputs: &inputs,
            cell: self.setup.cell,
            card,
            frames: self.frames,
            speeds: &self.speeds,
        })
    }

    /// The moments frames `from` to the end keep.
    pub fn moments_from(&self, from: u32) -> u64 {
        (from..self.frames).map(|k| u64::from(self.speeds.keep_for(k))).sum::<u64>() + 1
    }

    /// Makes what is missing of the final (or out of date) into `finals` on `gpu`; returns the
    /// take's head. `stop` ends it at the next frame, keeping every whole chunk.
    pub fn make(
        &self,
        gpu: &Arc<WaterGpu>,
        finals: &Finals,
        progress: &MakeProgress,
        stop: &AtomicBool,
    ) -> anyhow::Result<TakeHead> {
        let card = card_name(gpu.device());
        let status = self.status(finals, &card);
        if status.own
            && status.complete()
            && let Some(take) = status.take
        {
            finals.touch(take.print);
            progress.phase.store(2, Ordering::Release);
            return Ok(take);
        }
        let from = if status.take.is_some() { status.restart } else { 0 };
        progress.from.store(from, Ordering::Relaxed);
        progress.at.store(from, Ordering::Relaxed);
        progress.to.store(self.frames, Ordering::Relaxed);
        progress.phase.store(0, Ordering::Release);
        let built = Instant::now();
        let field = self.setup.field();
        progress.field_ms.store(built.elapsed().as_millis() as u32, Ordering::Relaxed);
        let layout = field.layout;
        anyhow::ensure!(
            bpe_water::take::frame::fits(&layout),
            "This course is too big for a final at this detail: pick a coarser Final detail or fewer screens."
        );
        // (Room for the moments still to make, by the bytes a drop took last time.)
        let drops = status
            .take
            .as_ref()
            .map_or(0, |t| t.drops)
            .max(self.setup.drops_about(&field, f64::from(self.frames) / 60.0));
        let a_drop = status.take.as_ref().and_then(TakeHead::bytes_a_drop);
        let needed = room_needed(self.moments_from(from), drops, self.frames - from.min(self.frames), a_drop);
        let free = self.free_room.or_else(|| {
            std::fs::create_dir_all(finals.dir()).ok();
            free_space(finals.dir())
        });
        if let Some(free) = free
            && needed.saturating_add(ROOM_LEFT) > free
        {
            anyhow::bail!(
                "The final needs about {} and the disk has {} free: clear some finals (the Finals menu) or make room on the disk.",
                gb(needed),
                gb(free)
            );
        }
        let head = TakeHead::new(self.print(), self.setup.cell, &layout, self.setup.inputs(), self.frames, &card);
        let take = finals.begin(head, status.take.as_ref().map(|t| (t, from)))?;
        let mut water = if from > 0 {
            let state = chunk::read_point(&take.dir, from).context("the restart point doesn't read")?;
            self.setup.resume(gpu.clone(), &field, &state)?
        } else {
            self.setup.start(gpu.clone(), &field, self.most)
        };
        let take = Arc::new(Mutex::new(take));
        let (send, recv) = sync_channel::<Message>(3);
        let writer = {
            let take = take.clone();
            let cells = (layout.cols - 1) * (layout.rows - 1);
            std::thread::Builder::new()
                .name("water final writer".into())
                .spawn(move || write(recv, take, cells))
                .context("couldn't start the final's writer")?
        };
        progress.phase.store(1, Ordering::Release);
        let played = self.play(gpu, &field, &mut water, &take, &send, progress, stop, from);
        let complete = played.is_ok() && !stop.load(Ordering::Acquire);
        let _ = send.send(Message::End { complete });
        drop(send);
        let wrote = writer.join().map_err(|_| anyhow::anyhow!("the final's writer stopped"))?;
        played?;
        wrote?;
        progress.phase.store(2, Ordering::Release);
        let take = Arc::try_unwrap(take).map_err(|_| anyhow::anyhow!("the take is still in use"))?;
        let take = take.into_inner().map_err(|_| anyhow::anyhow!("the take's lock broke"))?;
        Ok(take.release())
    }

    /// Plays frames `from` on, sending each frame's moments to the writer.
    #[allow(clippy::too_many_arguments)]
    fn play(
        &self,
        gpu: &Arc<WaterGpu>,
        field: &SolidField,
        water: &mut Water,
        take: &Arc<Mutex<TakeDir>>,
        send: &SyncSender<Message>,
        progress: &MakeProgress,
        stop: &AtomicBool,
        from: u32,
    ) -> anyhow::Result<()> {
        let mut capture = Capture::new();
        let started = Instant::now();
        let mut timed = from;
        let mut mark = started;
        water.capture_now(&mut capture);
        for k in from..self.frames {
            if stop.load(Ordering::Acquire) {
                break;
            }
            if k > from && k % POINT_EVERY == 0 {
                let state = water.state();
                let dir = take.lock().map_err(|_| anyhow::anyhow!("the take's lock broke"))?.dir.clone();
                chunk::write_point(&dir, k, &state)?;
                take.lock().map_err(|_| anyhow::anyhow!("the take's lock broke"))?.point_done(k)?;
            }
            self.setup.set_inputs(water, field, self.most);
            water.frame_keeping(self.speeds.keep_for(k), &mut capture);
            let ready = capture.take(gpu.device(), false);
            if !ready.is_empty() {
                let frames = k + 1 - timed;
                timed = k + 1;
                let seconds = mark.elapsed().as_secs_f64();
                mark = Instant::now();
                if send.send(Message::Moments { moments: ready, drops: water.count(), seconds, frames }).is_err() {
                    break;
                }
            }
            progress.at.store(k + 1, Ordering::Relaxed);
            progress.drops.store(water.count(), Ordering::Relaxed);
            progress.bytes.store(take.lock().map(|t| t.head.bytes()).unwrap_or(0), Ordering::Relaxed);
        }
        let rest = capture.take(gpu.device(), true);
        let frames = progress.at.load(Ordering::Relaxed).saturating_sub(timed);
        let _ = send.send(Message::Moments {
            moments: rest,
            drops: water.count(),
            seconds: mark.elapsed().as_secs_f64(),
            frames,
        });
        Ok(())
    }
}

/// What the make sends its writer.
enum Message {
    Moments {
        moments: Vec<Captured>,
        drops: u32,
        seconds: f64,
        frames: u32,
    },
    /// The make is over: the last chunk is whole when `complete` (else it is let go).
    End {
        complete: bool,
    },
}

/// The writer: packs each moment (several at once), writes them into their second's chunk, and
/// tells the head when a chunk is whole.
fn write(recv: Receiver<Message>, take: Arc<Mutex<TakeDir>>, cells: u32) -> anyhow::Result<()> {
    let dir = take.lock().map_err(|_| anyhow::anyhow!("the take's lock broke"))?.dir.clone();
    let mut chunk: Option<ChunkWriter> = None;
    let (mut seconds, mut frames, mut most) = (0.0, 0u32, 0u32);
    let finish = |w: ChunkWriter, seconds: &mut f64, frames: &mut u32, most: u32| -> anyhow::Result<()> {
        let info = w.finish()?;
        take.lock().map_err(|_| anyhow::anyhow!("the take's lock broke"))?.chunk_done(info, most, *seconds, *frames)?;
        *seconds = 0.0;
        *frames = 0;
        Ok(())
    };
    let outcome = (|| -> anyhow::Result<()> {
        for message in recv.iter() {
            match message {
                Message::Moments { moments, drops, seconds: s, frames: f } => {
                    seconds += s;
                    frames += f;
                    most = most.max(drops);
                    let packed: Vec<Vec<u8>> = moments
                        .par_iter()
                        .map_init(FrameCodec::new, |codec, c| codec.encode(&c.drops, cells))
                        .collect();
                    for (c, bytes) in moments.iter().zip(packed) {
                        let second = c.moment.frame / 60;
                        if chunk.as_ref().is_some_and(|w| w.second() != second)
                            && let Some(w) = chunk.take()
                        {
                            finish(w, &mut seconds, &mut frames, most)?;
                        }
                        if chunk.is_none() {
                            chunk = Some(ChunkWriter::new(&dir, second)?);
                        }
                        if let Some(w) = chunk.as_mut() {
                            w.push(c.moment, &bytes)?;
                        }
                    }
                }
                Message::End { complete } => {
                    if let Some(w) = chunk.take() {
                        if complete {
                            finish(w, &mut seconds, &mut frames, most)?;
                        } else {
                            w.abandon();
                        }
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    })();
    if let Some(w) = chunk.take() {
        w.abandon();
    }
    outcome
}

/// Bytes in plain words.
pub fn gb(bytes: u64) -> String {
    let g = bytes as f64 / f64::from(1u32 << 30);
    if g >= 10.0 {
        format!("{g:.0} GB")
    } else if g >= 0.1 {
        format!("{g:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / f64::from(1u32 << 20))
    }
}
