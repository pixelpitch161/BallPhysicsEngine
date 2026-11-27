//! The Final view (Water mode part 8): the stage shows the take's moment nearest the playhead,
//! its drops moved on by their speed to the playhead's time, in the plain look. A worker thread
//! opens the take, reads and unpacks the newest moment asked for (and lays out the course at the
//! Final cell once a scene); the window's thread only hands the drops to the card and draws them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use bpe_cli::water::TakeReader;
use bpe_gfx::wgpu;
use bpe_kit::things::water::draft::WaterSetup;
use bpe_kit::things::water::finals::{Finals, Status};
use bpe_water::SolidField;
use bpe_water::draw::{Plain, PlainGpu, PlainLook, StoredDrops};
use bpe_water::take::TakeFrame;
use eframe::egui::{self, Painter, Rect, pos2};

use super::course_view::CourseView;
use super::draft::frame_of;
use super::final_job::Found;
use super::picture::picture_view;
use crate::style;

/// What the stage can show of the final at the playhead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FinalShown {
    /// The final's moment.
    Made,
    /// The final has not made this moment: the stage shows the draft.
    NotMade,
    /// The moment made is out of date: shown, and a make starts again from the restart frame.
    OutOfDate { restart: u32 },
}

/// What the stage says over a moment the final has not made.
pub const NOT_MADE: &str = "Not made yet: showing the draft";

/// What the stage says over a moment that is out of date.
pub fn out_of_date(restart: u32) -> String {
    let seconds = f64::from(restart) / 60.0;
    let text = format!("{seconds:.1}");
    format!("Out of date: Make final remakes from {} s", text.trim_end_matches('0').trim_end_matches('.'))
}

/// What the final shows at water time `tau`.
pub fn shown_at(found: &Found, tau: f64) -> FinalShown {
    let k = frame_of(tau);
    let status = &found.status;
    match &status.take {
        Some(_) if status.good_at(k) => FinalShown::Made,
        Some(_) if k < status.made => FinalShown::OutOfDate { restart: status.restart },
        _ => FinalShown::NotMade,
    }
}

/// A moment asked of the worker.
#[derive(Clone)]
struct Asked {
    finals: Finals,
    /// The take to read and its frames to read (all it made, out of date or not).
    status: Status,
    setup: Arc<WaterSetup>,
    tau: f64,
}

impl Asked {
    fn same(&self, other: &Asked) -> bool {
        let take = |a: &Asked| a.status.take.as_ref().map(|t| t.print);
        take(self) == take(other)
            && self.status.good == other.status.good
            && self.setup.print == other.setup.print
            && self.tau.to_bits() == other.tau.to_bits()
            && self.finals.dir() == other.finals.dir()
    }
}

/// A moment read: the take it is of, the time asked for, and the drops with how far the time is
/// past them (`None`: not there).
struct Got {
    take: u64,
    tau: f64,
    frame: Option<(TakeFrame, f64)>,
}

/// What the worker and the window share.
#[derive(Default)]
struct Shared {
    ask: Mutex<Option<Asked>>,
    wake: Condvar,
    got: Mutex<Option<Got>>,
    /// The course at the Final cell, for the setup of this print.
    field: Mutex<Option<(u64, Arc<SolidField>)>>,
    stop: AtomicBool,
}

struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(ctx: egui::Context) -> Option<Worker> {
        let shared = Arc::new(Shared::default());
        let theirs = shared.clone();
        let thread =
            std::thread::Builder::new().name("water final view".into()).spawn(move || work(&theirs, &ctx)).ok()?;
        Some(Worker { shared, thread: Some(thread) })
    }

    fn ask(&self, asked: Asked) {
        if let Ok(mut ask) = self.shared.ask.lock() {
            *ask = Some(asked);
        }
        self.shared.wake.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The worker: the newest moment asked for, read and unpacked.
fn work(shared: &Shared, ctx: &egui::Context) {
    let mut reader: Option<(u64, u32, TakeReader)> = None;
    let mut field_for = None;
    loop {
        let asked = {
            let Ok(mut ask) = shared.ask.lock() else { return };
            while ask.is_none() && !shared.stop.load(Ordering::Acquire) {
                ask = match shared.wake.wait(ask) {
                    Ok(ask) => ask,
                    Err(_) => return,
                };
            }
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            ask.take().expect("waited for it")
        };
        if field_for != Some(asked.setup.print) {
            let field = Arc::new(asked.setup.field());
            field_for = Some(asked.setup.print);
            if let Ok(mut slot) = shared.field.lock() {
                *slot = Some((asked.setup.print, field));
            }
        }
        let Some(take) = asked.status.take.as_ref().map(|t| t.print) else { continue };
        if reader.as_ref().is_none_or(|r| (r.0, r.1) != (take, asked.status.good)) {
            reader = TakeReader::open(&asked.finals, &asked.status).map(|r| (take, asked.status.good, r));
        }
        let frame = reader.as_mut().and_then(|r| r.2.near(asked.tau).ok().flatten());
        if let Ok(mut got) = shared.got.lock() {
            *got = Some(Got { take, tau: asked.tau, frame });
        }
        ctx.request_repaint();
    }
}

/// What a drawn picture showed.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    take: u64,
    tau: u64,
    size: (u32, u32),
    look: PlainLook,
}

/// The Final view's picture.
#[derive(Default)]
pub struct FinalPicture {
    worker: Option<Worker>,
    asked: Option<Asked>,
    gpu: Option<Arc<PlainGpu>>,
    /// The drawing, for the setup of this print.
    plain: Option<(u64, Plain)>,
    stored: Option<StoredDrops>,
    /// The drops on the card: the take and time they are for, and how far that is past them.
    holds: Option<(u64, f64, f32)>,
    texture: Option<(egui::TextureId, (u32, u32))>,
    drawn: Option<Drawn>,
}

impl FinalPicture {
    /// Shows the final's moment at water time `tau` over the course's rect (asking the worker for
    /// it, and drawing it once it is read); `false` while there is no picture of it yet.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        painter: &Painter,
        render: &egui_wgpu::RenderState,
        found: &Found,
        finals: &Finals,
        look: &PlainLook,
        view: &CourseView,
        tau: f64,
    ) -> bool {
        let Some(take) = found.status.take.as_ref().map(|t| t.print) else { return false };
        if self.worker.is_none() {
            self.worker = Worker::start(painter.ctx().clone());
        }
        let Some(worker) = &self.worker else { return false };
        let mut status = found.status.clone();
        status.good = status.made;
        let asked = Asked { finals: finals.clone(), status, setup: found.make.setup.clone(), tau };
        if self.asked.as_ref().is_none_or(|a| !a.same(&asked)) {
            worker.ask(asked.clone());
            self.asked = Some(asked);
        }
        let gpu = self.gpu.get_or_insert_with(|| PlainGpu::new(&render.device, &render.queue)).clone();
        let setup = &found.make.setup;
        let field = worker.shared.field.lock().ok().and_then(|f| f.clone());
        if let Some((print, field)) = field
            && print == setup.print
            && self.plain.as_ref().is_none_or(|p| p.0 != print)
        {
            self.plain = Some((print, Plain::new(gpu, &field, setup.numbers().wall_off)));
            self.drawn = None;
        }
        if let Some(got) = worker.shared.got.lock().ok().and_then(|mut g| g.take())
            && let Some((frame, ahead)) = got.frame
        {
            let stored = self.stored.get_or_insert_with(|| StoredDrops::new(&render.device, &render.queue, &[]));
            stored.set(&render.device, &render.queue, &frame.xv, &frame.kinds);
            self.holds = Some((got.take, got.tau, ahead as f32));
        }
        let v = picture_view(view, painter.ctx().pixels_per_point());
        if let (Some((_, plain)), Some(stored), Some((held, at, ahead))) = (&mut self.plain, &self.stored, self.holds)
            && held == take
        {
            let want = Drawn { take, tau: at.to_bits(), size: (v.width, v.height), look: look.clone() };
            if self.drawn.as_ref() != Some(&want) {
                plain.draw_at(&stored.buffers(), &v, look, ahead);
                if let Some(output) = plain.output() {
                    let output = output.create_view(&wgpu::TextureViewDescriptor::default());
                    let mut renderer = render.renderer.write();
                    let id = match self.texture {
                        Some((id, _)) => {
                            renderer.update_egui_texture_from_wgpu_texture(
                                &render.device,
                                &output,
                                wgpu::FilterMode::Linear,
                                id,
                            );
                            id
                        }
                        None => renderer.register_native_texture(&render.device, &output, wgpu::FilterMode::Linear),
                    };
                    self.texture = Some((id, want.size));
                }
                self.drawn = Some(want);
            }
        }
        let shows = self.drawn.as_ref().is_some_and(|d| d.take == take);
        match self.texture {
            Some((id, _)) if shows => {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(id, view.rect, uv, style::UNTINTED);
                true
            }
            _ => false,
        }
    }

    /// Lets the picture and the worker go.
    pub fn free(&mut self, render: Option<&egui_wgpu::RenderState>) {
        if let (Some((id, _)), Some(render)) = (self.texture.take(), render) {
            render.renderer.write().free_texture(&id);
        }
        *self = FinalPicture::default();
    }
}
