//! One lit frame: [`Lighting::run`]. It works out which passes run ([`RunPlan`]), makes the
//! pictures they need, uploads the frame's uniforms and buffers ([`Lighting::upload`]), then
//! records the passes in order: free glass, the blockers' field ([`Lighting::field_passes`]), the
//! light from Light things with caustics and bent light ([`Lighting::direct_passes`]), the light
//! field, the bloom, the moving haze and the memory ([`Lighting::glow_passes`]), the bake's
//! bounce light ([`Lighting::bounce_passes`]), then the mix and what reads it: mirror walls, a
//! glossy floor and depth of field, with the light beams' bands ([`Lighting::mix_passes`]).

use super::passes::Binder;
use super::*;

/// What one run draws: its on/off decisions and the numbers they come from.
pub(super) struct RunPlan {
    /// A baked sample.
    pub(super) baking: bool,
    /// The bake's bounce light is traced (Stage).
    pub(super) traced: bool,
    /// The glass balls caustics trace light through (none: no caustics).
    pub(super) glass: Vec<CausticGlass>,
    pub(super) caustics_on: bool,
    /// Glass and light part 3: lamp light bent by glass shapes, liquid and bubbles.
    pub(super) bent: bent_light::BentPlan,
    pub(super) bent_on: bool,
    /// The background's own light was drawn into the direct light picture.
    pub(super) background_glows: bool,
    /// The tint picture was drawn (light pass 8).
    pub(super) tinted: bool,
    /// The background-light picture carries the background's own light, the caustics, the bent
    /// light and the traced bounce light to the pixels that light themselves.
    pub(super) into_bg: bool,
    /// A glossy floor mirrors the lit picture.
    pub(super) reflects: bool,
    /// Depth of field blurs the finished light.
    pub(super) blurs: bool,
    /// Mirror walls show the lit picture again.
    pub(super) mirror_walls: bool,
    /// Steps a shadow ray takes (the Quality's).
    pub(super) steps: u32,
    /// A picture pixel in world units.
    pub(super) world_pixel: f32,
    /// Part 4: the moving haze, stirred by the balls, the memory, the UV and tags pictures.
    pub(super) haze_moves: bool,
    pub(super) stirring: bool,
    pub(super) remembers: bool,
    pub(super) extras_on: bool,
    pub(super) uv_on: bool,
    pub(super) tags_on: bool,
    /// Part 1b: the light field's bounce light (0 when the bake traces it), its reach, the blooms.
    pub(super) bounce: f32,
    pub(super) reach: f32,
    pub(super) blooms: f32,
    pub(super) field_on: bool,
    /// How many times the light field is worked out (each bounces the one before).
    pub(super) runs: usize,
    pub(super) blooms_on: bool,
    /// The light in the air is worked out (haze, or smoke that catches beams).
    pub(super) haze_on: bool,
}

impl RunPlan {
    fn of(frame: &Frame, scene: &LightScene, output_width: u32, background_glows: bool, tinted: bool) -> RunPlan {
        let s = &scene.settings;
        // Part 3: a baked sample, its bounce light traced in Stage, and caustics.
        let baking = frame.bake.is_some();
        let bounce_setting = s.glow[0];
        // Bounced light is as live unless the bake traces it (Bounced light: Traced).
        let more_bounces = baking && s.bake_more[3] > 0.5;
        let traced = more_bounces && s.stage[0] > 0.5 && bounce_setting != 0.0 && s.bake[1] >= 1.0;
        let glass = if !scene.lights.is_empty() { caustic_glass(scene) } else { Vec::new() };
        let caustics_on = !glass.is_empty();
        // Glass and light part 3: lamp light bent by glass shapes, liquid and bubbles (Glass
        // caustics on, shadows not Off): photons for each (light, bender) pair.
        let bent = bent_light::BentPlan::of(frame, scene);
        let bent_on = bent.on();
        // The background-light picture carries the background's own light, the caustics, the
        // bent light and the traced bounce light to the pixels that light themselves (Stage,
        // shiny surfaces).
        let into_bg = background_glows || caustics_on || traced || bent_on;
        // A glossy floor mirrors the lit picture: the mix draws into a picture of its own.
        let reflects = s.stage[0] > 0.5 && s.floor[0] > 0.5 && s.floor[2] > 0.5;
        // Depth of field blurs the finished light: one more picture in the chain.
        let blurs = s.stage[0] > 0.5 && s.dof[0] > 0.5;
        // Mirror walls show the lit picture again, smoothly sampled: a pass of their own.
        let mirror_walls = !scene.mirrors.is_empty();
        let (_, steps) = quality_tier(s.more[2]);
        let camera = frame.camera;
        let pixel = frame.width as f32 / output_width.max(1) as f32;
        let world_pixel = if camera.zoom == 1.0 { pixel } else { pixel / camera.zoom };

        // Part 4: the moving haze (stirred by the balls), the memory, the UV and tags pictures.
        let haze_moves = s.haze_moves();
        let stirring = haze_moves && s.haze_more[2] != 0.0 && !scene.stirs.is_empty();
        let remembers = s.remembers();

        // What part 1b draws: the light field (glowing things, bounce light), the bloom, the haze.
        let [bounce, reach, blooms, _] = s.glow;
        // A Stage bake traces its bounce light: the field then carries only glowing things'. A
        // bake with no Light bounces has none.
        let bounce = if traced || (more_bounces && s.bake[1] < 1.0) { 0.0 } else { bounce };
        let field_on = scene.glows || bounce != 0.0;
        // One bounce, as live; a bake with Bounced light Traced, Light bounces (each run bounces the one
        // before), at most 16.
        let bounces = if more_bounces { (s.bake[1].max(0.0) as usize).min(16) } else { 1 };
        let runs = match () {
            _ if !field_on => 0,
            _ if bounce == 0.0 => 1,
            _ if scene.glows => 1 + bounces,
            _ => bounces.max(1),
        };
        RunPlan {
            baking,
            traced,
            glass,
            caustics_on,
            bent,
            bent_on,
            background_glows,
            tinted,
            into_bg,
            reflects,
            blurs,
            mirror_walls,
            steps,
            world_pixel,
            haze_moves,
            stirring,
            remembers,
            extras_on: haze_moves || remembers,
            uv_on: scene.needs_uv(),
            tags_on: scene.needs_tags(),
            bounce,
            reach,
            blooms,
            field_on,
            runs,
            blooms_on: blooms != 0.0,
            // The light in the air is worked out for haze and for smoke that catches beams.
            haze_on: s.air[..3].iter().any(|c| *c != 0.0) || s.air[3] != 0.0 || scene.smoke,
        }
    }
}

/// What [`Lighting::upload`] made for the passes.
pub(super) struct Uploaded {
    /// The jump flood's passes (each with its own uniform after the base one).
    pub(super) jumps: usize,
    /// How many cascades the light field uses.
    pub(super) count: u32,
    /// The 1b passes' base uniform, the cascades' (run by run), the runs' and the bloom levels'.
    pub(super) base_glow: wgpu::Buffer,
    pub(super) cascade_uniforms: Vec<wgpu::Buffer>,
    pub(super) run_uniforms: Vec<wgpu::Buffer>,
    pub(super) level_uniforms: Vec<wgpu::Buffer>,
    /// The behind picture is bound (glass shows what is behind it).
    pub(super) has_behind: bool,
}

impl Lighting {
    /// Lights `frame` (whose things are in `picture`, a copy of the HDR picture) into `out`.
    /// `background_glows`: the background's own light was drawn into the direct light picture;
    /// `tinted`: the tint picture was drawn (light pass 8).
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        scene: &LightScene,
        output_width: u32,
        picture: &wgpu::TextureView,
        out: &wgpu::TextureView,
        background_glows: bool,
        tinted: bool,
    ) {
        if self.targets.is_none() {
            return;
        }
        let plan = RunPlan::of(frame, scene, output_width, background_glows, tinted);
        self.bent_count = plan.bent.per * plan.bent.run as u32;
        self.make_run_targets(device, &plan);
        let up = self.upload(device, queue, frame, scene, &plan);

        let t = self.targets.as_ref().expect("checked");
        let b = Binder { lighting: self, device, t, picture };
        // The memory: last frame's in one extras picture, this frame's drawn into the other.
        let memory_to = 1 - self.memory.current;
        let none = &t.dummy_colour.1;
        let all = GlowInputs {
            emission: &t.emission.1,
            direct: &t.direct.1,
            air: &t.air.1,
            gi_scene: &t.gi_scene.1,
            gi_dist: &t.gi_dist.1,
            above: none,
            gi: [&t.gi[0].1, &t.gi[1].1, &t.gi[2].1],
            glow: &t.glow.2,
            tags: t.tags.as_ref().filter(|_| plan.tags_on).map_or(none, |p| &p.1),
            uv: t.uv.as_ref().filter(|_| plan.uv_on).map_or(none, |p| &p.1),
            extras: t.extras.as_ref().filter(|_| plan.extras_on).map_or(none, |e| &e[memory_to].1),
            stir: t.stir.as_ref().filter(|_| plan.stirring).map_or(none, |e| &e.1),
        };
        self.binds.sweep();
        let stage_group = self.stage_group(&b, &up);
        let (main, seeds) = self.field_passes(&b, encoder, &plan, &up);
        self.direct_passes(&b, encoder, scene, &plan, &up, &all, (&main, &stage_group));
        self.glow_passes(&b, encoder, scene, &plan, &up, &all, (&main, seeds));
        self.bounce_passes(&b, encoder, scene, &plan, &up, &all, (&main, seeds, &stage_group));
        self.mix_passes(&b, encoder, scene, out, &plan, &up, &all, (seeds, &stage_group));
        let bound = [main, stage_group];

        self.bound = Some(bound);
        if plan.extras_on {
            self.memory.current = memory_to;
        }
    }

    /// Makes the pictures this run needs that aren't made yet: the chain after the mix, depth of
    /// field's small copy, the background light, the extras, the haze stir, the traced bounce
    /// light and the light field's cascades.
    fn make_run_targets(&mut self, device: &wgpu::Device, plan: &RunPlan) {
        let Some(t) = self.targets.as_mut() else { return };
        let chained = usize::from(plan.reflects) + usize::from(plan.blurs) + usize::from(plan.mirror_walls);
        if chained > 0 && t.stage.is_none() {
            t.stage = Some(texture(device, "lit stage", (t.width, t.height), HDR_FORMAT, false));
        }
        if chained > 1 && t.stage_more.is_none() {
            t.stage_more = Some(texture(device, "lit stage", (t.width, t.height), HDR_FORMAT, false));
        }
        if plan.blurs && t.dof_small.is_none() {
            let small = (t.width.div_ceil(DOF_SMALL), t.height.div_ceil(DOF_SMALL));
            t.dof_small = Some(texture(device, "depth of field copy", small, HDR_FORMAT, false));
        }
        if plan.into_bg && t.bg_glow.is_none() {
            t.bg_glow = Some(texture(device, "background light", (t.width, t.height), HDR_FORMAT, true));
        }
        let small = t.field_size();
        if plan.extras_on && t.extras.is_none() {
            t.extras = Some([
                texture(device, "light extras", small, GI_FORMAT, false),
                texture(device, "light extras", small, GI_FORMAT, false),
            ]);
        }
        if plan.stirring && t.stir.is_none() {
            t.stir = Some(texture(device, "haze stir", small, GI_FORMAT, false));
        }
        if plan.traced && t.traced.is_none() {
            let full = (t.width, t.height);
            t.traced = Some([
                texture(device, "bounce light", full, HDR_FORMAT, false),
                texture(device, "bounce light", full, HDR_FORMAT, false),
            ]);
        }
        if plan.field_on && t.cascades.is_none() {
            let size = (2 * t.padded.0, 2 * t.padded.1);
            t.cascades = Some([
                texture(device, "light cascade", size, GI_FORMAT, false),
                texture(device, "light cascade", size, GI_FORMAT, false),
            ]);
        }
    }

    /// Writes the run's uniforms and uploads its buffers: the base uniform and the jump flood's,
    /// the lights, segments, links and light tiles, the memory's step, the balls stirring the
    /// haze, the 1b uniforms, the surfaces, spheres and mirrors with the stage's uniform, the
    /// caustics' glass, bent light's pairs and legs, and the light beams' bands.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        scene: &LightScene,
        plan: &RunPlan,
    ) -> Uploaded {
        let s = &scene.settings;
        let t = self.targets.as_ref().expect("checked");
        let (fw, fh) = t.field_size();
        let divisor = t.divisor;
        let (pw, ph) = t.padded;
        let camera = frame.camera;
        let world_pixel = plan.world_pixel;
        let (dichroic, dichroic_count) = dichroic_list(&scene.surfaces);
        // Review S19: which lights each tile of the picture may take.
        let links_len = scene.links.len().max(4);
        let light_tiles = if !self.tiles_on {
            None
        } else {
            LightTiles::build(
                &scene.lights,
                &scene.segments,
                [camera.center.x, camera.center.y, camera.zoom, camera.turn],
                (t.width, t.height),
                (frame.width as f32, frame.height as f32),
                links_len,
            )
        };
        let (tiles, tile_list) = LightTiles::uniform(light_tiles.as_ref(), links_len);
        let bending = plan.bent.bending();
        let base = Uniform {
            view: [2.0 / frame.width.max(1) as f32, 2.0 / frame.height.max(1) as f32, world_pixel, frame.time],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            ambient: [s.ambient[0], s.ambient[1], s.ambient[2], scene.show.code()],
            shadows: s.shadows,
            more: [s.more[0], s.more[1], plan.steps as f32, scene.lights.len() as f32],
            field: [fw as f32, fh as f32, world_pixel * divisor as f32, 0.0],
            sample: frame
                .bake
                .map_or([0.0, 1.0, 0.0, 0.0], |b| [b.index as f32, b.count.max(1) as f32, 1.0, s.bake[1].max(0.0)]),
            trace: [
                s.glow[0],
                if s.stage[0] > 0.5 { 1.0 } else { 0.0 },
                if s.glass_colour() { 1.0 } else { 0.0 },
                if plan.tinted { 1.0 } else { 0.0 },
            ],
            // Only glass that bends changes the direct light (mirrors add light, part 4a).
            bent: [if bending { 1.0 } else { 0.0 }, dichroic_count as f32, 0.0, 0.0],
            tiles,
            tile_list,
            bends: if bending { bent_light::bend_bits(&plan.bent.benders) } else { [[0; 4]; 16] },
            dichroic,
        };
        // The jump flood's steps: from half the field's size down to one, then one again.
        let mut jumps = Vec::new();
        let mut step = (fw.max(fh) / 2).max(1).next_power_of_two();
        while step >= 1 && jumps.len() < MOST_JUMPS - 1 {
            jumps.push(step);
            step /= 2;
        }
        jumps.push(1);
        queue.write_buffer(&self.uniforms[0], 0, bytemuck::bytes_of(&base));
        if plan.traced {
            for (b, step) in self.trace_uniforms.iter().zip([1.0, 2.0, 4.0, 0.0]) {
                let mut u = base;
                u.field[3] = step;
                queue.write_buffer(b, 0, bytemuck::bytes_of(&u));
            }
        }
        for (i, step) in jumps.iter().enumerate() {
            let mut u = base;
            u.field[3] = *step as f32;
            queue.write_buffer(&self.uniforms[i + 1], 0, bytemuck::bytes_of(&u));
        }
        // A lit scene with no lights still binds something.
        let lights: &[LightGpu] = if scene.lights.is_empty() { &[LightGpu::ZERO] } else { &scene.lights };
        self.lights.upload(device, queue, bytemuck::cast_slice(lights));
        let segments: &[[f32; 4]] = if scene.segments.is_empty() { &[[0.0; 4]] } else { &scene.segments };
        self.segments.upload(device, queue, bytemuck::cast_slice(segments));
        // The Lights up links (at least 4 entries), then the light tiles.
        let mut links = scene.links.clone();
        links.resize(links_len, 0.0);
        if let Some(t) = &light_tiles {
            links.extend_from_slice(&t.entries);
        }
        self.links.upload(device, queue, bytemuck::cast_slice(&links));

        let camera_now = [camera.center.x, camera.center.y, camera.zoom, camera.turn];
        let (memory_step, last_camera) = self.remember(frame, s.memory, plan, camera_now);
        if plan.stirring {
            let stirs: Vec<[f32; 4]> = scene.stirs.iter().flat_map(|s| *s).collect();
            self.stirs.upload(device, queue, bytemuck::cast_slice(&stirs));
        }

        let texel_world = world_pixel * divisor as f32;
        let diagonal = ((fw * fw + fh * fh) as f32).sqrt();
        let reach = plan.reach;
        let count = cascade_count(if reach > 0.0 { diagonal.min(reach / texel_world.max(1e-6)) } else { 1.0 });
        let glow_base = GlowUniform {
            size: [fw as f32, fh as f32, pw as f32, ph as f32],
            cascade: [0.0, count as f32, gi_steps(s.more[2]) as f32, 0.0],
            glow: [plan.bounce, reach, plan.blooms, 0.0],
            air: s.air,
            on: [
                f32::from(u8::from(plan.field_on)),
                f32::from(u8::from(plan.blooms_on)),
                f32::from(u8::from(plan.haze_on)),
                f32::from(u8::from(s.stage[0] > 0.5)),
            ],
            haze: s.haze,
            haze_more: [s.haze_more[0], s.haze_more[1], if plan.stirring { s.haze_more[2] } else { 0.0 }, 0.0],
            water: s.water,
            water_colour: s.water_colour,
            water_depth: s.water_depth,
            memory: s.memory,
            memory_colour: s.memory_colour,
            memory_step,
            last_camera,
            more_on: [
                f32::from(u8::from(plan.uv_on)),
                f32::from(u8::from(plan.tags_on)),
                f32::from(u8::from(plan.extras_on)),
                f32::from(u8::from(plan.haze_moves)),
            ],
            rainbow: rainbow_of(&s.rainbow),
            rainbow_at: {
                let [x, y] = rainbow_centre(scene, [frame.camera.center.x, frame.camera.center.y]);
                [x, y, f32::from(u8::from(scene.rainbows_in_rain() && plan.tags_on)), 0.0]
            },
        };
        let runs = plan.runs;
        let base_glow = self.glow_uniform(device, 0);
        queue.write_buffer(&base_glow, 0, bytemuck::bytes_of(&glow_base));
        let mut cascade_uniforms = Vec::new();
        for run in 0..runs {
            for i in 0..count {
                let b = self.glow_uniform(device, 1 + run * MOST_CASCADES as usize + i as usize);
                let mut u = glow_base;
                u.cascade[0] = i as f32;
                u.cascade[3] = run as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                cascade_uniforms.push(b);
            }
        }
        let runs_base = 1 + runs.max(2) * MOST_CASCADES as usize;
        let run_uniforms: Vec<wgpu::Buffer> = (0..runs)
            .map(|run| {
                let b = self.glow_uniform(device, runs_base + run);
                let mut u = glow_base;
                u.cascade[3] = run as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                b
            })
            .collect();
        let level_uniforms: Vec<wgpu::Buffer> = (1..GLOW_LEVELS as usize)
            .map(|level| {
                let b = self.glow_uniform(device, runs_base + runs.max(2) + level);
                let mut u = glow_base;
                u.glow[3] = level as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                b
            })
            .collect();

        let has_behind = self.upload_stage(device, queue, scene, plan);
        if plan.caustics_on {
            self.upload_caustics(device, queue, frame, scene, plan);
        }
        if plan.bent_on {
            self.upload_bent(device, queue, frame, scene, plan);
        }
        if !scene.ribbons.is_empty() {
            self.ribbons.upload(device, queue, bytemuck::cast_slice(&scene.ribbons));
        }
        Uploaded { jumps: jumps.len(), count, base_glow, cascade_uniforms, run_uniforms, level_uniforms, has_behind }
    }

    /// Glow in the dark's memory: how much last frame's fades and this frame charges, and the
    /// camera it was seen through (a seek forgets it).
    fn remember(
        &mut self,
        frame: &Frame,
        memory: [f32; 4],
        plan: &RunPlan,
        camera_now: [f32; 4],
    ) -> ([f32; 4], [f32; 4]) {
        let mut memory_step = [0.0f32; 4];
        let mut last_camera = camera_now;
        if plan.remembers {
            let (fade, charge) = (memory[1].max(1e-3), memory[2]);
            match self.memory.last {
                // Carried on from the last frame (the same frame again changes nothing).
                Some((last, cam)) if frame.time >= last && frame.time - last <= MOST_MEMORY_STEP => {
                    let dt = frame.time - last;
                    memory_step = [(-dt / fade).exp(), (1.0 - (-charge.max(0.0) * dt).exp()).clamp(0.0, 1.0), 1.0, 0.0];
                    last_camera = cam;
                    self.memory.cold = false;
                }
                // A seek: nothing remembered, cold unless this is the very start.
                _ => self.memory.cold = frame.time > 1e-4,
            }
            if plan.stirring || plan.remembers {
                self.memory.last = Some((frame.time, camera_now));
            }
        } else {
            self.memory.last = None;
            self.memory.cold = false;
        }
        (memory_step, last_camera)
    }
    /// Surfaces (0 always the plain sheet) and, in Stage, the spheres and their grid, the mirrors
    /// and the stage's uniform. Gives whether the picture behind glass is bound.
    fn upload_stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &LightScene, plan: &RunPlan) -> bool {
        let s = &scene.settings;
        let mut surfaces = scene.surfaces.clone();
        if surfaces.is_empty() {
            surfaces.push(SurfaceGpu::MATTE);
        }
        surfaces[0] = SurfaceGpu::MATTE;
        self.surfaces.upload(device, queue, bytemuck::cast_slice(&surfaces));
        let stage = s.stage[0] > 0.5;
        // Flat reads the spheres too (part 2b: reflections and glass), but never for shadows.
        let grid = SphereGrid::build(&scene.spheres);
        let spheres: &[SphereGpu] = if !scene.spheres.is_empty() { &scene.spheres } else { &[SphereGpu::default()] };
        self.spheres.upload(device, queue, bytemuck::cast_slice(spheres));
        self.cells.upload(device, queue, bytemuck::cast_slice(&grid.cells));
        let items: &[u32] = if grid.items.is_empty() { &[0] } else { &grid.items };
        self.items.upload(device, queue, bytemuck::cast_slice(items));
        let sphere_count = if !grid.items.is_empty() { scene.spheres.len() as f32 } else { 0.0 };
        let mirrors: &[MirrorGpu] = if scene.mirrors.is_empty() { &[MirrorGpu::default()] } else { &scene.mirrors };
        self.mirrors.upload(device, queue, bytemuck::cast_slice(mirrors));
        let has_behind = self.targets.as_ref().is_some_and(|t| t.behind.is_some()) && scene.refracts;
        let uniform = StageUniform {
            room: s.stage,
            floor: s.floor,
            more: [s.floor_more[0], s.floor_more[1], surfaces.len() as f32, sphere_count],
            sky_top: s.sky_top,
            sky_bottom: s.sky_bottom,
            grid: grid.grid,
            grid_size: grid.size,
            dof: s.dof,
            counts: [
                scene.mirrors.len().min(MOST_MIRRORS) as f32,
                f32::from(u8::from(has_behind)),
                f32::from(u8::from(plan.into_bg)),
                f32::from(u8::from(stage && plan.field_on)),
            ],
        };
        queue.write_buffer(&self.stage_uniform, 0, bytemuck::bytes_of(&uniform));
        has_behind
    }

    /// Caustics (part 3): their uniform and the glass balls they trace light through.
    fn upload_caustics(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        scene: &LightScene,
        plan: &RunPlan,
    ) {
        let s = &scene.settings;
        let glass = &plan.glass;
        let lights_n = scene.lights.len();
        let per = photons_per_pair(s.more[2], lights_n * glass.len());
        let turn = frame.bake.map_or(0.0, |b| b.index as f32 * 2.399_963);
        let cu = CausticUniform {
            counts: [per as f32, glass.len() as f32, lights_n as f32, s.bent[2]],
            more: [plan.world_pixel, turn, f32::from(u8::from(plan.baking)), s.bake_more[2]],
        };
        queue.write_buffer(&self.caustic_uniform, 0, bytemuck::bytes_of(&cu));
        self.glass.upload(device, queue, bytemuck::cast_slice(glass));
    }

    /// Light bent by glass (part 3): its uniform, the pairs that send photons and room for their
    /// legs.
    fn upload_bent(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        scene: &LightScene,
        plan: &RunPlan,
    ) {
        let s = &scene.settings;
        let (per_pair, bent_run) = (plan.bent.per, plan.bent.run);
        let photons = per_pair * bent_run as u32;
        let turn = frame.bake.map_or(0.0, |b| (b.index as f32 * 0.618_034).fract());
        let half_width = (3.0 * s.bent[1].max(0.0)).max(2.0) * plan.world_pixel;
        let (table, throwers) = bent_light::throw_table(&plan.bent.throwers);
        let bu = BentUniform {
            counts: [per_pair as f32, bent_run as f32, (16 * plan.steps).max(1024) as f32, 0.0],
            more: [plan.world_pixel, turn, half_width, s.bake_more[2]],
            extra: [s.bent[2], throwers as f32, 0.0, 0.0],
            throws: bent_light::throw_bits(&plan.bent.throwers),
            table,
        };
        queue.write_buffer(&self.bent_uniform, 0, bytemuck::bytes_of(&bu));
        self.bent_pairs.upload(device, queue, bytemuck::cast_slice(&plan.bent.pairs[..bent_run]));
        let leg_count = photons as u64 * BENT_LEGS as u64;
        self.legs.reserve(device, leg_count * std::mem::size_of::<Leg>() as u64);
    }
}
