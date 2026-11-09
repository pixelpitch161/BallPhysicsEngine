use super::*;

#[test]
fn tint_bits_pack_into_the_high_bits() {
    assert_eq!(LayerGpu::tint_bits(1.0, 0.5), (64 << 16) | (32 << 24));
    assert_eq!(LayerGpu::tint_bits(-1.0, 9.0), 255 << 24);
    assert_eq!(LayerGpu::tint_bits(f32::NAN, 0.0), 0);
    for (t, d) in [(1.0, 0.0), (0.3, 0.7), (3.0, 3.0)] {
        assert_eq!(LayerGpu::tint_bits(t, d) & !LayerGpu::TINT_BITS, 0, "the flags' bits stay free");
    }
}

fn record(blocks: f32, opacity: f32) -> LayerGpu {
    let mut r = LayerGpu::ZERO;
    r.light[1] = blocks;
    r.amount[0] = opacity;
    r
}

#[test]
fn only_see_through_blockers_ask_for_the_tint_picture() {
    let mut frame = Frame::default();
    assert!(!frame.see_through_blockers());
    frame.look_layers.push(record(1.0, 1.0));
    frame.look_layers.push(record(0.0, 0.3));
    assert!(!frame.see_through_blockers(), "solid, or not blocking at all");
    frame.look_layers.push(record(0.5, 1.0));
    assert!(frame.see_through_blockers());
    frame.look_layers.clear();
    frame.look_layers.push(record(1.0, 0.5));
    assert!(frame.see_through_blockers(), "a solid record at half opacity");
    let mut trails = Frame::default();
    trails.trails.push(TrailVertex { light: [1.0, 0.0], ..Default::default() });
    assert!(!trails.see_through_blockers(), "a trail that blocks no light");
    trails.trails.push(TrailVertex { light: [1.0, 1.0], ..Default::default() });
    assert!(trails.see_through_blockers());
    let mut layered = Frame::default();
    let mut inner = Frame::default();
    inner.particles.push(Particle { light: [1.0, 1.0, 0.0, 0.0], ..Default::default() });
    layered.layers.push(BlendLayer {
        blend: bpe_core::Blend::Add,
        amount: 1.0,
        each: false,
        z: 0,
        place: Place::Front,
        frame: inner,
    });
    assert!(layered.see_through_blockers(), "a blend layer's particle");
}

#[test]
fn the_tint_pass_needs_glass_colour_a_light_and_something_see_through() {
    let mut frame = Frame::default();
    frame.look_layers.push(LayerGpu { light: [1.0, 0.5, 0.0, 0.0], ..LayerGpu::ZERO });
    let mut scene = LightScene::default();
    scene.settings.more[3] = 1.0;
    assert!(!scene.needs_tint(&frame), "no light");
    scene.lights.push(LightGpu::default());
    assert!(scene.needs_tint(&frame));
    scene.settings.more[3] = 0.0;
    assert!(!scene.needs_tint(&frame), "Coloured light through glass off");
    scene.settings.more[3] = 1.0;
    assert!(!scene.needs_tint(&Frame::default()), "nothing see-through");
}
