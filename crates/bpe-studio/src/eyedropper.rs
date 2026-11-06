//! Pick from picture: a gradient's tool asks for a colour, the preview turns into an eyedropper,
//! and a click on the picture hands back the colour drawn there (the renderer's output, read
//! back), for the gradient row that asked to put on its selected stop.
//!
//! The state lives in egui's memory under one id, so the inspector (which asks and takes) and
//! the preview (which picks) need nothing of each other.

use bpe_core::Color;
use eframe::egui;

/// A picked colour older than this many passes is thrown away rather than taken: its row was
/// gone when it came, and it must not land on a stop later.
pub const STALE_PASSES: u64 = 4;

/// Which row is picking (the id its gradient bar keeps its memory under), and the last pass that
/// row was drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Picking(egui::Id, u64);

/// A colour picked for a row, and the pass it came in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Picked {
    target: egui::Id,
    color: Color,
    pass: u64,
}

fn picking_id() -> egui::Id {
    egui::Id::unique("gradient eyedropper picking")
}

fn picked_id() -> egui::Id {
    egui::Id::unique("gradient eyedropper picked")
}

/// Turns the preview into an eyedropper for the gradient row whose bar keeps its memory under
/// `target`.
pub fn start(ctx: &egui::Context, target: egui::Id) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(picking_id(), Picking(target, pass)));
}

/// Notes that the gradient row `target` is drawn this pass, so its picking (if it is picking)
/// carries on.
pub fn seen(ctx: &egui::Context, target: egui::Id) {
    if self::target(ctx) == Some(target) {
        start(ctx, target);
    }
}

/// Whether the row picking was drawn in this pass or the one before. A row gone from the
/// inspector (another thing selected, another tab) can't take a colour, so its picking ends.
pub fn alive(ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Picking>(picking_id())).is_some_and(|p| now.saturating_sub(p.1) <= 1)
}

/// Stops picking.
pub fn stop(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Picking>(picking_id()));
}

/// The row picking, if any.
pub fn target(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|d| d.get_temp::<Picking>(picking_id())).map(|p| p.0)
}

/// Hands `color` to the row picking and stops picking. Nothing happens when no row is picking.
pub fn deliver(ctx: &egui::Context, color: Color) {
    let Some(target) = target(ctx) else { return };
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(picked_id(), Picked { target, color, pass }));
    stop(ctx);
    ctx.request_repaint();
}

/// The colour picked for the row `target`, once. A colour for another row is left for it; one
/// that waited too long is thrown away.
pub fn take(ctx: &egui::Context, target: egui::Id) -> Option<Color> {
    let picked = ctx.data(|d| d.get_temp::<Picked>(picked_id()))?;
    if ctx.cumulative_pass_nr().saturating_sub(picked.pass) > STALE_PASSES {
        ctx.data_mut(|d| d.remove::<Picked>(picked_id()));
        return None;
    }
    if picked.target != target {
        return None;
    }
    ctx.data_mut(|d| d.remove::<Picked>(picked_id()));
    Some(picked.color)
}

/// The colour at `uv` (0 to 1 across and down) of a picture `size` pixels big held as tightly
/// packed RGBA bytes, opaque. `None` outside the picture or when the bytes are short.
pub fn pixel_at(pixels: &[u8], (width, height): (u32, u32), uv: egui::Vec2) -> Option<Color> {
    if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) || width == 0 || height == 0 {
        return None;
    }
    let x = ((uv.x * width as f32) as u32).min(width - 1) as usize;
    let y = ((uv.y * height as f32) as u32).min(height - 1) as usize;
    let at = (y * width as usize + x) * 4;
    let p = pixels.get(at..at + 3)?;
    Some(Color::rgb(p[0], p[1], p[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_at_reads_the_pixel_under_the_point() {
        // 2 × 2: red, green / blue, white.
        let pixels = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128];
        let at = |x: f32, y: f32| pixel_at(&pixels, (2, 2), egui::vec2(x, y));
        assert_eq!(at(0.0, 0.0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(at(0.99, 0.2), Some(Color::rgb(0, 255, 0)));
        assert_eq!(at(0.2, 0.7), Some(Color::rgb(0, 0, 255)));
        assert_eq!(at(1.0, 1.0), Some(Color::rgb(255, 255, 255)), "the far edge is the last pixel, opaque");
        assert_eq!(at(-0.1, 0.5), None);
        assert_eq!(at(0.5, 1.5), None);
        assert_eq!(pixel_at(&pixels[..8], (2, 2), egui::vec2(0.9, 0.9)), None, "short bytes");
    }

    #[test]
    fn a_picked_colour_goes_to_its_row_once() {
        let ctx = egui::Context::default();
        let (row, other) = (egui::Id::unique("row"), egui::Id::unique("other"));
        deliver(&ctx, Color::rgb(1, 2, 3));
        assert_eq!(take(&ctx, row), None, "nobody was picking");
        start(&ctx, row);
        assert_eq!(target(&ctx), Some(row));
        deliver(&ctx, Color::rgb(1, 2, 3));
        assert_eq!(target(&ctx), None, "a pick ends picking");
        assert_eq!(take(&ctx, other), None, "not for another row");
        assert_eq!(take(&ctx, row), Some(Color::rgb(1, 2, 3)));
        assert_eq!(take(&ctx, row), None, "taken once");
    }

    #[test]
    fn picking_lives_while_its_row_is_drawn() {
        let ctx = egui::Context::default();
        let row = egui::Id::unique("row");
        let pass = |ctx: &egui::Context| {
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        };
        start(&ctx, row);
        assert!(alive(&ctx));
        for _ in 0..3 {
            pass(&ctx);
            seen(&ctx, row);
            assert!(alive(&ctx), "drawn every pass");
        }
        pass(&ctx);
        pass(&ctx);
        assert!(!alive(&ctx), "its row has gone");
        seen(&ctx, egui::Id::unique("other"));
        assert!(!alive(&ctx), "another row doesn't keep it going");
    }

    #[test]
    fn a_colour_left_too_long_is_thrown_away() {
        let ctx = egui::Context::default();
        let row = egui::Id::unique("row");
        start(&ctx, row);
        deliver(&ctx, Color::rgb(9, 9, 9));
        for _ in 0..=STALE_PASSES {
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        }
        assert_eq!(take(&ctx, row), None);
    }
}
