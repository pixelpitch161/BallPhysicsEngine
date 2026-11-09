//! Rendering a frame: the scene pass, lighting, effects, bloom and the grade into the output.

use super::*;

impl Renderer {
    /// Draws one frame into the output texture. Each blend layer is drawn into its own picture
    /// and mixed in where its z-order and place put it among the frame's own things (the
    /// overlay's layers among the overlay's things, the same way).
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let scened = self.render_scene(device, queue, &mut encoder, frame);
        self.finish(device, queue, encoder, frame, scened);
    }

    /// Draws a baked frame (lighting part 3): every sample's scene, lit, averaged, then the
    /// picture's effects, bloom and grade once, with the last sample's frame (the frame's own
    /// moment). Each sample is moved within the pixel by [`bake::jitter`] and drawn at the
    /// lighting's Bake quality with its own random rays; effects that remember count every sample
    /// as the last one's moment. One sample (or none baked) is exactly [`Renderer::render`].
    pub fn render_samples(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, samples: &[Frame]) {
        let Some(last) = samples.last() else { return };
        if samples.len() == 1 && last.bake.is_none() {
            self.render(device, queue, last);
            return;
        }
        let count = samples.len() as u32;
        let size = (self.targets.width, self.targets.height);
        self.chain.effect_time = Some(last.time);
        let mut scened = None;
        let mut light = None;
        let mut frames = Vec::with_capacity(samples.len());
        for (index, sample) in samples.iter().enumerate() {
            let index = index as u32;
            let mut f = sample.clone();
            f.bake = Some(bake::BakeSample { index, count });
            let [jx, jy] = bake::jitter(index);
            let px = self.pixel_size(&f) / f.camera.zoom.max(1e-6);
            let (sin, cos) = f.camera.turn.sin_cos();
            f.camera.center.x += (jx * cos - jy * sin) * px;
            f.camera.center.y += (jx * sin + jy * cos) * px;
            if let Some(light) = f.light.as_mut() {
                light.settings.more[2] = 4.0;
                light.settings.dof[3] = light.settings.bake[2].max(1.0);
            }
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sample") });
            let s = self.render_scene(device, queue, &mut encoder, &f);
            // Effects on the light read the unlit picture and the lit colour beside the picture:
            // averaged the same way, so they match it at blurred and smoothed edges.
            light = self.light_pictures(&f, s.moved);
            scened = Some(s);
            let acc = self.accumulate.get_or_insert_with(|| accumulate::Accumulate::new(device));
            acc.add(device, queue, &mut encoder, size, index, 0, &self.targets.hdr);
            if let Some((unlit, lit)) = &light {
                acc.add(device, queue, &mut encoder, size, index, 1, unlit);
                acc.add(device, queue, &mut encoder, size, index, 2, lit);
            }
            queue.submit([encoder.finish()]);
            frames.push(f);
        }
        self.chain.effect_time = None;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("baked") });
        if let Some(acc) = self.accumulate.as_ref() {
            acc.put(device, &mut encoder, 0, &self.targets.hdr);
            if let Some((unlit, lit)) = &light {
                acc.put(device, &mut encoder, 1, unlit);
                acc.put(device, &mut encoder, 2, lit);
            }
        }
        let last = frames.last().expect("one at least");
        self.finish(device, queue, encoder, last, scened.expect("one at least"));
    }

    /// The pictures effects on the light read beside the picture, once `frame` is drawn: the
    /// picture before lighting and the lit colour (seen through the Stage camera when `moved`).
    pub(crate) fn light_pictures(&self, frame: &Frame, moved: bool) -> Option<(wgpu::TextureView, wgpu::TextureView)> {
        match (frame.light.as_ref(), self.lighting.as_ref().and_then(|l| l.pictures())) {
            (Some(_), Some(_)) if moved => {
                let lit = &self.stage_camera.as_ref().expect("moved").seen()[2].1;
                Some((self.targets.behind_view.clone(), lit.clone()))
            }
            (Some(_), Some([(_, lit), _, _])) => Some((self.targets.behind_view.clone(), lit)),
            _ => None,
        }
    }

    /// Makes the lighting pass from the store's lighting pipelines, unless it is made. False while
    /// a background store is still building them (the renderer then counts as waiting).
    pub(crate) fn lighting_ready(&mut self, device: &wgpu::Device) -> bool {
        if self.lighting.is_some() {
            return true;
        }
        let Some(kit) = self.programs.lock().light_kit(device) else {
            self.waiting = true;
            return false;
        };
        self.lighting = Some(lighting::Lighting::with_kit(device, &kit));
        true
    }

    /// Draws `frame`'s scene, lit, into the HDR picture: everything before the picture's effects.
    pub(crate) fn render_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
    ) -> Scened {
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals::new(frame, &frame.camera, self.targets.width)),
        );
        let overlay = frame.overlay.as_deref().filter(|_| frame.has_overlay());
        // The frame's time and hits, for effects inside balls.
        let fx = effects::chain::base_uniform(frame, frame.width, frame.height);
        if let Some(o) = overlay {
            let identity = CameraView::default();
            queue.write_buffer(
                &self.screen_globals,
                0,
                bytemuck::bytes_of(&Globals::new(frame, &identity, self.targets.width)),
            );
            self.overlay.upload(device, queue, &self.layers_layout, &self.sampler, &fx, o);
        }
        self.buffers.upload(device, queue, &self.layers_layout, &self.sampler, &fx, frame);
        // Every blend layer's lists, the frame's then the overlay's, each in its own buffers.
        let overlay_layers = overlay.map_or(&[][..], |o| &o.layers[..]);
        let layers: Vec<&BlendLayer> = frame.layers.iter().chain(overlay_layers).collect();
        let mut layer_buffers = std::mem::take(&mut self.layer_buffers);
        layer_buffers.resize_with(layer_buffers.len().max(layers.len()), FrameBuffers::default);
        for (b, l) in layer_buffers.iter_mut().zip(&layers) {
            b.upload(device, queue, &self.layers_layout, &self.sampler, &fx, &l.frame);
        }
        // Their soft fields of particles, at half the output's size.
        let size = (self.targets.width, self.targets.height);
        if let Some(o) = overlay {
            self.overlay.fields.upload(device, queue, &self.fields, o, size);
        }
        self.buffers.fields.upload(device, queue, &self.fields, frame, size);
        for (b, l) in layer_buffers.iter_mut().zip(&layers) {
            b.fields.upload(device, queue, &self.fields, &l.frame, size);
        }
        // Their Huge swarms' runs.
        if let Some(o) = overlay {
            self.overlay.swarms.upload(device, queue, &self.swarms, &o.swarms, &o.swarms_front);
        }
        self.buffers.swarms.upload(device, queue, &self.swarms, &frame.swarms, &frame.swarms_front);
        for (b, l) in layer_buffers.iter_mut().zip(&layers) {
            b.swarms.upload(device, queue, &self.swarms, &l.frame.swarms, &l.frame.swarms_front);
        }
        // The programs of stacks with shader layers, built the first time a frame names them.
        let frames = std::iter::once((frame, None))
            .chain(overlay.map(|o| (o, None)))
            .chain(layers.iter().map(|l| (&l.frame, l.each.then_some(l.blend))));
        self.waiting = self.programs.lock().ensure(device, &self.layered_pipeline_layout, frames);
        self.ripples.prepare(device, queue, encoder, frame);
        let liquids = liquid::all_in(frame);
        if !liquids.is_empty() || self.liquid.is_some() {
            let pass = self.liquid.get_or_insert_with(|| liquid::LiquidPass::new(device, &self.globals_layout));
            let size = (self.targets.width, self.targets.height);
            pass.prepare(device, queue, liquids.into_iter(), size, &self.targets.behind_view);
        }
        self.paint.sync(device, queue, &self.paint_layout, &self.sampler, &frame.gradients, &self.ripples);
        self.paint.bind_ripples(device, &self.paint_layout, &self.sampler, &self.ripples);
        self.grade_map.sync(queue, &frame.grade);
        if let Some(o) = overlay {
            self.overlay_paint.sync(device, queue, &self.paint_layout, &self.sampler, &o.gradients, &self.ripples);
            self.overlay_paint.bind_ripples(device, &self.paint_layout, &self.sampler, &self.ripples);
        }

        if !layers.is_empty() {
            self.targets.ensure_layer(device);
        }
        let wants_objects = effects::chain::Chain::wants_objects(frame);
        let material = effects::chain::Chain::material(frame);
        if wants_objects {
            self.targets.ensure_object(device);
        }
        self.chain.begin_frame();
        // Counts the blend composites this frame.
        let mut slot = 0;
        let globals = self.globals_group.clone();
        let screen_globals = self.screen_globals_group.clone();
        // A moved Stage camera swings the stage, not what is stuck to the screen: that is drawn
        // after the camera pass, over what it saw.
        let moving = frame.stage_view().filter(|v| v.motion > 0.0);
        let staged_overlay = if moving.is_some() { None } else { overlay };
        self.draw_scene(
            device,
            queue,
            encoder,
            frame,
            staged_overlay,
            &layer_buffers,
            &mut slot,
            (wants_objects, material),
            (&globals, &screen_globals),
        );
        // A lit scene: the things again as their lit colour, then as their surface, each into
        // its own picture, then the light mixed into the HDR picture.
        self.lit_lights = frame.light.as_ref().map(|scene| scene.lights.len());
        let mut timed = false;
        let mut moved = false;
        // A lit frame whose lighting shaders are still being built is drawn unlit for now.
        let lit = frame.light.as_ref().filter(|_| self.lighting_ready(device));
        if let Some(scene) = lit {
            let timer = self.light_timer.get_or_insert_with(|| timing::LightTimer::new(device, queue));
            if let Some(t) = timer {
                t.start(encoder);
            }
            let lighting = self.lighting.as_mut().expect("ready, checked above");
            let video_width = if self.field_like_video { frame.width } else { self.targets.width };
            lighting.ensure(device, self.targets.width, self.targets.height, video_width, scene);
            lighting.tiles_on = self.light_tiles;
            let pictures = lighting.pictures().expect("just made");
            // With nothing giving off light its picture is left black (the bounce and the
            // blooms read it), not as the last frame that had some.
            if !scene.glows {
                drop(begin_pass(
                    encoder,
                    &pictures[2].1,
                    "no light given off",
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                ));
            }
            // Lit colour, surface, and (when something gives off light) the light things give off;
            // part 4's what glows under UV and the tags, when asked for.
            // Glass and light part 1: the tint picture, when something see-through colours the light.
            let tints = scene.needs_tint(frame);
            if tints {
                lighting.make_tint(device);
            }
            lighting.keep_uv_and_tags(device, scene.needs_uv(), scene.needs_tags());
            // Passes LIT to TINT; BACKGROUND and BEHIND are drawn on their own below.
            let wanted = [true, true, scene.glows, false, scene.needs_uv(), scene.needs_tags(), false, tints];
            let modes: Vec<(u32, (wgpu::Texture, wgpu::TextureView))> = (light_pass::LIT..=light_pass::TINT)
                .filter(|mode| wanted[*mode as usize - 1])
                .map(|mode| (mode, lighting.picture(mode).expect("just made")))
                .collect();
            // A background that gives off light is a lamp: it doesn't take light.
            let background_lit = if scene.settings.background_glows() { 0.0 } else { scene.settings.ambient[3] };
            for (mode, (texture, view)) in modes {
                let (world, screen) = self.light_globals(device, queue, frame, mode, background_lit, scene);
                let hdr_texture = std::mem::replace(&mut self.targets.hdr_texture, texture);
                let hdr = std::mem::replace(&mut self.targets.hdr, view);
                self.light_mode = mode;
                self.light_stage = scene.settings.stage[0] > 0.5;
                self.draw_scene(
                    device,
                    queue,
                    encoder,
                    frame,
                    staged_overlay,
                    &layer_buffers,
                    &mut slot,
                    (false, false),
                    (&world, &screen),
                );
                self.light_mode = light_pass::PICTURE;
                self.targets.hdr_texture = hdr_texture;
                self.targets.hdr = hdr;
            }
            // Glass and light part 3: free glass, when photons are sent through it.
            if bent_light::BentPlan::of(frame, scene).free_glass() {
                let lighting = self.lighting.as_mut().expect("made above");
                lighting.make_glass(device);
                let (texture, view) = lighting.picture(light_pass::GLASS).expect("just made");
                let (world, _) = self.light_globals(device, queue, frame, light_pass::GLASS, background_lit, scene);
                let hdr_texture = std::mem::replace(&mut self.targets.hdr_texture, texture);
                let hdr = std::mem::replace(&mut self.targets.hdr, view);
                self.light_mode = light_pass::GLASS;
                self.draw_glass(encoder, frame, &layer_buffers, &world);
                self.light_mode = light_pass::PICTURE;
                self.targets.hdr_texture = hdr_texture;
                self.targets.hdr = hdr;
            }
            // What glass shows behind it: the plain picture without what is see-through
            // (part 2b; `Globals.light.x` -1 makes see-through records draw nothing).
            if let Some((texture, view)) = self.lighting.as_mut().expect("made above").behind(device, scene) {
                let (world, screen) =
                    self.light_globals(device, queue, frame, light_pass::BEHIND, background_lit, scene);
                let hdr_texture = std::mem::replace(&mut self.targets.hdr_texture, texture);
                let hdr = std::mem::replace(&mut self.targets.hdr, view);
                self.light_mode = light_pass::BEHIND;
                self.draw_scene(
                    device,
                    queue,
                    encoder,
                    frame,
                    overlay,
                    &layer_buffers,
                    &mut slot,
                    (false, false),
                    (&world, &screen),
                );
                self.light_mode = light_pass::PICTURE;
                self.targets.hdr_texture = hdr_texture;
                self.targets.hdr = hdr;
            }
            // The background's own light, alone.
            if scene.settings.background_glows() {
                let (world, _) =
                    self.light_globals(device, queue, frame, light_pass::BACKGROUND, background_lit, scene);
                let lighting = self.lighting.as_ref().expect("made above");
                let view = lighting.background_glow().expect("made for a glowing background");
                let mut pass = begin_pass(encoder, &view, "background glow", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_bind_group(0, &world, &[]);
                pass.set_bind_group(1, &self.paint.group, &[]);
                pass.set_pipeline(&self.background);
                pass.draw(0..3, 0..1);
            }
            self.copy_under(encoder);
            let lighting = self.lighting.as_mut().expect("made above");
            lighting.run(
                device,
                queue,
                encoder,
                frame,
                scene,
                self.targets.width,
                &self.targets.behind_view,
                &self.targets.hdr,
                scene.settings.background_glows(),
                tints,
            );
            // A moved Stage camera sees the lit picture again: the picture, the picture before
            // lighting and the lit colour are then the moved camera's.
            if let Some(view) = moving {
                let lighting = self.lighting.as_ref().expect("made above");
                let pass = self
                    .stage_camera
                    .get_or_insert_with(|| stage_camera_pass::StageCameraPass::with_kit(device, lighting.stage_kit()));
                pass.ensure(device, self.targets.width, self.targets.height);
                {
                    let mut background = begin_pass(
                        encoder,
                        pass.background(),
                        "background alone",
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    );
                    background.set_bind_group(0, &self.globals_group, &[]);
                    background.set_bind_group(1, &self.paint.group, &[]);
                    background.set_pipeline(&self.background);
                    background.draw(0..3, 0..1);
                }
                // Draft takes one ray a pixel; better qualities five at edges.
                let samples = if scene.settings.more[2] < 0.5 { 1 } else { 5 };
                let lit_colour = &lighting.pictures().expect("made above")[0].1;
                pass.run(device, queue, encoder, lighting, &view, samples, &self.targets.hdr, lit_colour);
                let [picture, unlit, _] = pass.seen();
                let extent =
                    wgpu::Extent3d { width: self.targets.width, height: self.targets.height, depth_or_array_layers: 1 };
                encoder.copy_texture_to_texture(
                    picture.0.as_image_copy(),
                    self.targets.hdr_texture.as_image_copy(),
                    extent,
                );
                encoder.copy_texture_to_texture(unlit.0.as_image_copy(), self.targets.behind.as_image_copy(), extent);
                moved = true;
            }
            if let Some(Some(t)) = self.light_timer.as_mut() {
                timed = t.end(encoder);
            }
        }
        if moving.is_some()
            && let Some(o) = overlay
        {
            self.draw_overlay(device, queue, encoder, frame, o, &layer_buffers, &mut slot, &screen_globals);
        }
        self.layer_buffers = layer_buffers;
        Scened { wants_objects, material, moved, timed }
    }

    /// The picture's effects, bloom and grade over the HDR picture `render_scene` drew for
    /// `frame`, into the output.
    pub(crate) fn finish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        frame: &Frame,
        scened: Scened,
    ) {
        let Scened { wants_objects, material, moved, timed } = scened;
        let above = BlendLayer::above;
        let object_view = self.targets.object.as_ref().filter(|_| wants_objects).map(|(_, v)| v.clone());
        // A material drew the layers' own object pictures over the full one: effects on the
        // whole picture see every thing again.
        if material && frame.layers.iter().any(above) && effects::chain::Chain::picture_wants_objects(frame) {
            let view = &self.targets.object.as_ref().expect("made above").1;
            let mut pass = begin_pass(&mut encoder, view, "objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            let layer_buffers = std::mem::take(&mut self.layer_buffers);
            self.draw_all_objects(&mut pass, frame, &layer_buffers, true);
            self.layer_buffers = layer_buffers;
        }
        // Effects that read where things are see them where the moved camera shows them.
        let object_view = match (moved, object_view) {
            (true, Some(front)) => {
                self.stage_camera.as_mut().and_then(|p| p.see_objects(device, &mut encoder, &front)).or(Some(front))
            }
            (_, view) => view,
        };
        // Effects on the light read the picture before lighting and the lit colour.
        let light_pictures = self.light_pictures(frame, moved);
        self.chain.set_light(light_pictures);
        self.chain.run(
            device,
            queue,
            &mut encoder,
            Stage::Picture,
            frame,
            effects::chain::Canvas::Main,
            &self.targets.hdr_texture,
            &self.targets.hdr,
            object_view.as_ref(),
        );
        self.chain.end_frame();
        self.waiting |= self.chain.waiting;
        // Bloom at 0 adds nothing (review S24): its chain is skipped, and the picture the
        // composite reads it from is left black (0 × black is 0, as 0 × any finite bloom was).
        if frame.bloom != 0.0 {
            for level in 0..BLOOM_LEVELS {
                let mut pass = begin_pass(
                    &mut encoder,
                    &self.targets.bloom[level],
                    "bloom down",
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                );
                pass.set_pipeline(&self.downsample);
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_bind_group(1, &self.targets.down_sources[level], &[]);
                pass.draw(0..3, 0..1);
            }
            for level in (0..BLOOM_LEVELS - 1).rev() {
                let mut pass = begin_pass(&mut encoder, &self.targets.bloom[level], "bloom up", wgpu::LoadOp::Load);
                pass.set_pipeline(&self.upsample);
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_bind_group(1, &self.targets.up_sources[level], &[]);
                pass.draw(0..3, 0..1);
            }
            self.bloom_black = false;
        } else if !self.bloom_black {
            drop(begin_pass(&mut encoder, &self.targets.bloom[0], "no bloom", wgpu::LoadOp::Clear(wgpu::Color::BLACK)));
            self.bloom_black = true;
        }
        {
            let mut pass = begin_pass(
                &mut encoder,
                &self.targets.output_view,
                "composite",
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.targets.composite, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        if timed && let Some(Some(t)) = &self.light_timer {
            t.read_after_submit();
        }
    }

    /// The world's and the screen's globals for light pass `mode` (1 to 6, [`light_pass::BEHIND`],
    /// [`light_pass::TINT`] or [`light_pass::GLASS`]),
    /// made the first time.
    pub(crate) fn light_globals(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        mode: u32,
        background_lit: f32,
        scene: &LightScene,
    ) -> (wgpu::BindGroup, wgpu::BindGroup) {
        while self.light_globals.len() < 2 * light_pass::GLASS as usize {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("light pass globals"),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("light pass globals"),
                layout: &self.globals_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.light_globals.push((buffer, group));
        }
        let at = 2 * (mode as usize - 1);
        let mut world = Globals::new(frame, &frame.camera, self.targets.width);
        world.light = [mode as f32, background_lit, scene.settings.stage[0], scene.background_link];
        if mode == light_pass::BEHIND {
            // The plain picture, but what is see-through left out (light.x -1, see layers.wgsl).
            world.light = [-1.0, background_lit, scene.settings.stage[0], 0.0];
        }
        world.light_emit = scene.settings.background;
        let mut screen = Globals::new(frame, &CameraView::default(), self.targets.width);
        screen.light = world.light;
        screen.light_emit = world.light_emit;
        queue.write_buffer(&self.light_globals[at].0, 0, bytemuck::bytes_of(&world));
        queue.write_buffer(&self.light_globals[at + 1].0, 0, bytemuck::bytes_of(&screen));
        (self.light_globals[at].1.clone(), self.light_globals[at + 1].1.clone())
    }

    /// Copies the HDR picture into `behind`: what a blend layer is mixed with.
    pub(crate) fn copy_under(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_texture(
            self.targets.hdr_texture.as_image_copy(),
            self.targets.behind.as_image_copy(),
            wgpu::Extent3d { width: self.targets.width, height: self.targets.height, depth_or_array_layers: 1 },
        );
    }
}
