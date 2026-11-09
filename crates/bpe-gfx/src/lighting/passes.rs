//! The passes of one lit frame, in the order [`Lighting::run`] records them.

use super::run::{RunPlan, Uploaded};
use super::*;

/// One full-screen pass drawing into `views` with `pipeline` and its bind groups in order.
pub(super) fn pass(
    encoder: &mut wgpu::CommandEncoder,
    views: &[(&wgpu::TextureView, wgpu::LoadOp<wgpu::Color>)],
    pipeline: &wgpu::RenderPipeline,
    binds: &[&wgpu::BindGroup],
    label: &str,
) {
    let attachments: Vec<_> = views
        .iter()
        .map(|(view, load)| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: *load, store: wgpu::StoreOp::Store },
            })
        })
        .collect();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    for (k, b) in binds.iter().enumerate() {
        pass.set_bind_group(k as u32, *b, &[]);
    }
    pass.draw(0..3, 0..1);
}

pub(super) const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

/// The whole of a buffer just uploaded, to bind.
pub(super) fn buffer(b: &GrowableBuffer) -> wgpu::BindingResource<'_> {
    b.buffer.as_ref().expect("just uploaded").as_entire_binding()
}

/// Makes (or finds again, [`BindCache`]) the bind groups the passes of one run use.
pub(super) struct Binder<'a> {
    pub(super) lighting: &'a Lighting,
    pub(super) device: &'a wgpu::Device,
    pub(super) t: &'a Targets,
    /// A copy of the HDR picture with the things in it.
    pub(super) picture: &'a wgpu::TextureView,
}

impl<'a> Binder<'a> {
    /// Any group, through the cache.
    fn get(&self, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup {
        self.lighting.binds.get(self.device, desc)
    }

    /// Group 0 with `uniform`, `picture`, the blockers' `field` and jump flood `seeds`. `lit` is
    /// the lit share of the picture; depth of field, which never reads it, has its small copy
    /// there instead (a new binding would pass the 16 textures a pass may read).
    fn with(
        &self,
        uniform: &wgpu::Buffer,
        picture: &wgpu::TextureView,
        field: &wgpu::TextureView,
        seeds: &wgpu::TextureView,
        lit: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let l = self.lighting;
        self.get(&wgpu::BindGroupDescriptor {
            label: Some("lighting"),
            layout: &l.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(picture) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(lit) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.t.surface.1) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(field) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&l.sampler) },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: l.lights.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: l.segments.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(seeds) },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: l.links.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                },
            ],
        })
    }

    /// Group 0 reading `picture` in the picture's place, with the lit share.
    fn on(
        &self,
        uniform: &wgpu::Buffer,
        picture: &wgpu::TextureView,
        field: &wgpu::TextureView,
        seeds: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.with(uniform, picture, field, seeds, &self.t.lit.1)
    }

    /// Group 0 reading the picture.
    fn main(&self, uniform: &wgpu::Buffer, field: &wgpu::TextureView, seeds: &wgpu::TextureView) -> wgpu::BindGroup {
        self.on(uniform, self.picture, field, seeds)
    }

    /// Group 1 of `layout`: `uniform` and the pictures at `bindings`.
    fn inputs(
        &self,
        layout: &wgpu::BindGroupLayout,
        bindings: &[u32],
        uniform: &wgpu::Buffer,
        i: &GlowInputs,
    ) -> wgpu::BindGroup {
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }];
        entries.extend(
            bindings
                .iter()
                .map(|b| wgpu::BindGroupEntry { binding: *b, resource: wgpu::BindingResource::TextureView(i.at(*b)) }),
        );
        self.get(&wgpu::BindGroupDescriptor { label: Some("lighting 1b"), layout, entries: &entries })
    }

    /// Group 1 of the 1b passes.
    fn glow(&self, uniform: &wgpu::Buffer, i: &GlowInputs) -> wgpu::BindGroup {
        self.inputs(&self.lighting.glow_layout, GLOW_BINDINGS, uniform, i)
    }
}

impl Lighting {
    /// Group 2 of the passes that read surfaces and the stage: its uniform, the surfaces, the
    /// spheres and their grid, the mirrors and the picture behind glass.
    pub(super) fn stage_group(&self, b: &Binder, up: &Uploaded) -> wgpu::BindGroup {
        let t = b.t;
        let behind_view = t.behind.as_ref().filter(|_| up.has_behind).map_or(&t.dummy_colour.1, |b| &b.1);
        b.get(&wgpu::BindGroupDescriptor {
            label: Some("lighting stage"),
            layout: &self.stage_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.stage_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: buffer(&self.surfaces) },
                wgpu::BindGroupEntry { binding: 2, resource: buffer(&self.spheres) },
                wgpu::BindGroupEntry { binding: 3, resource: buffer(&self.cells) },
                wgpu::BindGroupEntry { binding: 4, resource: buffer(&self.items) },
                wgpu::BindGroupEntry { binding: 5, resource: buffer(&self.mirrors) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(behind_view) },
            ],
        })
    }

    /// Free glass marked in the surface picture, then the blockers' field: seeds, the jump flood
    /// and the signed distance. Gives group 0 as every later pass reads it (the blockers' field
    /// and their jump flood: nearest blocker and nearest open texel) and which seeds picture holds
    /// the flood.
    pub(super) fn field_passes(
        &self,
        b: &Binder,
        encoder: &mut wgpu::CommandEncoder,
        plan: &RunPlan,
        up: &Uploaded,
    ) -> (wgpu::BindGroup, usize) {
        let t = b.t;
        // Glass and light part 3: free glass (drawn by light pass 9) marked in the surface picture
        // before anything reads it.
        if plan.bent.free_glass()
            && let Some([glass, under]) = t.glass.as_ref()
        {
            encoder.copy_texture_to_texture(
                t.surface.0.as_image_copy(),
                under.0.as_image_copy(),
                wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
            );
            let g = b.get(&wgpu::BindGroupDescriptor {
                label: Some("free glass"),
                layout: &self.free_glass_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&glass.1) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&under.1) },
                ],
            });
            pass(encoder, &[(&t.surface.1, CLEAR)], &self.free_glass, &[&g], "free glass");
        }
        // The seed and field passes read the tint picture (glass and light part 1) in the picture's
        // place: clear coloured glass seeds the field too.
        let tint = t.tint.as_ref().filter(|_| plan.tinted).map_or(&t.dummy_colour.1, |p| &p.1);
        // Seeds into the first picture, then back and forth.
        let seed = b.on(&self.uniforms[0], tint, &t.field.1, &t.seeds[1].1);
        pass(encoder, &[(&t.seeds[0].1, CLEAR)], &self.seed, &[&seed], "light seeds");
        let mut from = 0;
        for i in 0..up.jumps {
            let to = 1 - from;
            let g = b.main(&self.uniforms[i + 1], &t.field.1, &t.seeds[from].1);
            pass(encoder, &[(&t.seeds[to].1, CLEAR)], &self.jump, &[&g], "light jump");
            from = to;
        }
        let field = b.on(&self.uniforms[0], tint, &t.dummy.1, &t.seeds[from].1);
        pass(encoder, &[(&t.field.1, CLEAR)], &self.field, &[&field], "light field");
        (b.main(&self.uniforms[0], &t.field.1, &t.seeds[from].1), from)
    }

    /// The light from Light things, on surfaces and in the air, added over the background's own
    /// light when it gives any: first caustics (part 3: light focused through glass balls, added
    /// where it lands) and light bent by glass (part 3: the photons' ways, then their legs
    /// splatted into the direct light and the light in the air), then the lamps.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn direct_passes(
        &self,
        b: &Binder,
        encoder: &mut wgpu::CommandEncoder,
        scene: &LightScene,
        plan: &RunPlan,
        up: &Uploaded,
        all: &GlowInputs,
        (main, stage_group): (&wgpu::BindGroup, &wgpu::BindGroup),
    ) {
        let t = b.t;
        let none = &t.dummy_colour.1;
        if plan.caustics_on {
            let group = b.get(&wgpu::BindGroupDescriptor {
                label: Some("caustics"),
                layout: &self.caustic_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.caustic_uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: buffer(&self.glass) },
                ],
            });
            let (lights_n, glass_n) = (scene.lights.len(), plan.glass.len());
            let per = photons_per_pair(scene.settings.more[2], lights_n * glass_n);
            let total = per as u64 * lights_n as u64 * glass_n as u64;
            let load = if plan.background_glows { wgpu::LoadOp::Load } else { CLEAR };
            let mut p = crate::begin_pass(encoder, &t.direct.1, "caustics", load);
            p.set_pipeline(&self.caustic);
            p.set_bind_group(0, main, &[]);
            p.set_bind_group(1, &self.empty_group, &[]);
            p.set_bind_group(2, stage_group, &[]);
            p.set_bind_group(3, &group, &[]);
            p.draw(0..4, 0..total.min(u32::MAX as u64) as u32);
        }
        if plan.bent_on {
            let photons = plan.bent.per * plan.bent.run as u32;
            let leg_count = photons as u64 * BENT_LEGS as u64;
            let ways = b.get(&wgpu::BindGroupDescriptor {
                label: Some("bent light photons"),
                layout: &self.bent_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 4, resource: self.bent_uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: buffer(&self.bent_pairs) },
                    wgpu::BindGroupEntry { binding: 6, resource: buffer(&self.legs) },
                ],
            });
            {
                let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("bent light photons"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(&self.bent);
                cp.set_bind_group(0, main, &[]);
                cp.set_bind_group(1, &self.empty_group, &[]);
                cp.set_bind_group(2, stage_group, &[]);
                cp.set_bind_group(3, &ways, &[]);
                cp.dispatch_workgroups(photons.div_ceil(64), 1, 1);
            }
            let splat = b.get(&wgpu::BindGroupDescriptor {
                label: Some("bent light splat"),
                layout: &self.bent_draw_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 4, resource: self.bent_uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: buffer(&self.legs) },
                ],
            });
            let load = if plan.background_glows || plan.caustics_on { wgpu::LoadOp::Load } else { CLEAR };
            let attachments = [(&t.direct.1, load), (&t.air.1, CLEAR)].map(|(view, load)| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })
            });
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bent light"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(&self.bent_splat);
            p.set_bind_group(0, main, &[]);
            p.set_bind_group(1, &self.empty_group, &[]);
            p.set_bind_group(2, &self.empty_group, &[]);
            p.set_bind_group(3, &splat, &[]);
            p.draw(0..4, 0..leg_count.min(u32::MAX as u64) as u32);
        }
        let glows_in_direct = plan.background_glows || plan.caustics_on || plan.bent_on;
        let direct_load = if glows_in_direct { wgpu::LoadOp::Load } else { CLEAR };
        let air_load = if plan.bent_on { wgpu::LoadOp::Load } else { CLEAR };
        if let Some((bg, bg_v)) = t.bg_glow.as_ref().filter(|_| plan.into_bg) {
            // Kept apart for the stage and shiny surfaces, which light a pixel themselves.
            if glows_in_direct {
                encoder.copy_texture_to_texture(
                    t.direct.0.as_image_copy(),
                    bg.as_image_copy(),
                    wgpu::Extent3d { width: t.width.max(1), height: t.height.max(1), depth_or_array_layers: 1 },
                );
            } else {
                drop(crate::begin_pass(encoder, bg_v, "no background light", CLEAR));
            }
        }
        let inputs = GlowInputs { direct: none, air: none, ..*all };
        let g = b.inputs(&self.direct_layout, DIRECT_BINDINGS, &up.base_glow, &inputs);
        pass(encoder, &[(&t.direct.1, direct_load), (&t.air.1, air_load)], &self.direct, &[main, &g], "direct light");
    }

    /// Part 1b and 4: the light field, once or more (each time the one before's light bounces
    /// too), the lights' bloom (the lamps and what gives off light, then smaller and smaller), the
    /// balls stirring the haze, then the extras: the haze's density and the memory.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn glow_passes(
        &self,
        b: &Binder,
        encoder: &mut wgpu::CommandEncoder,
        scene: &LightScene,
        plan: &RunPlan,
        up: &Uploaded,
        all: &GlowInputs,
        (main, _): (&wgpu::BindGroup, usize),
    ) {
        let t = b.t;
        let none = &t.dummy_colour.1;
        let all = *all;
        let count = up.count;
        if let Some(cascades) = t.cascades.as_ref().filter(|_| plan.field_on) {
            for run in 0..plan.runs {
                let g = b.glow(&up.run_uniforms[run], &GlowInputs { gi_scene: none, ..all });
                pass(encoder, &[(&t.gi_scene.1, CLEAR)], &self.gi_scene, &[main, &g], "light field scene");
                if run == 0 {
                    // How far each texel is from something opaque: a jump flood of its own.
                    let g = b.glow(&up.base_glow, &all);
                    pass(encoder, &[(&t.seeds[2].1, CLEAR)], &self.seed_gi, &[main, &g], "light field seeds");
                    let mut from = 2;
                    for i in 0..up.jumps {
                        let to = 5 - from;
                        let jg = b.main(&self.uniforms[i + 1], &t.field.1, &t.seeds[from].1);
                        pass(encoder, &[(&t.seeds[to].1, CLEAR)], &self.jump, &[&jg], "light field jump");
                        from = to;
                    }
                    let sg = b.main(&self.uniforms[0], &t.field.1, &t.seeds[from].1);
                    let g = b.glow(&up.base_glow, &GlowInputs { gi_dist: none, ..all });
                    pass(encoder, &[(&t.gi_dist.1, CLEAR)], &self.gi_dist, &[&sg, &g], "light field distance");
                }
                // The cascades, the farthest first, each merging the one above it.
                let mut into = 0;
                for i in (0..count).rev() {
                    let above = if i + 1 == count { none } else { &cascades[1 - into].1 };
                    let u = &up.cascade_uniforms[run * count as usize + i as usize];
                    let g = b.glow(u, &GlowInputs { above, ..all });
                    pass(encoder, &[(&cascades[into].1, CLEAR)], &self.cascade, &[main, &g], "light cascade");
                    into = 1 - into;
                }
                let g = b.glow(
                    &up.run_uniforms[run],
                    &GlowInputs { above: &cascades[1 - into].1, gi: [none, none, none], ..all },
                );
                let targets = [(&t.gi[0].1, CLEAR), (&t.gi[1].1, CLEAR), (&t.gi[2].1, CLEAR)];
                pass(encoder, &targets, &self.resolve, &[main, &g], "light field resolve");
            }
        }

        // The lights' bloom: the lamps and what gives off light, then smaller and smaller.
        if plan.blooms_on {
            let levels = &t.glow.1;
            let g = b.glow(&up.base_glow, &GlowInputs { glow: none, ..all });
            pass(encoder, &[(&levels[0], CLEAR)], &self.glow_source, &[main, &g], "light bloom");
            for level in 1..GLOW_LEVELS as usize {
                let g = b.glow(&up.level_uniforms[level - 1], &GlowInputs { glow: &levels[level - 1], ..all });
                pass(encoder, &[(&levels[level], CLEAR)], &self.glow_down, &[main, &g], "light bloom down");
            }
        }

        // The balls stirring the haze, then the extras: the haze's density and the memory.
        if plan.stirring
            && let Some(stir) = t.stir.as_ref()
        {
            let group = b.get(&wgpu::BindGroupDescriptor {
                label: Some("haze stir"),
                layout: &self.stir_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms[0].as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 12,
                        resource: self.stirs.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                    },
                ],
            });
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("haze stir"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &stir.1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: CLEAR, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(&self.stir);
            p.set_bind_group(0, &group, &[]);
            p.draw(0..4, 0..scene.stirs.len() as u32);
        }
        // The memory: last frame's in one extras picture, this frame's drawn into the other.
        let (memory_from, memory_to) = (self.memory.current, 1 - self.memory.current);
        if plan.extras_on
            && let Some(extras) = t.extras.as_ref()
        {
            let inputs = GlowInputs { above: &extras[memory_from].1, extras: none, ..all };
            let g = b.inputs(&self.extras_layout, EXTRAS_BINDINGS, &up.base_glow, &inputs);
            pass(encoder, &[(&extras[memory_to].1, CLEAR)], &self.extras, &[main, &g], "light extras");
        }
    }

    /// The bake's bounce light (part 3, Stage): traced, cleaned up, added to the background light.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bounce_passes(
        &self,
        b: &Binder,
        encoder: &mut wgpu::CommandEncoder,
        scene: &LightScene,
        plan: &RunPlan,
        up: &Uploaded,
        all: &GlowInputs,
        (main, from, stage_group): (&wgpu::BindGroup, usize, &wgpu::BindGroup),
    ) {
        let t = b.t;
        let none = &t.dummy_colour.1;
        if plan.traced
            && let (Some(bg), Some(pics)) = (t.bg_glow.as_ref(), t.traced.as_ref())
        {
            let g = b.inputs(&self.mix_layout, MIX_BINDINGS, &up.base_glow, &GlowInputs { above: none, ..*all });
            pass(encoder, &[(&pics[0].1, CLEAR)], &self.trace, &[main, &g, stage_group], "bounce light");
            // Clean-up: steps 1, 2 and 4 (uniforms 0 to 2); without it a plain copy (uniform 3).
            let steps: &[usize] = if scene.settings.bake_more[1] > 0.5 { &[0, 1, 2] } else { &[3] };
            let mut pic = 0;
            for (k, &step) in steps.iter().enumerate() {
                let reads = b.on(&self.trace_uniforms[step], &pics[pic].1, &t.field.1, &t.seeds[from].1);
                if k + 1 == steps.len() {
                    let into = [(&bg.1, wgpu::LoadOp::Load)];
                    pass(encoder, &into, &self.trace_add, &[&reads, &g, stage_group], "bounce light added");
                } else {
                    let into = [(&pics[1 - pic].1, CLEAR)];
                    pass(encoder, &into, &self.denoise, &[&reads, &g, stage_group], "bounce clean-up");
                    pic = 1 - pic;
                }
            }
        }
    }

    /// The mix into `out`, then mirror walls, a glossy floor, then depth of field, each reading
    /// the one before; the light beams' bands go over the mixed light (so mirror walls, the floor
    /// and depth of field after it see them too).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mix_passes(
        &self,
        b: &Binder,
        encoder: &mut wgpu::CommandEncoder,
        scene: &LightScene,
        out: &wgpu::TextureView,
        plan: &RunPlan,
        up: &Uploaded,
        all: &GlowInputs,
        (from, stage_group): (usize, &wgpu::BindGroup),
    ) {
        let t = b.t;
        // The mix reads the background's own light where the cascades read the one above.
        let bg_view = t.bg_glow.as_ref().filter(|_| plan.into_bg).map_or(&t.dummy_colour.1, |b| &b.1);
        let g = b.inputs(&self.mix_layout, MIX_BINDINGS, &up.base_glow, &GlowInputs { above: bg_view, ..*all });
        let mut steps: Vec<(&wgpu::RenderPipeline, &str)> = vec![(&self.mix, "light")];
        if plan.mirror_walls {
            steps.push((&self.mirror, "mirror walls"));
        }
        if plan.reflects {
            steps.push((&self.floor, "glossy floor"));
        }
        if plan.blurs {
            steps.push((&self.dof, "depth of field"));
        }
        let spare: Vec<&wgpu::TextureView> =
            [t.stage.as_ref(), t.stage_more.as_ref()].into_iter().flatten().map(|p| &p.1).collect();
        let last = steps.len() - 1;
        let mut read: Option<&wgpu::TextureView> = None;
        for (i, (pipeline, label)) in steps.into_iter().enumerate() {
            let into = if i == last { out } else { spare[i % spare.len().max(1)] };
            let main_on = match read {
                Some(lit) => b.on(&self.uniforms[0], lit, &t.field.1, &t.seeds[from].1),
                None => b.main(&self.uniforms[0], &t.field.1, &t.seeds[from].1),
            };
            match (std::ptr::eq(pipeline, &self.dof), read, t.dof_small.as_ref()) {
                (true, Some(lit), Some(small)) => {
                    // Depth of field: first the small copy of what it blurs, then the blur reading it.
                    let copy = "depth of field copy";
                    pass(encoder, &[(&small.1, CLEAR)], &self.dof_small, &[&main_on, &g, stage_group], copy);
                    let with_copy = b.with(&self.uniforms[0], lit, &t.field.1, &t.seeds[from].1, &small.1);
                    pass(encoder, &[(into, CLEAR)], pipeline, &[&with_copy, &g, stage_group], label);
                }
                _ => pass(encoder, &[(into, CLEAR)], pipeline, &[&main_on, &g, stage_group], label),
            }
            if i == 0 && !scene.ribbons.is_empty() {
                // Light beams' bands, over the mixed light.
                let ribbons = b.get(&wgpu::BindGroupDescriptor {
                    label: Some("beam ribbons"),
                    layout: &self.ribbon_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 2, resource: buffer(&self.ribbons) }],
                });
                let mut p = crate::begin_pass(encoder, into, "beam ribbons", wgpu::LoadOp::Load);
                p.set_pipeline(&self.ribbon);
                p.set_bind_group(0, &main_on, &[]);
                p.set_bind_group(1, &self.empty_group, &[]);
                p.set_bind_group(2, &self.empty_group, &[]);
                p.set_bind_group(3, &ribbons, &[]);
                p.draw(0..4, 0..scene.ribbons.len().min(u32::MAX as usize) as u32);
            }
            read = Some(into);
        }
    }
}
