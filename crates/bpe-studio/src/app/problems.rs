//! Problems told in the status line: the graphics card's complaints and export news.

use super::*;

impl Studio {
    /// Sends the window's graphics card complaints to the status line instead of letting wgpu
    /// close the editor.
    pub(super) fn catch_gpu_errors(&self, device: &egui_wgpu::wgpu::Device, ctx: &egui::Context) {
        let (errors, repaint) = (self.gpu_errors.clone(), ctx.clone());
        device.on_uncaptured_error(Arc::new(move |error| {
            let mut errors = errors.lock().unwrap_or_else(|e| e.into_inner());
            errors.first.get_or_insert_with(|| error.to_string());
            errors.count += 1;
            repaint.request_repaint();
        }));
        let (errors, repaint) = (self.gpu_errors.clone(), ctx.clone());
        device.set_device_lost_callback(move |reason, _| {
            // Destroyed is the editor's own shutdown.
            if reason != egui_wgpu::wgpu::DeviceLostReason::Destroyed {
                errors.lock().unwrap_or_else(|e| e.into_inner()).lost = true;
                repaint.request_repaint();
            }
        });
    }

    /// Shows a failure in the status line until it is dismissed or its source clears it.
    pub fn report(&mut self, kind: ProblemKind, text: impl Into<String>) {
        self.problem = Some(Problem { kind, text: text.into() });
    }

    /// Clears the problem in the status line if it came from `kind`.
    pub fn clear_problem(&mut self, kind: ProblemKind) {
        if self.problem.as_ref().is_some_and(|p| p.kind == kind) {
            self.problem = None;
        }
    }

    /// Tells the status line what the graphics card complained about since the last frame.
    pub(super) fn tell_gpu_errors(&mut self) {
        let (first, count, lost) = {
            let errors = self.gpu_errors.lock().unwrap_or_else(|e| e.into_inner());
            (errors.first.clone(), errors.count, errors.lost)
        };
        if lost && !self.gpu_lost_told {
            self.gpu_lost_told = true;
            // Save for recovery now: drawing is gone, the scene isn't.
            self.autosaved_at = None;
            self.report(
                ProblemKind::Graphics,
                "The graphics card stopped responding. Your unsaved work was autosaved; save it if you can, then \
                 close and reopen Ball Studio.",
            );
        } else if count > self.gpu_told
            && !self.gpu_lost_told
            && let Some(first) = first
        {
            let line = first.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("unknown").to_string();
            let times = if count > 1 { format!(" ({count} times)") } else { String::new() };
            let text = format!("The graphics card reported a problem{times}: {line}");
            if self.gpu_told == 0 {
                // Told once; a complaint that repeats every frame only counts up in place, so it can
                // be dismissed and never hides another problem.
                self.report(ProblemKind::Graphics, text);
            } else if let Some(problem) = self.problem.as_mut().filter(|p| p.kind == ProblemKind::Graphics) {
                problem.text = text;
            }
        }
        self.gpu_told = count;
    }

    /// Puts how the last export ended in the status line.
    pub(super) fn tell_export_news(&mut self) {
        match self.panels.export.take_news() {
            Some(crate::export_window::ExportNews::Saved { name, seconds }) => {
                self.status = format!("Video saved: {name} ({}).", crate::export_window::minutes(seconds));
                self.clear_problem(ProblemKind::Export);
            }
            Some(crate::export_window::ExportNews::Failed { headline }) => {
                self.report(ProblemKind::Export, format!("The export failed. {headline}"));
            }
            Some(crate::export_window::ExportNews::Cancelled) => self.status = "Export cancelled.".into(),
            None => {}
        }
    }
}
