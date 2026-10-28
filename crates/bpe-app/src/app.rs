use crate::audio_out::AudioOut;
use crate::inspector;
use bpe_audio::Soundtrack;
use bpe_export::ExportOptions;
use bpe_render::{FrameData, Renderer};
use bpe_scene::Scene;
use bpe_sim::Playback;
use eframe::egui::{self, Key, RichText};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// Wait this long after the last edit before remixing the soundtrack.
const SOUND_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct StudioApp {
    scene: Scene,
    scene_path: Option<PathBuf>,
    saved_scene: Scene,
    progression_text: String,
    playback: Playback,
    frame_data: FrameData,
    time: f64,
    playing: bool,
    looping: bool,
    muted: bool,
    last_tick: Option<Instant>,
    preview: Option<Preview>,
    audio: Option<AudioOut>,
    sound: SoundJob,
    export: ExportPanel,
    status: String,
}

struct Preview {
    renderer: Renderer,
    texture: egui::TextureId,
}

/// Mixes the soundtrack on a background thread whenever the scene changes.
struct SoundJob {
    generation: u64,
    dirty_since: Option<Instant>,
    running: bool,
    sender: Sender<(u64, anyhow::Result<Soundtrack>)>,
    receiver: Receiver<(u64, anyhow::Result<Soundtrack>)>,
    error: Option<String>,
}

struct ExportPanel {
    open: bool,
    path: String,
    half_size: bool,
    crf: u8,
    ffmpeg: String,
    job: Option<ExportJob>,
    message: Option<String>,
}

struct ExportJob {
    done: Arc<AtomicU32>,
    total: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<anyhow::Result<()>>,
    started: Instant,
}

impl StudioApp {
    pub fn new(_cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        let (mut scene, mut status) = (Scene::default(), String::from("Space plays and pauses."));
        if let Some(path) = &scene_path {
            match Scene::load(path) {
                Ok(loaded) => scene = loaded,
                Err(error) => status = format!("Could not open {}: {error:#}", path.display()),
            }
        }
        let audio = AudioOut::new().map_err(|e| status = format!("No sound in preview: {e}")).ok();
        let (sender, receiver) = channel();
        let mut app = Self {
            progression_text: inspector::progression_to_text(&scene.sound.progression),
            playback: Playback::new(&scene),
            saved_scene: scene.clone(),
            scene,
            scene_path,
            frame_data: FrameData::default(),
            time: 0.0,
            playing: false,
            looping: true,
            muted: false,
            last_tick: None,
            preview: None,
            audio,
            sound: SoundJob {
                generation: 0,
                dirty_since: Some(Instant::now()),
                running: false,
                sender,
                receiver,
                error: None,
            },
            export: ExportPanel {
                open: false,
                path: "ball-video.mp4".into(),
                half_size: false,
                crf: 18,
                ffmpeg: "ffmpeg".into(),
                job: None,
                message: None,
            },
            status,
        };
        app.sound.dirty_since = Some(Instant::now() - SOUND_DEBOUNCE);
        app
    }

    fn base_dir(&self) -> Option<PathBuf> {
        self.scene_path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

    fn scene_changed(&mut self) {
        self.playback = Playback::new(&self.scene);
        self.time = self.time.min(self.scene.video.duration as f64);
        self.sound.dirty_since = Some(Instant::now());
    }

    fn replace_scene(&mut self, scene: Scene, path: Option<PathBuf>) {
        self.progression_text = inspector::progression_to_text(&scene.sound.progression);
        self.saved_scene = scene.clone();
        self.scene = scene;
        self.scene_path = path;
        self.time = 0.0;
        self.set_playing(false);
        self.scene_changed();
    }

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.last_tick = None;
        if let Some(audio) = &self.audio {
            if playing { audio.play_from(self.time) } else { audio.pause() }
        }
    }

    fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.scene.video.duration as f64);
        if self.playing
            && let Some(audio) = &self.audio
        {
            audio.play_from(self.time);
        }
    }

    fn advance_clock(&mut self) {
        if !self.playing {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last_tick {
            self.time += (now - last).as_secs_f64();
        }
        self.last_tick = Some(now);
        let duration = self.scene.video.duration as f64;
        if self.time >= duration {
            if self.looping {
                self.time = 0.0;
                if let Some(audio) = &self.audio {
                    audio.play_from(0.0);
                }
            } else {
                self.time = duration;
                self.set_playing(false);
            }
        }
    }

    fn update_soundtrack(&mut self, ctx: &egui::Context) {
        while let Ok((generation, result)) = self.sound.receiver.try_recv() {
            self.sound.running = false;
            if generation != self.sound.generation {
                continue;
            }
            match result {
                Ok(track) => {
                    self.sound.error = None;
                    if let Some(audio) = &self.audio {
                        audio.set_track(Arc::new(track.samples));
                        if self.playing {
                            audio.play_from(self.time);
                        }
                    }
                }
                Err(error) => self.sound.error = Some(format!("{error:#}")),
            }
        }
        let due = self.sound.dirty_since.is_some_and(|t| t.elapsed() >= SOUND_DEBOUNCE);
        if due && !self.sound.running {
            self.sound.dirty_since = None;
            self.sound.running = true;
            self.sound.generation += 1;
            let (generation, scene, base_dir, sender, ctx) =
                (self.sound.generation, self.scene.clone(), self.base_dir(), self.sound.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                let hits = bpe_sim::collect_wall_hits(&scene);
                let result = bpe_audio::render_soundtrack(&scene, &hits, base_dir.as_deref());
                let _ = sender.send((generation, result));
                ctx.request_repaint();
            });
        } else if self.sound.dirty_since.is_some() {
            ctx.request_repaint_after(SOUND_DEBOUNCE);
        }
    }

    fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Scene", &["ron"]).pick_file() {
            match Scene::load(&path) {
                Ok(scene) => {
                    self.status = format!("Opened {}", path.display());
                    self.replace_scene(scene, Some(path));
                }
                Err(error) => self.status = format!("Could not open {}: {error:#}", path.display()),
            }
        }
    }

    fn save(&mut self, choose_path: bool) {
        let path = match (&self.scene_path, choose_path) {
            (Some(path), false) => Some(path.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("Scene", &["ron"])
                .set_file_name(format!("{}.ron", self.scene.name))
                .save_file(),
        };
        let Some(path) = path else { return };
        match self.scene.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.saved_scene = self.scene.clone();
                self.scene_path = Some(path);
            }
            Err(error) => self.status = format!("Could not save: {error:#}"),
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open scene...").clicked() {
                    self.open();
                }
                if ui.button("Save").clicked() {
                    self.save(false);
                }
                if ui.button("Save as...").clicked() {
                    self.save(true);
                }
            });
            ui.menu_button("Presets", |ui| {
                for (name, scene) in Scene::presets() {
                    if ui.button(name).clicked() {
                        self.replace_scene(scene, None);
                    }
                }
            });
            ui.separator();
            let title = match &self.scene_path {
                Some(path) => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                None => "Unsaved scene".into(),
            };
            let marker = if self.scene != self.saved_scene { " (edited)" } else { "" };
            ui.label(format!("{title}{marker}"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("Export video").strong()).clicked() {
                    self.export.open = true;
                }
            });
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = if self.playing { "Pause" } else { "Play" };
            if ui.button(label).clicked() {
                self.set_playing(!self.playing);
            }
            if ui.button("Restart").clicked() {
                self.seek(0.0);
                self.set_playing(true);
            }
            let duration = self.scene.video.duration as f64;
            let mut time = self.time;
            ui.style_mut().spacing.slider_width = (ui.available_width() - 330.0).max(100.0);
            let response = ui.add(egui::Slider::new(&mut time, 0.0..=duration).show_value(false));
            if response.changed() {
                self.seek(time);
            }
            let frame = (self.time * self.scene.video.fps as f64).floor() as u32;
            ui.monospace(format!("{:5.2} s / {:.2} s  frame {frame}", self.time, duration));
            ui.checkbox(&mut self.looping, "Loop");
            if ui.checkbox(&mut self.muted, "Mute").changed()
                && let Some(audio) = &self.audio
            {
                audio.set_muted(self.muted);
            }
        });
        ui.horizontal(|ui| {
            ui.small(&self.status);
            if self.sound.running || self.sound.dirty_since.is_some() {
                ui.small("Updating sound...");
            }
            if let Some(error) = &self.sound.error {
                ui.colored_label(ui.visuals().error_fg_color, format!("Sound: {error}"));
            }
        });
    }

    fn preview(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else {
            ui.label("The preview needs the wgpu renderer.");
            return;
        };
        let video = &self.scene.video;
        let aspect = video.width as f32 / video.height.max(1) as f32;
        let available = ui.available_size();
        let mut size = egui::vec2(available.y * aspect, available.y);
        if size.x > available.x {
            size = egui::vec2(available.x, available.x / aspect);
        }
        // Render at the screen's real pixel size, but never above the video's own size.
        let scale = (size.x * ui.ctx().pixels_per_point() / video.width as f32).min(1.0);
        let (width, height) = bpe_export::output_size(&self.scene, scale);

        let device = &render_state.device;
        let preview = self.preview.get_or_insert_with(|| {
            let renderer = Renderer::new(device, width, height);
            let texture = render_state.renderer.write().register_native_texture(
                device,
                renderer.output_view(),
                bpe_render::wgpu::FilterMode::Linear,
            );
            Preview { renderer, texture }
        });
        if preview.renderer.size() != (width, height) {
            preview.renderer.resize(device, width, height);
            render_state.renderer.write().update_egui_texture_from_wgpu_texture(
                device,
                preview.renderer.output_view(),
                bpe_render::wgpu::FilterMode::Linear,
                preview.texture,
            );
        }

        self.playback.seek(self.time);
        let pixel = preview.renderer.pixel_size(&self.scene);
        self.frame_data.fill(&self.scene, &self.playback, pixel);
        preview.renderer.render(device, &render_state.queue, &self.scene, self.time, &self.frame_data);

        ui.centered_and_justified(|ui| {
            ui.image(egui::load::SizedTexture::new(preview.texture, size));
        });
    }

    fn export_window(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if let Some(job) = &self.export.job
            && let Ok(result) = job.receiver.try_recv()
        {
            let seconds = job.started.elapsed().as_secs_f32();
            self.export.message = Some(match result {
                Ok(()) => format!("Saved {} in {seconds:.0} s.", self.export.path),
                Err(error) => format!("Export failed: {error:#}"),
            });
            self.export.job = None;
        }
        let mut open = self.export.open;
        egui::Window::new("Export video").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            let busy = self.export.job.is_some();
            ui.add_enabled_ui(!busy, |ui| {
                egui::Grid::new("export").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Save to");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.export.path).desired_width(240.0));
                        if ui.button("Browse").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("MP4 video", &["mp4"])
                                .set_file_name(format!("{}.mp4", self.scene.name))
                                .save_file()
                        {
                            self.export.path = path.display().to_string();
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        let video = &self.scene.video;
                        ui.radio_value(
                            &mut self.export.half_size,
                            false,
                            format!("Full ({} x {})", video.width, video.height),
                        );
                        ui.radio_value(&mut self.export.half_size, true, "Half, for a quick draft");
                    });
                    ui.end_row();
                    ui.label("Quality");
                    ui.add(egui::Slider::new(&mut self.export.crf, 14..=30).text("CRF, lower is better"));
                    ui.end_row();
                    ui.label("ffmpeg");
                    ui.text_edit_singleline(&mut self.export.ffmpeg);
                    ui.end_row();
                });
            });
            ui.add_space(6.0);
            if let Some(job) = &self.export.job {
                let (done, total) = (job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed).max(1));
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(format!("Frame {done} of {total}")));
                if ui.button("Cancel").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            } else if ui.button(RichText::new("Export").strong()).clicked() {
                self.start_export(ctx, frame);
            }
            if let Some(message) = &self.export.message {
                ui.label(message);
            }
        });
        self.export.open = open;
    }

    fn start_export(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else { return };
        let mut options = ExportOptions::new(&self.export.path);
        options.scale = if self.export.half_size { 0.5 } else { 1.0 };
        options.crf = self.export.crf;
        options.ffmpeg = self.export.ffmpeg.clone().into();
        options.base_dir = self.base_dir();
        let (device, queue, scene, ctx) =
            (render_state.device.clone(), render_state.queue.clone(), self.scene.clone(), ctx.clone());
        let (done, total, cancel) = (
            Arc::new(AtomicU32::new(0)),
            Arc::new(AtomicU32::new(scene.frame_count())),
            Arc::new(AtomicBool::new(false)),
        );
        let (sender, receiver) = channel();
        let (d, t, c) = (done.clone(), total.clone(), cancel.clone());
        std::thread::spawn(move || {
            let result = bpe_export::export_video(&device, &queue, &scene, &options, |frame, frames| {
                d.store(frame, Ordering::Relaxed);
                t.store(frames, Ordering::Relaxed);
                ctx.request_repaint();
                !c.load(Ordering::Relaxed)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.export.message = None;
        self.export.job = Some(ExportJob { done, total, cancel, receiver, started: Instant::now() });
    }
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let typing = ctx.memory(|m| m.focused().is_some());
        if !typing && ctx.input(|i| i.key_pressed(Key::Space)) {
            self.set_playing(!self.playing);
        }
        self.advance_clock();
        self.update_soundtrack(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("transport").show(ui, |ui| {
            ui.add_space(4.0);
            self.transport(ui);
            ui.add_space(2.0);
        });

        let before = self.scene.clone();
        egui::Panel::left("inspector").default_size(380.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                inspector::show(ui, &mut self.scene, &mut self.progression_text);
            });
        });
        if self.scene != before {
            self.scene_changed();
        }

        egui::CentralPanel::default().show(ui, |ui| self.preview(ui, frame));
        self.export_window(&ctx, frame);

        if self.playing || self.export.job.is_some() {
            ctx.request_repaint();
        }
    }
}
