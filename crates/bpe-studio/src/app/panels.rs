//! The window's panels: the toolbar, the left panel (tree, Mixer, Lights), the inspector's
//! panel, the grid controls and the status line.

use super::*;

/// The three chips on top of the left panel.
pub(super) fn left_switch(ui: &mut egui::Ui, left: &mut LeftPanel) {
    ui.horizontal(|ui| {
        // Snug, so the three fit the panel at its narrowest.
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.spacing_mut().button_padding.x = 6.0;
        for (panel, name) in [(LeftPanel::Things, "Things"), (LeftPanel::Lights, "Lights"), (LeftPanel::Mixer, "Mixer")]
        {
            let hint = match panel {
                LeftPanel::Things => "The things in the scene",
                LeftPanel::Lights => "Every light in the scene: strength, mute and solo",
                LeftPanel::Mixer => "Tempo, the Master and the meters",
            };
            if ui.add(style::toggle_button(name, *left == panel)).on_hover_text(hint).clicked() {
                *left = panel;
            }
        }
    });
    ui.add_space(6.0);
}

/// The toolbar's frame: the panel grey with a soft line under it.
pub(super) fn bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(style::PANEL)
        .inner_margin(egui::Margin::symmetric(8, 7))
        .stroke(Stroke::new(1.0, style::LINE_SOFT))
}

/// The tree's and the inspector's frame: the panel grey, touching its neighbours with a soft line.
pub(super) fn side_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(style::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .stroke(Stroke::new(1.0, style::LINE_SOFT))
}

/// How wide the toolbar's save status may be.
pub(super) const SAVE_STATUS_WIDTH: f32 = 180.0;

/// The narrowest the save status is cut to before it goes to the toolbar's next line.
pub(super) const MIN_SAVE_STATUS_WIDTH: f32 = 90.0;

impl Studio {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        // Export sits at the right, as wide as it was last drawn; everything else gets the
        // room left of it and wraps onto a second line rather than running under it.
        let right_id = ui.make_persistent_id("toolbar right");
        let right_width = ui.ctx().data(|d| d.get_temp::<f32>(right_id)).unwrap_or(SAVE_STATUS_WIDTH);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let left = egui::vec2((ui.available_width() - right_width).max(0.0), 0.0);
            let wrap = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
            ui.allocate_ui_with_layout(left, wrap, |ui| {
                ui.set_width(left.x);
                self.toolbar_left(ui);
            });
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let export = style::primary_button("Export video…").fill(style::TEXT);
                if ui.add(export).on_hover_text("Render the scene to an mp4").clicked() {
                    self.clicked = Some(ToolbarClick::Export);
                }
            });
            // The gap before Export counts as part of the right side.
            let width = right.response.rect.width() + ui.spacing().item_spacing.x;
            if (width - right_width).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(right_id, width));
                ui.ctx().request_discard("the toolbar's right side changed width");
            }
        });
    }

    /// The toolbar's buttons left of Export, ending with the scene's name and its save status.
    pub(super) fn toolbar_left(&mut self, ui: &mut egui::Ui) {
        if style::toolbar_button(ui, "New")
            .on_hover_text("Start a new blank scene with just a camera (Ctrl+N)")
            .clicked()
        {
            self.clicked = Some(ToolbarClick::New);
        }
        if style::toolbar_button(ui, "Open…").on_hover_text("Open a saved scene (Ctrl+O)").clicked() {
            self.clicked = Some(ToolbarClick::Open);
        }
        ui.menu_button("Recent", |ui| {
            let recent = settings::recent_scenes();
            if recent.is_empty() {
                ui.label("No scenes opened yet");
            }
            for path in recent {
                if ui.button(file_label(&path)).on_hover_text(path.display().to_string()).clicked() {
                    self.clicked = Some(ToolbarClick::Recent(path));
                }
            }
        })
        .response
        .on_hover_text("Open a scene you opened or saved lately");
        if style::toolbar_button(ui, "Save").on_hover_text("Save this scene (Ctrl+S)").clicked() {
            self.clicked = Some(ToolbarClick::Save);
        }
        if style::toolbar_button(ui, "Save as…")
            .on_hover_text("Save this scene under a new name (Ctrl+Shift+S)")
            .clicked()
        {
            self.clicked = Some(ToolbarClick::SaveAs);
        }
        style::bar_separator(ui);
        let undo = egui::Button::new("Undo").frame_when_inactive(false);
        if ui.add_enabled(self.history.can_undo(), undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
            self.clicked = Some(ToolbarClick::Undo);
        }
        let redo = egui::Button::new("Redo").frame_when_inactive(false);
        if ui.add_enabled(self.history.can_redo(), redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
            self.clicked = Some(ToolbarClick::Redo);
        }
        style::bar_separator(ui);
        self.grid_controls(ui);
        if self.particle_job.is_some() {
            let air = ui.add(style::toggle_button("Air", self.show_air)).on_hover_text(
                "Show air: faint arrows of which way the air goes at the playhead, for particles that \
                 follow the air. They never show in the video.",
            );
            if air.clicked() {
                self.show_air = !self.show_air;
            }
        }
        style::bar_separator(ui);
        ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(170.0).hint_text("Scene name"))
            .on_hover_text("The scene's name. Exported videos are named after it.");
        let saved_label = match (&self.path, self.unsaved()) {
            (None, _) => RichText::new("Not saved yet").color(style::MUTED),
            (Some(_), true) => RichText::new("• Unsaved changes").color(style::TEXT),
            (Some(path), false) => RichText::new(format!("Saved as {}", file_label(path))).color(style::MUTED),
        };
        // A slot of its own, so a long file name is cut short; with too little room on this
        // line it goes to the next.
        let full = saved_label.text().to_owned();
        // What is left of this line (a wrapping layout's available width is a whole line).
        let room = ui.max_rect().right() - ui.cursor().left();
        let width = if room >= MIN_SAVE_STATUS_WIDTH { room.min(SAVE_STATUS_WIDTH) } else { SAVE_STATUS_WIDTH };
        let status = ui
            .allocate_ui_with_layout(
                egui::vec2(width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    ui.add(egui::Label::new(saved_label.small()).truncate().show_tooltip_when_elided(false))
                },
            )
            .inner;
        status.on_hover_text(full);
    }

    /// The inspector on the right, as wide as the user last dragged it. Returns its width.
    /// The left panel, Things or Mixer, with the switch on top. The tree keeps its width; the
    /// Mixer opens at the tree's width each time it is switched to, and can be dragged wider
    /// while open. Returns the width it was drawn at.
    pub(crate) fn left_panel(&mut self, ui: &mut egui::Ui, before: &Scene) -> f32 {
        let mut left = self.left;
        let width = match self.left {
            LeftPanel::Things => {
                let shown = egui::Panel::left("tree")
                    .default_size(style::TREE_W)
                    .min_size(180.0)
                    .max_size(480.0)
                    .resizable(true)
                    .frame(side_frame().inner_margin(egui::Margin::symmetric(8, 10)))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        left_switch(ui, &mut left);
                        self.panels.tree.paste_words = self.clipboard.things_words();
                        tree::show(
                            ui,
                            &mut self.panels.tree,
                            &mut self.scene,
                            &self.kit,
                            &self.resolved,
                            &mut self.selection,
                        )
                    });
                let tree = shown.inner;
                if let Some(message) = tree.message {
                    self.status = message;
                }
                if let Some(ids) = &tree.copy {
                    self.copy_things(ids);
                }
                if let Some(at) = tree.paste {
                    self.paste_things(Some(at));
                }
                if tree.picked {
                    // A thing picked in the tree is what Ctrl+C copies now, not the timeline's keys.
                    self.panels.timeline.selected.clear();
                }
                self.separate_edit |= tree.separate;
                if let Some(id) = tree.added {
                    self.place_added(id);
                    if tree.looks {
                        self.panels.inspector.tab.insert(id, crate::inspector::InspectorTab::Looks);
                    }
                }
                if tree.removed.is_some() {
                    // Deleting is always its own undo step.
                    self.recorded = true;
                    self.after_separate_edit(before.clone());
                }
                self.tree_width = shown.response.rect.width();
                self.tree_width
            }
            LeftPanel::Lights => {
                let shown = egui::Panel::left("lights")
                    .default_size(self.tree_width)
                    .min_size(180.0)
                    .max_size(640.0)
                    .resizable(true)
                    .frame(side_frame().inner_margin(egui::Margin::symmetric(8, 10)))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        left_switch(ui, &mut left);
                        crate::lights_panel::show(ui, self);
                    });
                shown.response.rect.width()
            }
            LeftPanel::Mixer => {
                let meters = self.sound.as_ref().map(SoundJob::meters);
                let reading = meters.as_ref().map(|m| m.read());
                let shown = egui::Panel::left(("mixer", self.mixer_opens))
                    .default_size(self.tree_width)
                    .min_size(180.0)
                    .max_size(960.0)
                    .resizable(true)
                    .frame(side_frame().inner_margin(egui::Margin::symmetric(10, 10)))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        left_switch(ui, &mut left);
                        let keyed = mixer::show(
                            ui,
                            &mut self.panels.mixer,
                            &mut self.scene,
                            &self.kit,
                            &self.resolved,
                            reading.as_ref(),
                            meters.as_deref(),
                            self.time,
                        );
                        if keyed {
                            self.panels.inspector.keyed_edit = true;
                        }
                    });
                if self.playing {
                    // The meters move while the sound plays.
                    ui.ctx().request_repaint();
                }
                shown.response.rect.width()
            }
        };
        if left == LeftPanel::Mixer && self.left != LeftPanel::Mixer {
            self.mixer_opens += 1;
        }
        self.left = left;
        width
    }

    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) -> f32 {
        // A Sound tab is wide (Osc A and Osc B side by side), and so is a Looks tab, with a width of their own:
        // a panel of its own id keeps each width as it was left.
        // (The Mixer's editors open in the Mixer itself: the inspector always shows the selection.)
        let sound = self.panels.inspector.wide_open(&self.scene, self.selection.primary);
        let (id, width, range) = if sound {
            ("inspector sound", self.sound_width, settings::SOUND_WIDTH_RANGE)
        } else {
            ("inspector", self.inspector_width, settings::INSPECTOR_RANGE)
        };
        if sound != self.inspector_sound {
            // The other width shows from this frame: not a drag of the edge.
            self.inspector_sound = sound;
            self.inspector_seen = width;
            self.inspector_resizing = false;
        }
        let panel = egui::Panel::right(id)
            .default_size(width)
            .min_size(*range.start())
            .max_size(*range.end())
            .resizable(true)
            .frame(side_frame())
            .show(ui, |ui| {
                // Claim the panel's whole width, or it shrinks to its widest row on the first frame.
                ui.set_min_width(ui.available_width());
                // The contents go in a child that only reports the panel's own rect, so a row that
                // is too wide for a frame (a grid's first frame lays out unmeasured) is clipped
                // instead of widening the panel for good.
                let rect = ui.available_rect_before_wrap();
                let mut contents = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                contents.shrink_clip_rect(rect);
                let time = self.sim_time();
                self.panels.inspector.meters = self.sound.as_ref().map(SoundJob::meters);
                self.panels.inspector.playing = self.playing;
                self.fill_rounds();
                match self.picked_block() {
                    Some(picked) => {
                        let runtime =
                            self.playback.world().rules().and_then(|r| r.problem(picked.0)).map(str::to_owned);
                        crate::rules_panel::inspect::show(
                            &mut contents,
                            &mut self.scene,
                            &self.kit,
                            picked,
                            runtime.as_deref(),
                        );
                    }
                    None => {
                        inspector::show(
                            &mut contents,
                            &mut self.panels.inspector,
                            &mut self.scene,
                            &self.kit,
                            &self.resolved,
                            &self.selection,
                            time,
                        );
                    }
                }
                ui.advance_cursor_after_rect(rect);
            });
        let shown = panel.response.rect.width();
        // Only a drag of the edge is remembered, once the mouse lets go; a window squeezing the
        // panel is not.
        let down = ui.input(|i| i.pointer.any_down());
        if down && (shown - self.inspector_seen).abs() >= 0.5 {
            self.inspector_resizing = true;
        }
        self.inspector_seen = shown;
        if !down && std::mem::take(&mut self.inspector_resizing) && (shown - width).abs() >= 0.5 {
            if sound {
                self.sound_width = shown;
                if self.persist_settings {
                    settings::save_sound_width(shown);
                }
            } else {
                self.inspector_width = shown;
                if self.persist_settings {
                    settings::save_inspector_width(shown);
                }
            }
        }
        shown
    }

    /// The rule block the inspector shows: the one picked in the Rules view, while no thing is
    /// selected and the block is still there.
    pub fn picked_block(&self) -> Option<(bpe_core::RuleId, bpe_core::BlockId)> {
        let (r, b) = self.panels.rules.picked?;
        (self.selection.primary.is_none() && self.scene.rules.rule(r).and_then(|r| r.block(b)).is_some())
            .then_some((r, b))
    }

    /// The grid buttons: show the lines, snap drags to them, and the size of one square.
    pub(super) fn grid_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.grid;
        let grid = ui
            .add(style::toggle_button("Grid", self.grid.show))
            .on_hover_text("Show grid lines over the preview. They never show in the video.");
        if grid.clicked() {
            self.grid.show = !self.grid.show;
        }
        let snap = ui.add(style::toggle_button("Snap", self.grid.snap)).on_hover_text(
            "Drags land on the grid: moving, moulding shapes and aiming in the preview, and distance \
             and angle settings dragged in the inspector. Hold Alt to drag freely.",
        );
        if snap.clicked() {
            self.grid.snap = !self.grid.snap;
        }
        style::wells(ui, |ui| {
            ui.add(
                egui::DragValue::new(&mut self.grid.spacing)
                    .range(grid::SPACING)
                    .speed(1.0)
                    .max_decimals(0)
                    .suffix(" px"),
            )
        })
        .labelled_by(grid.id)
        .on_hover_text("The size of one grid square, in video pixels");
        style::wells(ui, |ui| {
            ui.add(
                egui::DragValue::new(&mut self.grid.angle).range(grid::ANGLES).speed(0.2).max_decimals(0).suffix("°"),
            )
        })
        .labelled_by(snap.id)
        .on_hover_text("The angle step that angle settings snap to");
        if self.grid != before && self.persist_settings {
            self.grid.save();
        }
    }

    pub(super) fn status_line(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            if let Some(problem) = &self.problem {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, style::PROBLEM);
                let text = RichText::new(&problem.text).small().color(style::PROBLEM);
                let width = (ui.available_width() * 0.6).max(120.0);
                ui.scope(|ui| {
                    ui.set_max_width(width);
                    ui.add(egui::Label::new(text).truncate()).on_hover_text(&problem.text);
                });
                let dismiss = ui
                    .add(egui::Button::new(RichText::new("×").color(style::PROBLEM)).frame(false).small())
                    .on_hover_text("Dismiss");
                dismiss.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Dismiss"));
                if dismiss.clicked() {
                    self.problem = None;
                }
                ui.label(RichText::new("·").small().color(style::FAINT));
            }
            ui.label(RichText::new(&self.status).small().color(style::MUTED));
            if let Some(problem) = &self.shader_problem {
                ui.label(RichText::new(format!("· Shader skipped: {problem}")).small().color(style::PROBLEM));
            }
            if let Some(percent) = self.panels.export.progress_percent() {
                let exporting = ui
                    .add(
                        egui::Label::new(RichText::new(format!("· Exporting {percent}%")).small().color(style::TEXT))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Show the export window");
                if exporting.clicked() {
                    self.panels.export.open = true;
                }
            }
            if let Some((lights, ms)) = self.lighting {
                ui.label(RichText::new(lighting_readout(lights, ms)).small().color(style::MUTED));
            }
            // Decoration from the particles thread, and two-way particles in the main world.
            let beside = self.particle_job.as_ref().and_then(|job| job.latest()).map(|p| (p.count, p.step_ms));
            let main = main_particles(self.playback.world());
            if beside.is_some() || main.is_some() {
                let (a, b) = (beside.unwrap_or_default(), main.unwrap_or_default());
                ui.label(RichText::new(particles_readout(a.0 + b.0, a.1 + b.1)).small().color(style::MUTED));
            }
        });
    }
}
