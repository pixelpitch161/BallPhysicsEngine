//! Light tiles (review S19): which lights may reach each tile of the picture.

use super::*;

/// The side of a light tile in the lighting's pixels (review S19).
pub const LIGHT_TILE: u32 = 32;
/// Fewer lights than this are all taken by every pixel: a list per tile isn't worth it.
pub const TILED_FROM: usize = 8;
/// The most light-tile entries a frame lists (past it every pixel takes every light again).
pub(super) const MOST_TILE_ENTRIES: usize = 1 << 22;
/// How far past a light's reach (pixels) its tiles reach: the GPU finds a point's tile in f32.
pub(super) const TILE_MARGIN: f32 = 2.0;

/// Which lights may reach each tile of the picture (review S19): every pixel in a tile then walks
/// only its tile's list, in light order, so the light it sums is exactly as before. Appended to
/// the links buffer after the Lights up links, as f32 (exact up to 2^24): a head per tile (where
/// its list starts, how long it is), then the lists.
#[derive(Debug, Default, PartialEq)]
pub struct LightTiles {
    /// Tiles across and down (0 when there are none).
    pub across: u32,
    pub down: u32,
    /// The heads, then the lists, each index counted from the start of the links buffer, which
    /// holds `links` entries before them.
    pub entries: Vec<f32>,
    /// The pictures' size over a tile's: tiles per unit of uv.
    pub scale: [f32; 2],
}

impl LightTiles {
    /// The lists for `lights` (and their strip `segments`) seen through `camera` (centre xy, zoom,
    /// turn) on a picture `size` pixels big that shows `frame` world units (across, down) at zoom 1;
    /// `links` entries come before them in the buffer. None (every pixel takes every light) with
    /// fewer than [`TILED_FROM`] lights or too many entries.
    pub fn build(
        lights: &[LightGpu],
        segments: &[[f32; 4]],
        camera: [f32; 4],
        size: (u32, u32),
        frame: (f32, f32),
        links: usize,
    ) -> Option<LightTiles> {
        if lights.len() < TILED_FROM || size.0 == 0 || size.1 == 0 || !(frame.0 > 0.0 && frame.1 > 0.0) {
            return None;
        }
        let across = size.0.div_ceil(LIGHT_TILE);
        let down = size.1.div_ceil(LIGHT_TILE);
        let tiles = (across * down) as usize;
        // Pixels a world unit across and down (the zoom included; `world_to_uv` in the lighting shader) and
        // the picture's middle, in pixels. A radius takes the larger.
        let scale = [size.0 as f32 / frame.0 * camera[2], size.1 as f32 / frame.1 * camera[2]];
        let most = scale[0].abs().max(scale[1].abs());
        let half = [size.0 as f32 * 0.5, size.1 as f32 * 0.5];
        let (sin, cos) = (-camera[3]).sin_cos();
        let to_pixel = |p: [f32; 2]| {
            let d = [p[0] - camera[0], p[1] - camera[1]];
            let d = if camera[3] != 0.0 { [d[0] * cos - d[1] * sin, d[0] * sin + d[1] * cos] } else { d };
            [d[0] * scale[0] + half[0], d[1] * scale[1] + half[1]]
        };
        let tile = LIGHT_TILE as f32;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); tiles];
        for (i, l) in lights.iter().enumerate() {
            let kind = l.p0[3];
            let i = i as u32;
            if kind > 1.5 && kind < 2.5 {
                // A sun reaches everywhere.
                for list in &mut lists {
                    list.push(i);
                }
                continue;
            }
            let reach = l.p2[0];
            if reach.is_nan() || reach <= 0.0 {
                // No reach: it lights nothing (`falloff`).
                continue;
            }
            // Circles (world centre, radius) that hold every point it can light.
            let mut circles: Vec<([f32; 2], f32)> = Vec::new();
            let at = [l.p0[0], l.p0[1]];
            if kind > 2.5 && kind < 3.5 {
                let (w, h) = (0.5 * l.p4[0].max(0.0), 0.5 * l.p4[1].max(0.0));
                circles.push((at, reach + (w * w + h * h).sqrt()));
            } else if kind > 3.5 && l.p4[3] >= 1.0 {
                let first = l.p4[2].max(0.0) as usize;
                let n = l.p4[3] as usize;
                if first.saturating_add(n) > segments.len() {
                    // Its segments aren't all there: the shader reads stand-ins, so every tile.
                    for list in &mut lists {
                        list.push(i);
                    }
                    continue;
                }
                for s in segments.iter().skip(first).take(n) {
                    let mid = [(s[0] + s[2]) * 0.5, (s[1] + s[3]) * 0.5];
                    let half_len = 0.5 * ((s[2] - s[0]).powi(2) + (s[3] - s[1]).powi(2)).sqrt();
                    circles.push((mid, reach + half_len));
                }
                if circles.is_empty() {
                    circles.push((at, reach));
                }
            } else {
                circles.push((at, reach));
            }
            for (centre, radius) in circles {
                let c = to_pixel(centre);
                let r = radius * most + TILE_MARGIN;
                if !(c[0].is_finite() && c[1].is_finite() && r.is_finite()) {
                    // Anything odd: listed everywhere, as before the lists.
                    for list in &mut lists {
                        if list.last() != Some(&i) {
                            list.push(i);
                        }
                    }
                    continue;
                }
                let x0 = ((c[0] - r) / tile).floor().max(0.0);
                let y0 = ((c[1] - r) / tile).floor().max(0.0);
                let x1 = ((c[0] + r) / tile).floor().min(across as f32 - 1.0);
                let y1 = ((c[1] + r) / tile).floor().min(down as f32 - 1.0);
                if x1 < x0 || y1 < y0 {
                    continue;
                }
                for ty in y0 as u32..=y1 as u32 {
                    for tx in x0 as u32..=x1 as u32 {
                        // The nearest point of the tile to the circle's middle.
                        let nx = c[0].clamp(tx as f32 * tile, (tx + 1) as f32 * tile);
                        let ny = c[1].clamp(ty as f32 * tile, (ty + 1) as f32 * tile);
                        if (nx - c[0]).powi(2) + (ny - c[1]).powi(2) > r * r {
                            continue;
                        }
                        let list = &mut lists[(ty * across + tx) as usize];
                        // A strip's circles overlap: each light once a tile.
                        if list.last() != Some(&i) {
                            list.push(i);
                        }
                    }
                }
            }
        }
        let listed: usize = lists.iter().map(Vec::len).sum();
        if 2 * tiles + listed > MOST_TILE_ENTRIES {
            return None;
        }
        let mut entries = Vec::with_capacity(2 * tiles + listed);
        let mut start = links + 2 * tiles;
        for list in &lists {
            entries.push(start as f32);
            entries.push(list.len() as f32);
            start += list.len();
        }
        for list in &lists {
            entries.extend(list.iter().map(|&i| i as f32));
        }
        let scale = [size.0 as f32 / tile, size.1 as f32 / tile];
        Some(LightTiles { across, down, entries, scale })
    }

    /// The uniform's `tiles` and `tile_list` for `links` entries before the heads.
    pub(super) fn uniform(tiles: Option<&LightTiles>, links: usize) -> ([f32; 4], [f32; 4]) {
        match tiles {
            Some(t) => ([t.scale[0], t.scale[1], t.across as f32, t.down as f32], [links as f32, 0.0, 0.0, 0.0]),
            None => ([0.0; 4], [0.0; 4]),
        }
    }
}

mod tests {
    use super::*;

    fn point(x: f32, y: f32, reach: f32) -> LightGpu {
        LightGpu { p0: [x, y, 50.0, 0.0], p2: [reach, 0.0, 0.0, 1.0], ..LightGpu::default() }
    }

    /// The lists of a 1080 x 1920 world seen whole on a 270 x 480 picture.
    fn lists_of(lights: &[LightGpu], camera: [f32; 4]) -> (LightTiles, Vec<Vec<u32>>) {
        let tiles = LightTiles::build(lights, &[], camera, (270, 480), (1080.0, 1920.0), 4).expect("tiled");
        let n = (tiles.across * tiles.down) as usize;
        let lists = (0..n)
            .map(|t| {
                let (start, len) = (tiles.entries[2 * t] as usize - 4, tiles.entries[2 * t + 1] as usize);
                tiles.entries[start..start + len].iter().map(|&i| i as u32).collect()
            })
            .collect();
        (tiles, lists)
    }

    #[test]
    fn few_lights_are_not_tiled() {
        let lights = vec![point(0.0, 0.0, 100.0); TILED_FROM - 1];
        assert!(LightTiles::build(&lights, &[], [0.0, 0.0, 1.0, 0.0], (270, 480), (1080.0, 1920.0), 0).is_none());
    }

    #[test]
    fn each_tile_lists_the_lights_that_reach_it_in_order() {
        let mut lights: Vec<LightGpu> =
            (0..TILED_FROM).map(|i| point(-400.0 + 100.0 * i as f32, -600.0, 60.0)).collect();
        // A sun, a light with no reach, and one far off the picture.
        lights.push(LightGpu { p0: [0.0, 0.0, 0.5, 2.0], ..LightGpu::default() });
        lights.push(point(0.0, 0.0, 0.0));
        lights.push(point(5000.0, 0.0, 100.0));
        let sun = TILED_FROM as u32;
        let (tiles, lists) = lists_of(&lights, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!((tiles.across, tiles.down), (270u32.div_ceil(LIGHT_TILE), 480u32.div_ceil(LIGHT_TILE)));
        for (t, list) in lists.iter().enumerate() {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "tile {t}: {list:?}");
            assert!(list.contains(&sun), "the sun is in every tile");
            assert!(!list.contains(&(sun + 1)) && !list.contains(&(sun + 2)));
        }
        // The tile under each lamp lists it; one well away from all of them lists only the sun.
        let tile_at = |x: f32, y: f32| {
            let (px, py) = ((x / 1080.0 + 0.5) * 270.0, (y / 1920.0 + 0.5) * 480.0);
            (py as u32 / LIGHT_TILE * tiles.across + px as u32 / LIGHT_TILE) as usize
        };
        for (i, l) in lights.iter().take(TILED_FROM).enumerate() {
            assert!(lists[tile_at(l.p0[0], l.p0[1])].contains(&(i as u32)));
        }
        assert_eq!(lists[tile_at(0.0, 600.0)], vec![sun]);
    }

    #[test]
    fn a_turned_and_zoomed_camera_moves_the_lists_with_it() {
        let lights: Vec<LightGpu> = (0..TILED_FROM).map(|_| point(300.0, 0.0, 40.0)).collect();
        // Turned a quarter, the lamp to the camera's right shows above or below the middle, not
        // beside it; zoomed in twice, it is twice as far from the middle.
        let (tiles, lists) = lists_of(&lights, [0.0, 0.0, 2.0, std::f32::consts::FRAC_PI_2]);
        let lit: Vec<usize> = (0..lists.len()).filter(|&t| !lists[t].is_empty()).collect();
        assert!(!lit.is_empty());
        let middle_x = tiles.across as f32 * 0.5;
        for t in lit {
            let (tx, ty) = (t as u32 % tiles.across, t as u32 / tiles.across);
            assert!((tx as f32 + 0.5 - middle_x).abs() < 2.0, "tile {tx},{ty} is beside the middle");
            let dy = (ty as f32 + 0.5) * LIGHT_TILE as f32 - 240.0;
            assert!((dy.abs() - 300.0 * 2.0 / 4.0).abs() < 2.0 * LIGHT_TILE as f32, "tile {tx},{ty}");
        }
    }
}
