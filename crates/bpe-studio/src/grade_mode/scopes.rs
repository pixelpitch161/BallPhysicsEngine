//! Grade mode's scopes strip, under the preview: chips for the **Histogram**, **Waveform**, **RGB
//! Parade** and **Vectorscope**, then the chosen one, counted on the GPU from the picture as shown
//! (`bpe_gfx::grade::Scopes`; the preview asks after each redraw). Never part of an export.
//!
//! - Histogram: how many pixels sit at each level of red, green, blue and brightness, darks on the
//!   left, heights square roots so a few bright pixels still show beside a dark picture's mass.
//! - Waveform: each column of the picture's brightness levels, darks at the bottom; RGB Parade
//!   the same for red, green and blue side by side.
//! - Vectorscope: each pixel's colour round the middle (BT.709 Cb across, Cr up), greys in the
//!   middle, with the 75% colour targets and the skin-tone line.
//!
//! A count's bins become a picture once (`picture`): each bin's strength is `ln(1 + n) / ln(1 +
//! r)`, `r` the 99th percentile of the bins that have any, so a big flat background doesn't wash
//! the rest out.

use super::GradeMode;
use crate::app::Studio;
use crate::style;
use bpe_gfx::grade::{HistogramBins, SCOPE_COLUMNS, SCOPE_LEVELS, ScopeCounts, ScopeKind, vectorscope_point};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};

/// The strip's starting height.
pub const STRIP: f32 = 180.0;

/// A scope's name, as its chip and kittest show it.
pub fn name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Histogram => "Histogram",
        ScopeKind::Waveform => "Waveform",
        ScopeKind::Parade => "RGB Parade",
        ScopeKind::Vectorscope => "Vectorscope",
    }
}

fn help(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Histogram => {
            "How many pixels sit at each level of red, green, blue and brightness, darks on the left"
        }
        ScopeKind::Waveform => "The picture's brightness down each column, darks at the bottom",
        ScopeKind::Parade => "Red, green and blue down each column, side by side",
        ScopeKind::Vectorscope => {
            "Each pixel's colour round the middle: greys in the middle, stronger colours further out"
        }
    }
}

/// The strip: chips, then the chosen scope filling the rest.
pub fn strip(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::bottom("grade scopes")
        .frame(
            egui::Frame::new()
                .fill(style::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .stroke(Stroke::new(1.0, style::LINE_SOFT)),
        )
        .resizable(true)
        .default_size(STRIP)
        .min_size(120.0)
        .max_size(360.0)
        .show(ui, |ui| show(ui, &mut studio.grade));
}

/// What the strip draws, for tests without the panel round it.
pub fn show(ui: &mut Ui, mode: &mut GradeMode) {
    ui.set_min_width(ui.available_width());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for kind in ScopeKind::ALL {
            let chip = style::toggle_button(name(kind), mode.scope == kind).small();
            if ui.add(chip).on_hover_text(help(kind)).clicked() && mode.scope != kind {
                mode.scope = kind;
                mode.recount = true;
            }
        }
    });
    ui.add_space(4.0);
    let counts = mode.counts.as_ref().filter(|c| c.kind == mode.scope);
    let room = ui.available_size().max(egui::vec2(60.0, 40.0));
    if mode.scope == ScopeKind::Histogram {
        histogram(ui, counts.and_then(ScopeCounts::histogram).as_ref(), room.y);
        return;
    }
    // The picture is made once a count comes in, not every frame.
    if let Some(c) = counts
        && mode.scope_image.as_ref().is_none_or(|(of, _)| *of != mode.counted)
    {
        let texture = ui.ctx().load_texture("grade scope", picture(c), egui::TextureOptions::LINEAR);
        mode.scope_image = Some((mode.counted, texture));
    }
    let rect = match mode.scope {
        ScopeKind::Vectorscope => {
            let side = room.x.min(room.y);
            let (all, _) = ui.allocate_exact_size(room, Sense::hover());
            Rect::from_center_size(all.center(), egui::vec2(side, side))
        }
        _ => ui.allocate_exact_size(room, Sense::hover()).0,
    };
    let response = ui.interact(rect, ui.scope_id().with("scope"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name(mode.scope)));
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let image = mode.scope_image.as_ref().filter(|_| counts.is_some());
    match image {
        Some((_, texture)) => {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(texture.id(), rect, uv, style::UNTINTED);
        }
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Counting\u{2026}",
                egui::FontId::proportional(11.0),
                style::FAINT,
            );
        }
    }
    match mode.scope {
        ScopeKind::Vectorscope => vectorscope_marks(&painter, rect),
        kind => wave_marks(&painter, rect, if kind == ScopeKind::Parade { 3 } else { 1 }),
    }
}

/// The Histogram, `height` tall.
pub fn histogram(ui: &mut Ui, bins: Option<&HistogramBins>, height: f32) -> egui::Response {
    let width = ui.available_width().max(60.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Histogram"));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    for i in 1..4 {
        painter.vline(rect.left() + rect.width() * i as f32 / 4.0, rect.y_range(), Stroke::new(1.0, style::LINE_SOFT));
    }
    let Some(bins) = bins else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Counting\u{2026}",
            egui::FontId::proportional(11.0),
            style::FAINT,
        );
        return response;
    };
    let inner = rect.shrink2(egui::vec2(1.0, 3.0));
    // The tallest bin of any channel fills the height (a picture of one flat colour still
    // reads).
    let top = bins.iter().flat_map(|c| c.iter()).copied().max().unwrap_or(0).max(1) as f32;
    let height = |n: u32| (n as f32 / top).sqrt() * inner.height();
    let x_of = |i: usize| inner.left() + inner.width() * (i as f32 + 0.5) / 256.0;
    // Brightness as a filled shape, the channels as lines over it.
    let bar = inner.width() / 256.0;
    for (i, &n) in bins[3].iter().enumerate() {
        if n > 0 {
            let x = x_of(i);
            let r = Rect::from_min_max(
                Pos2::new(x - bar * 0.5, inner.bottom() - height(n)),
                Pos2::new(x + bar * 0.5 + 0.5, inner.bottom()),
            );
            painter.rect_filled(r, 0.0, style::CHANNEL_LUMA);
        }
    }
    for (c, ink) in [(0, style::CHANNEL_RED), (1, style::CHANNEL_GREEN), (2, style::CHANNEL_BLUE)] {
        let line: Vec<Pos2> =
            bins[c].iter().enumerate().map(|(i, &n)| Pos2::new(x_of(i), inner.bottom() - height(n))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.0, ink)));
    }
    response
}

/// The level bins at the 99th percentile of the bins that have any (at least 1).
pub fn reference(counts: &[u32]) -> f64 {
    let mut used: Vec<u32> = counts.iter().copied().filter(|&n| n > 0).collect();
    if used.is_empty() {
        return 1.0;
    }
    let at = ((used.len() - 1) as f64 * 0.99).round() as usize;
    let (_, nth, _) = used.select_nth_unstable(at);
    f64::from(*nth).max(1.0)
}

/// How strongly a bin of `n` pixels shows, 0..1, against reference `r`.
pub fn strength(n: u32, r: f64) -> f32 {
    if n == 0 {
        return 0.0;
    }
    ((1.0 + f64::from(n)).ln() / (1.0 + r).ln().max(f64::MIN_POSITIVE)).min(1.0) as f32
}

/// A count's picture: Waveform 256 × 256 and Parade 768 × 256 (whites at the top), Vectorscope
/// 256 × 256 (Cr up). The Histogram draws itself and has none (an empty picture).
pub fn picture(c: &ScopeCounts) -> egui::ColorImage {
    let r = reference(&c.counts);
    let (w, h) = (SCOPE_COLUMNS, SCOPE_LEVELS);
    match c.kind {
        ScopeKind::Histogram => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
        ScopeKind::Waveform => {
            let pixels = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, h - 1 - y)))
                .map(|(x, level)| style::SCOPE_TRACE.gamma_multiply(strength(c.wave(0, level, x), r)))
                .collect();
            egui::ColorImage::new([w, h], pixels)
        }
        ScopeKind::Parade => {
            let inks = [style::CHANNEL_RED, style::CHANNEL_GREEN, style::CHANNEL_BLUE];
            let pixels = (0..h)
                .flat_map(|y| (0..3 * w).map(move |x| (x, h - 1 - y)))
                .map(|(x, level)| inks[x / w].gamma_multiply(strength(c.wave(x / w, level, x % w), r)))
                .collect();
            egui::ColorImage::new([3 * w, h], pixels)
        }
        ScopeKind::Vectorscope => {
            let pixels = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, h - 1 - y)))
                .map(|(u, v)| {
                    let n = c.counts.get(v * w + u).copied().unwrap_or(0);
                    style::scope_hue(chroma_colour(u as f64, v as f64)).gamma_multiply(strength(n, r))
                })
                .collect();
            egui::ColorImage::new([w, h], pixels)
        }
    }
}

/// The colour (sRGB-encoded) at Vectorscope bin `(u, v)`, at a middling brightness.
fn chroma_colour(u: f64, v: f64) -> [f64; 3] {
    let (cb, cr) = (u / 255.0 - 0.5, v / 255.0 - 0.5);
    let y = 0.6;
    let r = y + 1.5748 * cr;
    let b = y + 1.8556 * cb;
    let g = (y - 0.2126 * r - 0.0722 * b) / 0.7152;
    [r, g, b]
}

/// Waveform and Parade lines at 0, 25, 50, 75 and 100%, and between the parade's thirds.
fn wave_marks(painter: &egui::Painter, rect: Rect, parts: usize) {
    for i in 0..=4 {
        let y = rect.bottom() - rect.height() * i as f32 / 4.0;
        painter.hline(rect.x_range(), y, Stroke::new(1.0, style::LINE_SOFT));
    }
    for (i, words) in [(0, "0"), (2, "50"), (4, "100")] {
        let y = rect.bottom() - rect.height() * i as f32 / 4.0;
        let at = Pos2::new(rect.left() + 3.0, y.clamp(rect.top() + 6.0, rect.bottom() - 6.0));
        painter.text(at, egui::Align2::LEFT_CENTER, words, egui::FontId::proportional(11.0), style::FAINT);
    }
    for k in 1..parts {
        let x = rect.left() + rect.width() * k as f32 / parts as f32;
        painter.vline(x, rect.y_range(), Stroke::new(1.0, style::LINE));
    }
}

/// The Vectorscope's ring, cross, colour targets (75%) and skin-tone line.
fn vectorscope_marks(painter: &egui::Painter, rect: Rect) {
    let c = rect.center();
    let radius = rect.width() * 0.5;
    let line = Stroke::new(1.0, style::LINE_SOFT);
    painter.circle_stroke(c, radius - 1.0, line);
    painter.hline(rect.x_range(), c.y, line);
    painter.vline(c.x, rect.y_range(), line);
    let place = |(u, v): (f64, f64)| {
        Pos2::new(rect.left() + rect.width() * (u / 255.0) as f32, rect.bottom() - rect.height() * (v / 255.0) as f32)
    };
    for (colour, words) in TARGETS {
        let at = place(vectorscope_point(colour.map(|x| x * 0.75)));
        painter.rect_stroke(Rect::from_center_size(at, egui::vec2(7.0, 7.0)), 0.0, line, egui::StrokeKind::Middle);
        painter.text(
            at + egui::vec2(6.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            words,
            egui::FontId::proportional(11.0),
            style::FAINT,
        );
    }
    let skin = SKIN_LINE.to_radians();
    let end = c + egui::vec2(skin.cos(), -skin.sin()) * (radius - 2.0);
    painter.line_segment([c, end], Stroke::new(1.0, style::LINE));
}

/// The colour targets (sRGB-encoded, full strength) and their initials.
const TARGETS: [([f64; 3], &str); 6] = [
    ([1.0, 0.0, 0.0], "R"),
    ([1.0, 1.0, 0.0], "Yl"),
    ([0.0, 1.0, 0.0], "G"),
    ([0.0, 1.0, 1.0], "Cy"),
    ([0.0, 0.0, 1.0], "B"),
    ([1.0, 0.0, 1.0], "Mg"),
];

/// The skin-tone line's angle (degrees anticlockwise from +Cb).
pub const SKIN_LINE: f32 = 123.0;
