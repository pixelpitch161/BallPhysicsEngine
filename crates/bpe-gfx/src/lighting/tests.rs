use super::*;

#[test]
fn a_preview_gets_a_field_as_fine_as_the_video() {
    // The video itself: the Quality's own divisor (Draft 4, Good 2, Ultra 1).
    assert_eq!(field_divisor(4, 1080, 1080), 4);
    assert_eq!(field_divisor(2, 1080, 1080), 2);
    assert_eq!(field_divisor(1, 1080, 1080), 1);
    // A half-size preview: half the divisor, so a texel covers the same part of the scene.
    assert_eq!(field_divisor(4, 540, 1080), 2);
    assert_eq!(field_divisor(2, 540, 1080), 1);
    // Smaller still, or bigger than the video: never under 1, never coarser than the Quality.
    assert_eq!(field_divisor(2, 270, 1080), 1);
    assert_eq!(field_divisor(2, 2160, 1080), 2);
    assert_eq!(field_divisor(4, 810, 1080), 3);
    // In between, the finer: a texel never covers more of the scene than in the video.
    assert_eq!(field_divisor(4, 700, 1080), 2);
    assert_eq!(field_divisor(4, 405, 1080), 1);
    assert_eq!(field_divisor(2, 1000, 1080), 1);
}

fn ball(x: f32, y: f32, r: f32) -> SphereGpu {
    SphereGpu { a: [x, y, r, 0.0], b: [1.0; 4], c: [0.0; 4] }
}

#[test]
fn the_tint_pass_and_the_lighting_agree_on_thickness() {
    let tint = include_str!("../shaders/tint.wgsl");
    let lighting = crate::LIGHTING_WGSL;
    let value = |text: &str, name: &str| -> f32 {
        let line = text.lines().find(|l| l.starts_with(&format!("const {name}: f32 = "))).expect(name);
        line.split('=').nth(1).unwrap().trim().trim_end_matches(';').parse().unwrap()
    };
    assert_eq!(value(tint, "THICK"), value(lighting, "THICK_DEPTH"));
}

#[test]
fn the_rainbow_sits_opposite_the_brightest_sun_or_where_it_is_put() {
    let mut scene = LightScene::default();
    scene.settings.rainbow = [1.0, 500.0, 0.4, 0.5];
    // No sun: straight down from the middle.
    assert_eq!(rainbow_centre(&scene, [10.0, 20.0]), [10.0, 20.0 + 650.0]);
    let sun = |bright: f32, way: [f32; 2]| LightGpu {
        p0: [0.0, 0.0, 0.0, 2.0],
        p1: [bright, bright, bright, 0.0],
        p3: [way[0], way[1], 0.0, 0.0],
        ..LightGpu::default()
    };
    scene.lights = vec![sun(0.5, [0.0, -1.0]), sun(2.0, [3.0, 4.0])];
    let c = rainbow_centre(&scene, [0.0, 0.0]);
    assert!((c[0] - 390.0).abs() < 1e-3 && (c[1] - 520.0).abs() < 1e-3, "{c:?}");
    scene.settings.rainbow_at = [-100.0, 50.0, 1.0, 0.0];
    assert_eq!(rainbow_centre(&scene, [0.0, 0.0]), [-100.0, 50.0]);
    assert_eq!(rainbow_of(&[0.0, 500.0, 0.4, 0.5]), [0.0; 4], "Rainbow 0 draws none");
    assert_eq!(rainbow_of(&[f32::NAN, 500.0, 0.4, 0.5]), [0.0; 4]);
    assert_eq!(rainbow_of(&[1.0, 0.0, 2.0, -1.0]), [1.0, 1.0, 2.0, 0.0]);
}

#[test]
fn dichroic_surfaces_are_listed_for_the_shadows() {
    let mut plain = SurfaceGpu::MATTE;
    let (_, n) = dichroic_list(&[plain; 3]);
    assert_eq!(n, 0);
    plain.g = [0.6, 0.25, 0.5, 0.0];
    assert_eq!(dichroic_list(&[plain; 3]).1, 0, "an opaque one colours no shadow");
    plain.a[3] = 0.5;
    let mut bending = plain;
    bending.e[3] = 1.5;
    assert_eq!(dichroic_list(&[bending; 3]).1, 0, "nor does glass that bends (its photons do)");
    let (list, n) = dichroic_list(&[SurfaceGpu::MATTE, SurfaceGpu::MATTE, plain]);
    assert_eq!(n, 1);
    assert_eq!(list[0], [2.0, 0.6, 0.25, 0.5]);
    let (_, n) = dichroic_list(&vec![plain; 40]);
    assert_eq!(n, MOST_DICHROIC);
}

#[test]
fn an_empty_grid_has_one_empty_cell() {
    let g = SphereGrid::build(&[]);
    assert_eq!(g.cells, vec![[0, 0]]);
    assert!(g.items.is_empty());
}

#[test]
fn every_sphere_is_found_from_every_cell_its_widened_disc_reaches() {
    let balls: Vec<SphereGpu> = (0..200)
        .map(|i| ball((i * 37 % 900) as f32 - 450.0, (i * 91 % 1600) as f32 - 800.0, 6.0 + (i % 5) as f32))
        .collect();
    let g = SphereGrid::build(&balls);
    let (lo, cell, margin) = ([g.grid[0], g.grid[1]], g.grid[2], g.size[2]);
    assert_eq!(margin, SPHERE_MARGIN);
    let (nx, ny) = (g.size[0] as usize, g.size[1] as usize);
    assert_eq!(g.cells.len(), nx * ny);
    for (i, b) in balls.iter().enumerate() {
        // Points round the ball, out to its radius plus the margin, each in a cell listing it.
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let r = b.a[2] * (1.0 + margin) * 0.999;
            let (x, y) = (b.a[0] + r * a.cos(), b.a[1] + r * a.sin());
            let cx = (((x - lo[0]) / cell).floor() as usize).min(nx - 1);
            let cy = (((y - lo[1]) / cell).floor() as usize).min(ny - 1);
            let [first, n] = g.cells[cy * nx + cx];
            let list = &g.items[first as usize..(first + n) as usize];
            assert!(list.contains(&(i as u32)), "ball {i} missing from cell ({cx}, {cy})");
        }
    }
    assert!(nx <= 256 && ny <= 256);
}
