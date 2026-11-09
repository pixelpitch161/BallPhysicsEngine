//! The view and culling: placing the frame's camera, fitting its lists to the GPU's limits and
//! dropping what the camera can't see.

use super::*;

/// Keeps the sprites of `list` that `keep` says and the splats of `fields` that `keep_splat`
/// says, each field (and each swarm run of `swarms`) still sitting between the same kept sprites.
pub(super) fn cull_sprites(
    list: &mut Vec<Particle>,
    fields: &mut [Field],
    swarms: &mut [crate::SwarmRun],
    keep: impl Fn(&Particle) -> bool,
    keep_splat: impl Fn(&Splat) -> bool,
) {
    if fields.is_empty() && swarms.is_empty() {
        list.retain(keep);
        return;
    }
    let seen: Vec<bool> = list.iter().map(&keep).collect();
    let mut kept_before = vec![0u32; seen.len() + 1];
    for (i, &s) in seen.iter().enumerate() {
        kept_before[i + 1] = kept_before[i] + u32::from(s);
    }
    for f in fields.iter_mut() {
        f.at = kept_before[(f.at as usize).min(seen.len())];
        f.splats.retain(&keep_splat);
    }
    for r in swarms.iter_mut() {
        r.at = kept_before[(r.at as usize).min(seen.len())];
    }
    let mut i = 0;
    list.retain(|_| {
        i += 1;
        seen[i - 1]
    });
}

/// World units past the edge of the shot that are still drawn, for glows and halos.
pub(super) const CULL_MARGIN: f32 = 64.0;

impl Frame {
    /// Looks at the frame through `camera` on a picture `size` world pixels across, its blend
    /// layers with it. Things stuck to the screen take the new size and keep no camera.
    pub fn set_view(&mut self, camera: CameraView, size: (u32, u32)) {
        self.camera = camera;
        (self.width, self.height) = size;
        for l in &mut self.layers {
            l.frame.set_view(camera, size);
        }
        if let Some(o) = &mut self.overlay {
            let camera = o.camera;
            o.set_view(camera, size);
        }
    }

    /// Cuts its trail and particle lists (its overlay's and blend layers' too) to what the
    /// graphics card can hold in one buffer, keeping the first: a last guard, far above what any
    /// layer draws on its own.
    pub fn fit_lists(&mut self) {
        self.fit_lists_to(MOST_TRAIL_VERTICES, MOST_SPRITES);
    }

    pub(super) fn fit_lists_to(&mut self, vertices: usize, sprites: usize) {
        // Whole triangles.
        let vertices = vertices - vertices % 6;
        self.trails.truncate(vertices);
        self.trails_front.truncate(vertices);
        self.particles.truncate(sprites);
        self.particles_front.truncate(sprites);
        for (fields, len) in
            [(&mut self.fields, self.particles.len()), (&mut self.fields_front, self.particles_front.len())]
        {
            for f in fields.iter_mut() {
                f.at = f.at.min(len as u32);
                f.splats.truncate(sprites);
            }
        }
        for (swarms, len) in
            [(&mut self.swarms, self.particles.len()), (&mut self.swarms_front, self.particles_front.len())]
        {
            for r in swarms.iter_mut() {
                r.at = r.at.min(len as u32);
            }
        }
        if let Some(o) = &mut self.overlay {
            o.fit_lists_to(vertices, sprites);
        }
        for l in &mut self.layers {
            l.frame.fit_lists_to(vertices, sprites);
        }
    }

    /// Drops balls, looks and flashes the camera can't see, so a big world draws only what is
    /// near the shot. Blend layers are culled through this frame's camera and size, whatever
    /// their own say.
    pub fn cull(&mut self) {
        let (camera, size) = (self.camera, (self.width, self.height));
        self.cull_through(camera, size);
    }

    pub(super) fn cull_through(&mut self, camera: CameraView, (width, height): (u32, u32)) {
        if camera.is_identity() {
            return;
        }
        let video = Vec2::new(width as f32, height as f32);
        let sees = |c: [f32; 2], r: f32| camera.sees(video, Vec2::from(c), r + CULL_MARGIN);
        self.paths.retain(|p| sees([p.a[0], p.a[1]], p.reach()));
        let seen: Vec<bool> = self
            .layered
            .iter()
            .map(|l| {
                let along = l.stretch[2].max(l.stretch[3]).max(1.0);
                sees(l.center, l.radius * l.form[3].max(1.0) * along)
            })
            .collect();
        // Each run names its instances by place, so the runs move with the instances kept.
        let mut kept_before = vec![0u32; seen.len() + 1];
        for (i, &s) in seen.iter().enumerate() {
            kept_before[i + 1] = kept_before[i] + u32::from(s);
        }
        let place = |i: u32| kept_before[(i as usize).min(seen.len())];
        self.layered_runs = self
            .layered_runs
            .iter()
            .map(|r| crate::LayeredRun { start: place(r.start), end: place(r.end), ..*r })
            .filter(|r| r.end > r.start)
            .collect();
        let mut i = 0;
        self.layered.retain(|_| {
            i += 1;
            seen[i - 1]
        });
        self.flashes.retain(|f| sees(f.center, f.size));
        let reach = |p: &Particle| p.place[3].max(p.style[0]) * (1.0 + p.style[2]) + 4.0;
        let splat = |s: &Splat| sees([s.place[0], s.place[1]], s.place[2] * 1.6 + 4.0);
        cull_sprites(
            &mut self.particles,
            &mut self.fields,
            &mut self.swarms,
            |p| sees([p.place[0], p.place[1]], reach(p)),
            splat,
        );
        cull_sprites(
            &mut self.particles_front,
            &mut self.fields_front,
            &mut self.swarms_front,
            |p| sees([p.place[0], p.place[1]], reach(p)),
            splat,
        );
        for l in &mut self.layers {
            l.frame.cull_through(camera, (width, height));
        }
    }
}
