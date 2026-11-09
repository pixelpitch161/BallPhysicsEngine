//! The draw calls inside the passes: backgrounds, balls, things, sprites, liquids, glass, the
//! overlay and blend layers.

use super::*;

impl Renderer {
    /// Draws the frame's things into the HDR picture: the background, the layers under
    /// everything, trails and shapes, the layers behind the balls, the balls, the effects on
    /// things, flashes and what is in front, the layers above, then the things stuck to the
    /// screen. `slot` counts the blend composites. `(wants_objects, material)` are the effects'
    /// needs (both false in the light passes, which run no effects); `globals` the world's and
    /// the screen's globals.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        overlay: Option<&Frame>,
        layer_buffers: &[FrameBuffers],
        slot: &mut usize,
        (wants_objects, material): (bool, bool),
        (globals, screen_globals): (&wgpu::BindGroup, &wgpu::BindGroup),
    ) {
        let world = Through {
            globals: globals.clone(),
            paint: self.paint.group.clone(),
            world: true,
            camera: frame.camera,
            each: None,
        };
        {
            let mut pass =
                begin_pass(encoder, &self.targets.hdr, "background", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }
        // Things of a z-order below 0 draw under everything else.
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| l.z < 0) {
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, None, false);
        }
        self.fields.splat(encoder, globals, &self.buffers.fields, false);
        {
            let mut pass = begin_pass(encoder, &self.targets.hdr, "scene", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_back(&mut pass, frame, &self.buffers, &world);
        }
        let hdr = self.targets.hdr.clone();
        self.draw_liquids(encoder, frame, false, globals, &hdr);
        // Blended things behind the balls (shapes) mix with what is behind them before the balls
        // are drawn over them.
        let behind = |l: &BlendLayer| l.z == 0 && l.behind();
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| behind(l)) {
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, None, false);
        }
        {
            let mut pass = begin_pass(encoder, &self.targets.hdr, "balls", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_balls(&mut pass, frame, &self.buffers, &world);
        }
        // Blended things behind the balls go under the rest; other blended things are drawn
        // above the rest, so they are on top here too. With a material on the scene, those
        // above get it on their own layers (see `draw_layer`), so the main picture's leaves them
        // out and the picture under a blended ball is left alone.
        let above = BlendLayer::above;
        if wants_objects {
            let view = &self.targets.object.as_ref().expect("made above").1;
            let mut pass = begin_pass(encoder, view, "objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_all_objects(&mut pass, frame, layer_buffers, !material);
        }
        let object_view = self.targets.object.as_ref().filter(|_| wants_objects).map(|(_, v)| v.clone());
        // The light passes run no effects: what effects paint counts as unlit.
        if self.light_mode == light_pass::PICTURE {
            self.chain.run(
                device,
                queue,
                encoder,
                Stage::Things,
                frame,
                effects::chain::Canvas::Main,
                &self.targets.hdr_texture,
                &self.targets.hdr,
                object_view.as_ref(),
            );
        }
        if !frame.flashes.is_empty()
            || !frame.trails_front.is_empty()
            || !frame.particles_front.is_empty()
            || !frame.fields_front.is_empty()
            || !frame.swarms_front.is_empty()
        {
            self.fields.splat(encoder, globals, &self.buffers.fields, true);
            let mut pass = begin_pass(encoder, &self.targets.hdr, "flashes", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            self.draw_front(&mut pass, frame, &self.buffers, &world);
        }
        self.draw_liquids(encoder, frame, true, globals, &hdr);
        // Then the blended things of z-order 0 and everything of a higher z-order, in order.
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| above(l)) {
            let objects = object_view.as_ref();
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, objects, material);
        }
        if let Some(o) = overlay {
            self.draw_overlay(device, queue, encoder, frame, o, layer_buffers, slot, screen_globals);
        }
    }

    /// Light pass 9 (glass and light part 3): the sprites and liquids of the frame and its blend
    /// layers' frames into the glass picture (the HDR picture's place), where only free glass
    /// draws (sprites.wgsl, liquid.wgsl). Its own place in the picture doesn't matter: whatever is
    /// solid over or under it wins when it is marked in the surface picture.
    pub(crate) fn draw_glass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        layer_buffers: &[FrameBuffers],
        globals: &wgpu::BindGroup,
    ) {
        let frames =
            std::iter::once((frame, &self.buffers)).chain(frame.layers.iter().map(|l| &l.frame).zip(layer_buffers));
        {
            let mut pass =
                begin_pass(encoder, &self.targets.hdr, "free glass", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            for (f, b) in frames {
                self.sprites.draw_range(&mut pass, &b.particles, 0..f.particles.len() as u32, None);
                self.sprites.draw_range(&mut pass, &b.particles_front, 0..f.particles_front.len() as u32, None);
            }
        }
        let hdr = self.targets.hdr.clone();
        for f in std::iter::once(frame).chain(frame.layers.iter().map(|l| &l.frame)) {
            self.draw_liquids(encoder, f, false, globals, &hdr);
            self.draw_liquids(encoder, f, true, globals, &hdr);
        }
    }

    /// Draws the things stuck to the screen (the overlay frame `o`) into the HDR picture over
    /// everything else, the overlay's blend layers in their own z-order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        o: &Frame,
        layer_buffers: &[FrameBuffers],
        slot: &mut usize,
        screen_globals: &wgpu::BindGroup,
    ) {
        let screen = Through {
            globals: screen_globals.clone(),
            paint: self.overlay_paint.group.clone(),
            world: false,
            camera: CameraView::default(),
            each: None,
        };
        let buffers = std::mem::take(&mut self.overlay);
        let hdr = self.targets.hdr.clone();
        // In the same order as the frame's own: lower z-orders, the overlay's trails and
        // shapes, the layers behind its balls, its balls and flashes, then the rest.
        let own = &layer_buffers[frame.layers.len()..];
        let mut layers = |r: &mut Self, encoder: &mut wgpu::CommandEncoder, which: fn(&BlendLayer) -> bool| {
            for (l, b) in o.layers.iter().zip(own).filter(|(l, _)| which(l)) {
                let through = r.layer_through(device, queue, frame, l, None, &screen);
                r.draw_layer(device, queue, encoder, l, b, &through, *slot, None);
                *slot += 1;
            }
        };
        layers(self, encoder, |l| l.z < 0);
        self.draw_things_part(encoder, o, &buffers, &screen, &hdr, false);
        layers(self, encoder, |l| l.z == 0 && l.behind());
        self.draw_things_part(encoder, o, &buffers, &screen, &hdr, true);
        layers(self, encoder, |l| l.z > 0 || (l.z == 0 && !l.behind()));
        self.overlay = buffers;
    }

    /// Trails and shapes: what sits behind the rest.
    pub(crate) fn draw_back(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        use each::Kind;
        if !f.trails.is_empty() {
            pass.set_pipeline(self.pipeline(&self.trail, Kind::Trail, through));
            pass.set_vertex_buffer(0, b.trails.slice());
            pass.draw(0..f.trails.len() as u32, 0..1);
        }
        if !f.particles.is_empty() || !f.fields.is_empty() || !f.swarms.is_empty() {
            self.draw_sprites(pass, b, f, false, through);
            // The sprites bind only the globals, the fields their own: put the paint back for
            // what follows.
            pass.set_bind_group(1, &through.paint, &[]);
        }
        if let Some(records) = b.layers_group.as_ref().filter(|_| !f.paths.is_empty()) {
            let plain = self.pipeline(&self.path, Kind::Path, through);
            pass.set_bind_group(2, records, &[]);
            pass.set_vertex_buffer(0, b.paths.slice());
            let each = through.each.map(|b| b.shader_code());
            // Runs of shapes drawn with one pipeline: their Shader layers' program, or, without one
            // (or one not built yet), the plain pipeline, which leaves shader records out.
            let mut start = 0;
            while start < f.paths.len() {
                let program = f.paths[start].layers[2];
                let end =
                    f.paths[start..].iter().position(|p| p.layers[2] != program).map_or(f.paths.len(), |n| start + n);
                let made =
                    f.programs.get(program as usize).and_then(|p| self.programs.lock().shape_pipeline(p.key, each));
                pass.set_pipeline(made.as_ref().unwrap_or(plain));
                pass.draw(0..4, start as u32..end as u32);
                start = end;
            }
        }
    }

    /// `normal`, or in a layer whose things blend with each other, that layer's pipeline for
    /// `kind` (made by [`each::EachPipelines::ensure`] before the layer is drawn).
    pub(crate) fn pipeline<'a>(
        &'a self,
        normal: &'a wgpu::RenderPipeline,
        kind: each::Kind,
        through: &Through,
    ) -> &'a wgpu::RenderPipeline {
        match through.each {
            Some(blend) => self.each.get(kind, blend.shader_code()),
            None => normal,
        }
    }

    /// The balls.
    pub(crate) fn draw_balls(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if let Some(group) = b.layers_group.as_ref().filter(|_| !f.layered.is_empty()) {
            let plain = self.pipeline(&self.layered, each::Kind::Layered, through);
            pass.set_bind_group(2, group, &[]);
            pass.set_vertex_buffer(0, b.layered.slice());
            let all = [LayeredRun { program: None, start: 0, end: f.layered.len() as u32 }];
            let runs = if f.layered_runs.is_empty() { &all[..] } else { &f.layered_runs[..] };
            let each = through.each.map(|b| b.shader_code());
            for run in runs {
                // A program that didn't build draws with the plain pipeline, leaving shader layers out.
                let program = run.program.and_then(|i| f.programs.get(i as usize));
                let built = program.and_then(|p| self.programs.lock().pipeline(p.key, each));
                let pipeline = built.as_ref().unwrap_or(plain);
                let end = run.end.min(f.layered.len() as u32);
                if run.start < end {
                    pass.set_pipeline(pipeline);
                    pass.draw(0..4, run.start..end);
                }
            }
        }
    }

    /// Flashes and particles, in front of everything; the globals are bound already.
    pub(crate) fn draw_front(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if !f.flashes.is_empty() {
            pass.set_pipeline(self.pipeline(&self.flash, each::Kind::Flash, through));
            pass.set_bind_group(1, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.flashes.slice());
            pass.draw(0..4, 0..f.flashes.len() as u32);
        }
        if !f.trails_front.is_empty() {
            pass.set_pipeline(self.pipeline(&self.trail, each::Kind::Trail, through));
            pass.set_bind_group(1, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.trails_front.slice());
            pass.draw(0..f.trails_front.len() as u32, 0..1);
        }
        if !f.particles_front.is_empty() || !f.fields_front.is_empty() || !f.swarms_front.is_empty() {
            self.draw_sprites(pass, b, f, true, through);
        }
    }

    /// The sprites of `f`'s list behind the balls (`front` false) or in front, with its fields
    /// shaded and its swarms drawn among them where each one's `at` says (a field before a swarm
    /// at the same place).
    pub(crate) fn draw_sprites(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        b: &FrameBuffers,
        f: &Frame,
        front: bool,
        through: &Through,
    ) {
        let (buffer, count, fields, swarms) = if front {
            (&b.particles_front, f.particles_front.len() as u32, &f.fields_front, &f.swarms_front)
        } else {
            (&b.particles, f.particles.len() as u32, &f.fields, &f.swarms)
        };
        // Fields and swarms in the order they sit, a field first where both sit at once.
        let mut between: Vec<(u32, bool, usize)> = fields.iter().enumerate().map(|(k, x)| (x.at, false, k)).collect();
        between.extend(swarms.iter().enumerate().map(|(k, x)| (x.at, true, k)));
        between.sort_by_key(|&(at, swarm, k)| (at, swarm, k));
        let mut start = 0;
        for (at, swarm, k) in between {
            let at = at.clamp(start, count);
            self.sprites.draw_range(pass, buffer, start..at, through.each);
            start = at;
            if swarm {
                self.swarms.draw(pass, &b.swarms, front, k, through.each);
            } else {
                self.fields.composite(pass, &b.fields, front, k, through.each);
            }
        }
        self.sprites.draw_range(pass, buffer, start..count, through.each);
    }

    /// The frame's object picture: the layers drawn before its own things, its own things, then,
    /// when `above`, the layers drawn after them.
    pub(crate) fn draw_all_objects(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &Frame,
        layers: &[FrameBuffers],
        above: bool,
    ) {
        for (l, b) in frame.layers.iter().zip(layers).filter(|(l, _)| !l.above()) {
            self.draw_objects(pass, &l.frame, b);
        }
        self.draw_objects(pass, frame, &self.buffers);
        for (l, b) in frame.layers.iter().zip(layers).filter(|(l, _)| above && l.above()) {
            self.draw_objects(pass, &l.frame, b);
        }
    }

    pub(crate) fn draw_objects(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers) {
        if let Some(records) = b.layers_group.as_ref().filter(|_| !f.paths.is_empty()) {
            pass.set_pipeline(&self.path_object);
            pass.set_bind_group(2, records, &[]);
            pass.set_vertex_buffer(0, b.paths.slice());
            pass.draw(0..4, 0..f.paths.len() as u32);
        }
        if let Some(group) = b.layers_group.as_ref().filter(|_| !f.layered.is_empty()) {
            pass.set_pipeline(&self.layered_object);
            pass.set_bind_group(2, group, &[]);
            pass.set_vertex_buffer(0, b.layered.slice());
            pass.draw(0..4, 0..f.layered.len() as u32);
        }
    }

    /// Draws everything in `f` into `view` over what is there, in the usual order: trails,
    /// shapes, balls, flashes and particles. Used for the things stuck to the
    /// screen and for each blend layer.
    pub(crate) fn draw_things(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        b: &FrameBuffers,
        through: &Through,
        view: &wgpu::TextureView,
    ) {
        self.draw_things_part(encoder, f, b, through, view, false);
        self.draw_things_part(encoder, f, b, through, view, true);
    }

    /// One half of [`Renderer::draw_things`]: the trails and shapes behind (`front` false), or
    /// the balls, flashes and particles in front.
    pub(crate) fn draw_things_part(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        b: &FrameBuffers,
        through: &Through,
        view: &wgpu::TextureView,
        front: bool,
    ) {
        self.fields.splat(encoder, &through.globals, &b.fields, front);
        {
            let label = if front { "things in front" } else { "things" };
            let mut pass = begin_pass(encoder, view, label, wgpu::LoadOp::Load);
            pass.set_bind_group(0, &through.globals, &[]);
            pass.set_bind_group(1, &through.paint, &[]);
            if front {
                self.draw_balls(&mut pass, f, b, through);
                self.draw_front(&mut pass, f, b, through);
            } else {
                self.draw_back(&mut pass, f, b, through);
            }
        }
        self.draw_liquids(encoder, f, front, &through.globals, view);
    }

    /// Draws `f`'s liquids behind the balls (`front` false) or in front of them into `view`,
    /// through `globals`. A Glass liquid first copies the picture behind it (not in the light
    /// passes, where it draws flat).
    pub(crate) fn draw_liquids(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        front: bool,
        globals: &wgpu::BindGroup,
        view: &wgpu::TextureView,
    ) {
        let Some(pass) = self.liquid.as_ref() else { return };
        for l in f.liquids.iter().filter(|l| l.front == front && pass.has(l)) {
            if l.style.glass() && self.light_mode == light_pass::PICTURE {
                self.copy_under(encoder);
            }
            pass.draw(encoder, l, globals, view);
        }
    }

    /// Draws `frame`'s layer `l` (its `index`th): a plain one straight into the HDR picture, a
    /// blended one through [`Renderer::draw_layer`], counting it in `slot`. `objects` is the object
    /// picture for its effects. With a `material` on the scene (only passed for layers drawn
    /// after the main picture's effects), a plain layer goes through the layer picture too, so
    /// the material paints its things.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_any_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        index: usize,
        l: &BlendLayer,
        b: &FrameBuffers,
        world: &Through,
        slot: &mut usize,
        objects: Option<&wgpu::TextureView>,
        material: bool,
    ) {
        if l.plain() && !material {
            let hdr = self.targets.hdr.clone();
            self.draw_things(encoder, &l.frame, b, world, &hdr);
            return;
        }
        let through = self.layer_through(device, queue, frame, l, Some(index), world);
        self.draw_layer(device, queue, encoder, l, b, &through, *slot, Some((frame, index, objects)));
        *slot += 1;
    }

    /// How `l` is drawn: as `through` says, or, when its things blend with each other, through
    /// globals that name its blend (see each.wgsl), with the pipelines it needs made. `index`
    /// is its place in `frame.layers` (none for the overlay's layers, which run no effects).
    ///
    /// This is where a layer's `each` is decided. A Multiply or Darken layer starts as 1 or far
    /// above it, not as nothing, and an effect on one of its things (Things stage) assumes a
    /// layer of colour over nothing: it would paint dark rings round the thing. Such a layer is
    /// drawn as if its things did not blend with each other. Add, Screen, Lighten and Subtract
    /// start as nothing, so they keep blending.
    pub(crate) fn layer_through(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        l: &BlendLayer,
        index: Option<usize>,
        through: &Through,
    ) -> Through {
        // The light passes draw every layer's things one over another.
        if self.light_mode != light_pass::PICTURE {
            return through.clone();
        }
        let starts_above_nothing = matches!(l.blend, bpe_core::Blend::Multiply | bpe_core::Blend::Darken);
        let has_effects = index.is_some_and(|i| effects::chain::layer_has_effects(frame, i));
        if !(l.each && l.blend.combines_with_itself()) || (starts_above_nothing && has_effects) {
            return through.clone();
        }
        let code = l.blend.shader_code();
        let mut globals = Globals::new(frame, &through.camera, self.targets.width);
        globals.post[3] = code as f32;
        let key = (code, !through.world);
        let group = self.each_globals.group(device, queue, &self.globals_layout, key, bytemuck::bytes_of(&globals));
        self.each.ensure(device, l.blend, &l.frame);
        if !l.frame.particles.is_empty() || !l.frame.particles_front.is_empty() {
            self.sprites.ensure_each(device, l.blend);
        }
        if !l.frame.swarms.is_empty() || !l.frame.swarms_front.is_empty() {
            self.swarms.ensure_each(device, l.blend);
        }
        if !l.frame.fields.is_empty() || !l.frame.fields_front.is_empty() {
            self.fields.ensure_each(device, l.blend);
        }
        Through { globals: group, each: Some(l.blend), ..through.clone() }
    }

    /// Draws one blend layer's things into the layer picture, runs the effects on those things
    /// that belong to it (`effects`: the main frame, the layer's index in it and the object
    /// picture; for a material on the scene, the object picture is redrawn with the layer's
    /// own things), then mixes it into the HDR picture over a copy of what is there. `slot`
    /// counts the layers mixed so far this frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        l: &BlendLayer,
        b: &FrameBuffers,
        through: &Through,
        slot: usize,
        effects: Option<(&Frame, usize, Option<&wgpu::TextureView>)>,
    ) {
        // The picture under the layer: what the composite mixes with.
        self.copy_under(encoder);
        let (texture, layer) = self.targets.layer.clone().expect("made before the frame is drawn");
        // A layer whose things blend with each other starts as its blend's identity.
        let clear = through.each.map_or(wgpu::Color::TRANSPARENT, each::clear);
        drop(begin_pass(encoder, &layer, "blend layer", wgpu::LoadOp::Clear(clear)));
        self.draw_things(encoder, &l.frame, b, through, &layer);
        if let Some((frame, index, objects)) = effects.filter(|(frame, index, _)| {
            self.light_mode == light_pass::PICTURE && effects::chain::layer_has_effects(frame, *index)
        }) {
            use effects::chain::Chain;
            // A material paints the layer's own things, so the object picture shows only them.
            let own = self.targets.object.clone().filter(|_| Chain::material(frame) && Chain::wants_objects(frame));
            if let Some((_, view)) = &own {
                let mut pass =
                    begin_pass(encoder, view, "layer objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_bind_group(1, &self.paint.group, &[]);
                self.draw_objects(&mut pass, &l.frame, b);
            }
            let on = effects::chain::Canvas::Layer(index);
            let objects = own.as_ref().map(|(_, v)| v).or(objects);
            self.chain.run(device, queue, encoder, Stage::Things, frame, on, &texture, &layer, objects);
        }
        // The surface keeps the most any layer blocks and stands (see the lighting shader).
        // The emission keeps what is in front hiding what is behind (added light stays added).
        // The tint picture's depths add up (tint.wgsl).
        let blend = match self.light_mode {
            light_pass::SURFACE => bpe_core::Blend::Lighten,
            light_pass::TINT => bpe_core::Blend::Add,
            light_pass::EMISSION | light_pass::UV if l.blend == bpe_core::Blend::Add => bpe_core::Blend::Add,
            light_pass::EMISSION | light_pass::UV | light_pass::TAGS => bpe_core::Blend::Normal,
            _ => l.blend,
        };
        self.blend.composite(
            device,
            queue,
            encoder,
            slot,
            blend,
            l.amount,
            through.each.is_some(),
            l.plain(),
            self.light_mode == light_pass::SURFACE && !self.light_stage,
            &self.targets.behind_view,
            &layer,
            &self.targets.hdr,
        );
    }
}
