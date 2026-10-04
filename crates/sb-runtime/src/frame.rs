//! Frame execution, following Iris' frame order and the model's pass list and static flip
//! schedule: clears → setup (first frame) → begin → shadow (opaque casters, shadowtex1
//! copy, translucent casters) → shadowcomp → prepare → opaque gbuffers (sky, DH LODs,
//! terrain, entities) → centre depth sample, depthtex2 and depthtex1 copies (Iris'
//! `beginHand` / `beginTranslucents`), dhDepthTex1 copy → deferred → translucent gbuffers
//! (water, DH water) → composite → final → end-of-frame copies.

use crate::descriptors::BindContext;
use crate::error::{RuntimeError, VkResultExt};
use crate::executor::{ColorPair, Executor, ProgramState, default_clear};
use crate::math::celestial;
use crate::pipelines::{AttachmentState, NULL_VERTEX_BINDING, Role, VariantKey, dispatch_size, dispatches_over_shadow_map, gl_blend_factor, output_blend, output_class};
use crate::resources::{ImageDesc, ImageId, clear_color_value, full_barrier, host_read_barrier};
use crate::scene::formats::{SODIUM_TERRAIN, VertexLayout};
use crate::scene::sky;
use crate::texel::{self, NumericClass};
use crate::uniforms::{DrawState, FrameInputs, FrameState, fill_host_block, fill_pack_block, push_constant_bytes, stage};
use ash::vk;
use sb_core::model::{Pass, Program, ProgramKind};
use sb_core::program::{GeometryGroup, GeometryProgram};
use sb_core::PassGroup;
use std::collections::HashMap;

/// Largest host copy of the `sb_Frame` layout (std140 blocks of real packs are a few KiB;
/// uniform buffer ranges are limited to 64 KiB on most devices).
const MAX_HOST_BLOCK: u64 = 1 << 20;

/// Where a batch's geometry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeshRef {
    Solid,
    Cutout,
    Water,
    /// Sub-draw of the entity mesh.
    Entity(usize),
    /// Sub-draw of the sky mesh ([`sky::SKY_HORIZON`] or [`sky::SKY_DISC`]).
    Sky(usize),
    Sun,
    /// Moon phase sub-draw.
    Moon(usize),
    DhOpaque(usize),
    DhWater(usize),
    /// A terrain layer (0 solid, 1 cutout, 2 translucent) in Sodium's format.
    Sodium(usize),
}

impl MeshRef {
    /// The terrain layer of a vanilla terrain mesh.
    fn terrain_layer(self) -> Option<usize> {
        match self {
            MeshRef::Solid => Some(0),
            MeshRef::Cutout => Some(1),
            MeshRef::Water => Some(2),
            _ => None,
        }
    }
}

/// The vertex layout, buffers and sub-draws of a mesh; Sodium sub-draws carry their
/// region (origin in blocks, `u_RegionID`).
struct MeshDraws {
    layout: &'static VertexLayout,
    vertex: vk::Buffer,
    index: vk::Buffer,
    draws: Vec<(crate::scene::SubDraw, Option<crate::scene::terrain::SodiumRegion>)>,
}

/// One batch of draws with one program and one draw state.
#[derive(Debug, Clone)]
struct Batch {
    geometry: GeometryProgram,
    what: &'static str,
    mesh: MeshRef,
    state: DrawState,
    albedo: ImageId,
    depth_write: bool,
    cull_back: bool,
    shadow: bool,
}

/// Rendering scope: colour attachments (`None` = unused slot) and depth.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Scope {
    colors: Vec<Option<ImageId>>,
    depth: Option<ImageId>,
}

/// Vertices of a composite-style (`fullscreen` profile) draw: Iris' [0,1]^2 quad as two
/// triangles, generated from `gl_VertexIndex` without vertex buffers.
pub(crate) const FULLSCREEN_VERTICES: u32 = 6;

impl Executor<'_> {
    /// Render `frames` frames.
    pub(crate) fn run(&mut self, frames: u32) -> Result<(), RuntimeError> {
        let cmd = self.gpu.allocate_command_buffer()?;
        self.arena.command_buffers.push(cmd);
        for frame in 0..frames.max(1) {
            let fs = self.frame_state(frame);
            let frame_block = self.frame_block(&fs);
            let mut attempts = 0;
            loop {
                self.record_frame(cmd, frame, &fs, &frame_block)?;
                if !self.ring.overflow {
                    break;
                }
                attempts += 1;
                if attempts > 6 {
                    return Err(RuntimeError::Unsupported("uniform data of one frame exceeds 512 MiB".into()));
                }
                self.grow_ring()?;
            }
            self.gpu.submit_and_wait(cmd)?;
            self.stats.frames = frame + 1;
            if let Some(m) = self.arena.buffer(self.center_buffer).mapped_ref() {
                let d = f32::from_le_bytes([m[0], m[1], m[2], m[3]]);
                self.center_depth = self.depth.gl_depth(d);
            }
        }
        Ok(())
    }

    fn grow_ring(&mut self) -> Result<(), RuntimeError> {
        let capacity = self.ring.capacity * 2;
        let buffer = self.arena.create_buffer(self.gpu, "uniform ring", capacity, vk::BufferUsageFlags::UNIFORM_BUFFER, gpu_allocator::MemoryLocation::CpuToGpu)?;
        self.ring.buffer = buffer;
        self.ring.capacity = capacity;
        Ok(())
    }

    fn frame_state(&self, frame: u32) -> FrameState {
        let ext = self.targets.extent;
        FrameState::new(&FrameInputs {
            scene: &self.req.scene,
            camera: self.scene.camera,
            width: ext.width,
            height: ext.height,
            frame,
            settings: &self.dim.settings,
            shadow: &self.dim.targets.shadow,
            dh: self.dh_enabled,
            unified_projection: self.unified,
            center_depth: self.center_depth,
        })
    }

    /// `sb_Frame` contents: builtins, defaults and custom uniforms, cut to what the
    /// programs declare (the largest `sb_Frame` range of any program).
    ///
    /// The host copy spans every member of the layout (custom uniforms may read builtins
    /// that no program declares), bounded by [`MAX_HOST_BLOCK`] so that a corrupt
    /// `size` or member offset cannot exhaust memory.
    fn frame_block(&mut self, fs: &FrameState) -> Vec<u8> {
        let layout = &self.dim.uniforms.frame;
        let mut upload = 16usize;
        for p in &self.programs {
            if let ProgramState::Ready(p) = p {
                for s in p.slots.iter().filter(|s| s.role == Role::Frame) {
                    upload = upload.max(s.range as usize);
                }
            }
        }
        let members = layout.members.iter().map(|m| u64::from(m.offset) + u64::from(m.ty.std140_size())).max().unwrap_or(0);
        let host = members.max(u64::from(layout.size)).min(MAX_HOST_BLOCK) as usize;
        let mut bytes = vec![0u8; host.max(upload)];
        fill_pack_block(layout, &mut bytes, fs, &DrawState::new(fs));
        self.custom.evaluate_into_block(layout, &mut bytes, fs.frame_time);
        bytes.truncate(upload);
        bytes
    }

    fn record_frame(&mut self, cmd: vk::CommandBuffer, frame: u32, fs: &FrameState, frame_block: &[u8]) -> Result<(), RuntimeError> {
        unsafe {
            self.gpu.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty()).vk("vkResetCommandBuffer")?;
            self.gpu.device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).vk("vkBeginCommandBuffer")?;
        }
        self.ring.head = 0;
        self.ring.overflow = false;
        self.reset_pools();
        let frame_offset = self.ring.push(&mut self.arena, frame_block, frame_block.len() as u32);
        self.stats.passes_run = 0;
        self.stats.draws = 0;
        self.stats.dispatches = 0;
        self.flips.reset();

        self.clear_targets(cmd, frame, fs);
        let dim = self.dim;
        let geometry_groups = [PassGroup::Shadow, PassGroup::GbuffersOpaque, PassGroup::GbuffersTranslucent];
        let mut done = [false; 3];
        let mut final_done = false;
        let mut prev_group = None;
        for pass in &dim.passes {
            for (k, g) in geometry_groups.iter().enumerate() {
                if !done[k] && *g < pass.group {
                    self.geometry_pass(cmd, *g, None, fs, frame_offset)?;
                    done[k] = true;
                }
            }
            self.adopt_flip_state(pass, prev_group != Some(pass.group));
            prev_group = Some(pass.group);
            match pass.group {
                PassGroup::Setup => {
                    if frame == 0 {
                        self.run_computes(cmd, pass, fs, frame_offset)?;
                        self.stats.passes_run += 1;
                    }
                }
                g @ (PassGroup::Shadow | PassGroup::GbuffersOpaque | PassGroup::GbuffersTranslucent) => {
                    let k = geometry_groups.iter().position(|x| *x == g).unwrap_or(0);
                    if done[k] {
                        // A second pass of the same geometry group: only its computes.
                        self.run_computes(cmd, pass, fs, frame_offset)?;
                    } else {
                        self.geometry_pass(cmd, g, Some(pass), fs, frame_offset)?;
                        done[k] = true;
                    }
                }
                group => {
                    self.run_computes(cmd, pass, fs, frame_offset)?;
                    if let Some(p) = pass.program {
                        let ran = self.fullscreen(cmd, p, group, fs, frame_offset)?;
                        if group == PassGroup::Final && ran {
                            final_done = true;
                        }
                    }
                    self.stats.passes_run += 1;
                }
            }
            if pass.group != PassGroup::ShadowComp {
                self.flips.flip(&pass.flips_after);
            }
        }
        for (k, g) in geometry_groups.iter().enumerate() {
            if !done[k] {
                self.geometry_pass(cmd, *g, None, fs, frame_offset)?;
            }
        }
        if !final_done {
            self.copy_to_output(cmd);
        }
        self.end_of_frame(cmd);
        unsafe { self.gpu.device.end_command_buffer(cmd) }.vk("vkEndCommandBuffer")?;
        Ok(())
    }

    /// Use the model's flip state for the pass (it is authoritative; a mismatch with
    /// the runtime's own tracking is a model inconsistency). The first pass of a group may
    /// differ legitimately: Iris applies the group's `flip.<group>_pre.<buf>` flips before
    /// it, which the model folds into `flip_state` only.
    fn adopt_flip_state(&mut self, pass: &Pass, first_of_group: bool) {
        if pass.flip_state.is_empty() {
            return;
        }
        let color = &self.targets.color;
        let mismatches = self.flips.adopt(&pass.flip_state, |i| color.contains_key(&i));
        if !mismatches.is_empty() && !first_of_group {
            self.warn(format!("pass {:?}{}: flip_state disagrees with the flips of the previous passes; using flip_state", pass.group, pass.index));
        }
    }

    fn clear_targets(&mut self, cmd: vk::CommandBuffer, frame: u32, fs: &FrameState) {
        let d = &self.gpu.device;
        full_barrier(d, cmd);
        let clear_pair = |arena: &crate::resources::Arena, index: u32, p: &ColorPair, shadow: bool| {
            if !(p.clear || frame == 0) {
                return;
            }
            let c = p.clear_color.unwrap_or_else(|| default_clear(index, shadow, fs.fog_color));
            for img in p.images {
                let i = arena.image(img);
                unsafe { d.cmd_clear_color_image(cmd, i.image, vk::ImageLayout::GENERAL, &clear_color_value(p.format, c), &[i.full_range()]) };
            }
        };
        for (i, p) in &self.targets.color {
            clear_pair(&self.arena, *i, p, false);
        }
        for (i, p) in &self.targets.shadow_color {
            clear_pair(&self.arena, *i, p, true);
        }
        let depth_clear = vk::ClearDepthStencilValue { depth: self.depth.clear, stencil: 0 };
        for img in self.targets.depth.iter().chain(&self.targets.dh_depth).chain(&self.targets.shadow) {
            let i = self.arena.image(*img);
            unsafe { d.cmd_clear_depth_stencil_image(cmd, i.image, vk::ImageLayout::GENERAL, &depth_clear, &[i.full_range()]) };
        }
        full_barrier(d, cmd);
    }

    fn run_computes(&mut self, cmd: vk::CommandBuffer, pass: &Pass, fs: &FrameState, frame_offset: u32) -> Result<(), RuntimeError> {
        for &c in &pass.computes {
            self.dispatch(cmd, c, fs, frame_offset)?;
        }
        Ok(())
    }

    fn dispatch(&mut self, cmd: vk::CommandBuffer, index: u32, fs: &FrameState, frame_offset: u32) -> Result<(), RuntimeError> {
        let dim = self.dim;
        let Some(model) = dim.programs.get(index as usize) else {
            self.warn(format!("pass references compute program {index}, which does not exist"));
            return Ok(());
        };
        let compute = match self.program(index) {
            Some(p) if p.compute => true,
            Some(_) => false,
            None => return Ok(()),
        };
        if !compute {
            self.stats.skip_program(&model.name, "listed as a compute but it is a graphics program");
            return Ok(());
        }
        let pipeline = match &mut self.programs[index as usize] {
            ProgramState::Ready(p) => p.compute_pipeline(self.gpu, &mut self.arena),
            ProgramState::Skipped => return Ok(()),
        };
        let pipeline = match pipeline {
            Ok(p) => p,
            Err(e) => {
                self.stats.skip_program(&model.name, &e.to_string());
                self.programs[index as usize] = ProgramState::Skipped;
                return Ok(());
            }
        };
        let geometry = matches!(model.kind, ProgramKind::GeometryCompute { .. } | ProgramKind::Geometry { .. });
        let ctx = BindContext { geometry, albedo: self.tex.atlas, mip_targets: model.mipmap_targets.clone(), attachments: Vec::new() };
        let sets = self.write_sets(index, &ctx)?;
        let offsets = self.dynamic_offsets(index, &DrawState::new(fs), fs, frame_offset);
        let Some(p) = self.program(index) else { return Ok(()) };
        let (layout, push, local) = (p.layout, p.push_constants, p.local_size);
        let push_bytes = push.map(|(_, size)| push_constant_bytes(&p.push_members, size, fs, &DrawState::new(fs))).unwrap_or_default();
        // Iris dispatches `shadowcomp` and the shadow pass's computes (`shadow.csh`,
        // `shadow_a.csh`, ...) over the shadow map resolution; every other compute over
        // the screen.
        let ext = if dispatches_over_shadow_map(&model.kind) { self.arena.image(self.targets.shadow[0]).extent_2d() } else { self.targets.extent };
        let lim = self.gpu.limits().max_compute_work_group_count;
        let size = dispatch_size(model.compute.as_ref().map(|c| &c.work_groups), local, ext.width, ext.height, lim);
        let indirect = model.compute.as_ref().and_then(|c| c.indirect);
        let indirect_buf = match indirect {
            Some((ssbo, offset)) => match self.tex.ssbos.get(&ssbo) {
                Some(b) if u64::from(offset) + 12 <= self.arena.buffer(*b).size && offset % 4 == 0 => Some((self.arena.buffer(*b).buffer, offset)),
                _ => {
                    self.warn(format!("{}: indirect dispatch buffer {ssbo} (offset {offset}) is missing or too small; dispatch skipped", model.name));
                    return Ok(());
                }
            },
            None => None,
        };
        if indirect_buf.is_none() && size.contains(&0) {
            return Ok(());
        }
        let d = &self.gpu.device;
        full_barrier(d, cmd);
        unsafe {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline);
            if !sets.is_empty() {
                d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, layout, 0, &sets, &offsets);
            }
            if let Some((stages, _)) = push {
                d.cmd_push_constants(cmd, layout, stages, 0, &push_bytes);
            }
            match indirect_buf {
                Some((b, off)) => d.cmd_dispatch_indirect(cmd, b, u64::from(off)),
                None => d.cmd_dispatch(cmd, size[0], size[1], size[2]),
            }
        }
        full_barrier(d, cmd);
        self.stats.dispatches += 1;
        Ok(())
    }

    /// Dynamic offsets of a program's uniform blocks for one draw state.
    fn dynamic_offsets(&mut self, index: u32, state: &DrawState, fs: &FrameState, frame_offset: u32) -> Vec<u32> {
        let Some(p) = self.program(index) else { return Vec::new() };
        let slots: Vec<_> = p.slots.iter().filter(|s| s.is_dynamic_ubo()).cloned().collect();
        let mut out = Vec::new();
        for s in slots {
            let offset = match &s.role {
                Role::Frame => frame_offset,
                Role::Draw => {
                    // Only the declared range is read (members beyond it are skipped).
                    let mut bytes = vec![0u8; s.range as usize];
                    fill_pack_block(&self.dim.uniforms.draw, &mut bytes, fs, state);
                    self.ring.push(&mut self.arena, &bytes, s.range)
                }
                Role::HostBlock(name) => {
                    let mut bytes = vec![0u8; s.range as usize];
                    if let Some(members) = s.kind.members() {
                        fill_host_block(name, members, &mut bytes, fs, state);
                    }
                    self.ring.push(&mut self.arena, &bytes, s.range)
                }
                Role::PackUbo | Role::Resource(_) => self.ring.push(&mut self.arena, &[], s.range),
            };
            for _ in 0..s.count {
                out.push(offset);
            }
        }
        out
    }

    // --------------------------------------------------------------------------------
    // Geometry passes
    // --------------------------------------------------------------------------------

    fn geometry_pass(&mut self, cmd: vk::CommandBuffer, group: PassGroup, pass: Option<&Pass>, fs: &FrameState, frame_offset: u32) -> Result<(), RuntimeError> {
        if let Some(p) = pass {
            self.run_computes(cmd, p, fs, frame_offset)?;
        }
        self.stats.passes_run += 1;
        match group {
            PassGroup::Shadow => {
                if !self.shadow_enabled {
                    return Ok(());
                }
                let sh = &self.dim.targets.shadow;
                let mut batches = Vec::new();
                if sh.render_terrain {
                    self.terrain_batches(&mut batches, fs, true, false);
                }
                if sh.render_entities {
                    self.entity_batches(&mut batches, fs, true);
                }
                if self.dh_enabled && self.dim.distant_horizons.shadow_enabled && sh.dh_shadow_enabled {
                    self.dh_batches(&mut batches, fs, true, false);
                }
                self.draw_batches(cmd, &batches, fs, frame_offset)?;
                self.copy_image(cmd, self.targets.shadow[0], self.targets.shadow[1]);
                if sh.render_translucent {
                    let mut t = Vec::new();
                    self.terrain_batches(&mut t, fs, true, true);
                    self.draw_batches(cmd, &t, fs, frame_offset)?;
                }
                let mipped: Vec<(u32, ImageId)> = self.targets.shadow_color.iter().filter(|(_, p)| p.mipped).map(|(i, p)| (*i, p.images[self.flips.shadow_read(*i)])).collect();
                for (_, img) in mipped {
                    self.generate_mips(cmd, img);
                }
            }
            PassGroup::GbuffersOpaque => {
                let mut batches = Vec::new();
                self.sky_batches(&mut batches, fs);
                if self.dh_enabled {
                    self.dh_batches(&mut batches, fs, false, false);
                }
                self.terrain_batches(&mut batches, fs, false, false);
                self.entity_batches(&mut batches, fs, false);
                self.draw_batches(cmd, &batches, fs, frame_offset)?;
                // Iris `beginHand`: the centre depth sample and depthtex2 (no translucents,
                // no hand), then `beginTranslucents`: depthtex1 (no translucents) before
                // `deferred`. The scene has no hand, so both copies hold the same depth.
                self.sample_center_depth(cmd);
                self.copy_image(cmd, self.targets.depth[0], self.targets.depth[2]);
                self.copy_image(cmd, self.targets.depth[0], self.targets.depth[1]);
                // DH: dhDepthTex1 is the LOD depth before dh_water.
                self.copy_image(cmd, self.targets.dh_depth[0], self.targets.dh_depth[1]);
            }
            _ => {
                let mut batches = Vec::new();
                self.terrain_batches(&mut batches, fs, false, true);
                if self.dh_enabled {
                    self.dh_batches(&mut batches, fs, false, true);
                }
                self.draw_batches(cmd, &batches, fs, frame_offset)?;
            }
        }
        Ok(())
    }

    /// Copy the depth at the screen centre (GL convention after readback) for
    /// `centerDepthSmooth` of the next frame, as Iris' `CenterDepthSampler` does at
    /// `beginHand` (after opaque geometry, before translucents).
    fn sample_center_depth(&self, cmd: vk::CommandBuffer) {
        let img = self.arena.image(self.targets.depth[0]);
        let ext = img.extent_2d();
        let region = vk::BufferImageCopy {
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::DEPTH, mip_level: 0, base_array_layer: 0, layer_count: 1 },
            image_offset: vk::Offset3D { x: (ext.width / 2) as i32, y: (ext.height / 2) as i32, z: 0 },
            image_extent: vk::Extent3D { width: 1, height: 1, depth: 1 },
        };
        let (image, buffer) = (img.image, self.arena.buffer(self.center_buffer).buffer);
        let d = &self.gpu.device;
        full_barrier(d, cmd);
        unsafe { d.cmd_copy_image_to_buffer(cmd, image, vk::ImageLayout::GENERAL, buffer, &[region]) };
        full_barrier(d, cmd);
        host_read_barrier(d, cmd);
    }

    fn base_state(&self, fs: &FrameState, shadow: bool) -> DrawState {
        let s = DrawState::new(fs);
        if shadow { s.for_shadow(fs) } else { s }
    }

    /// The sky as Iris draws it: the horizon cone (fog colour) and the upper disc (sky
    /// colour) with `gbuffers_skybasic` unless `sky=false`, then the sun and moon with
    /// `gbuffers_skytextured` unless `sun=false` / `moon=false`.
    fn sky_batches(&self, out: &mut Vec<Batch>, fs: &FrameState) {
        let settings = &self.dim.settings;
        if settings.sky {
            let mut horizon = self.base_state(fs, false);
            // Iris: `renderHorizon(..., fogColor)` with alpha 1.
            horizon.color_modulator = [fs.fog_color[0], fs.fog_color[1], fs.fog_color[2], 1.0];
            horizon.render_stage = stage::SKY;
            horizon.texture_size = [1, 1];
            let mut disc = horizon.clone();
            disc.color_modulator = [fs.sky_color[0], fs.sky_color[1], fs.sky_color[2], 1.0];
            let sky = |what, sub, state| Batch { geometry: GeometryProgram::SkyBasic, what, mesh: MeshRef::Sky(sub), state, albedo: self.tex.white, depth_write: false, cull_back: false, shadow: false };
            out.push(sky("horizon", sky::SKY_HORIZON, horizon));
            out.push(sky("sky", sky::SKY_DISC, disc));
        }
        let celestial = celestial::sky_model_view(&fs.model_view, fs.sky_angle, fs.sun_path_rotation);
        let mut sun = self.base_state(fs, false);
        sun.model_view = celestial;
        sun.color_modulator = [1.0, 1.0, 1.0, 1.0 - fs.rain];
        sun.render_stage = stage::SUN;
        sun.texture_size = [32, 32];
        if settings.sun {
            out.push(Batch { geometry: GeometryProgram::SkyTextured, what: "sun", mesh: MeshRef::Sun, state: sun.clone(), albedo: self.tex.sun, depth_write: false, cull_back: false, shadow: false });
        }
        if !settings.moon {
            return;
        }
        let mut moon = sun;
        moon.render_stage = stage::MOON;
        moon.texture_size = [64, 32];
        out.push(Batch {
            geometry: GeometryProgram::SkyTextured,
            what: "moon",
            mesh: MeshRef::Moon(fs.moon_phase.rem_euclid(8) as usize),
            state: moon,
            albedo: self.tex.moon,
            depth_write: false,
            cull_back: false,
            shadow: false,
        });
    }

    /// Whether a terrain layer renders its back faces: `backFace.<layer>` (OptiFine; the
    /// cutout layer also honours `backFace.cutoutMipped`, merged into cutout since
    /// Minecraft 1.21.5). Minecraft culls every terrain layer by default; Iris 26.3 parses
    /// these keys but ignores them.
    fn back_faces(&self, keys: &[&str]) -> bool {
        keys.iter().any(|k| self.dim.settings.back_face.get(*k).copied().unwrap_or(false))
    }

    fn terrain_batches(&self, out: &mut Vec<Batch>, fs: &FrameState, shadow: bool, translucent: bool) {
        let mut s = self.base_state(fs, shadow);
        s.texture_size = [64, 64];
        let mk = |g, what, mesh, st: i32, back_faces: bool| {
            let mut state = s.clone();
            state.render_stage = st;
            Batch { geometry: g, what, mesh, state, albedo: self.tex.atlas, depth_write: true, cull_back: !back_faces, shadow }
        };
        if translucent {
            if self.scene.water.is_some() {
                let g = if shadow { GeometryProgram::ShadowWater } else { GeometryProgram::Water };
                out.push(mk(g, "water", MeshRef::Water, stage::TERRAIN_TRANSLUCENT, self.back_faces(&["translucent"])));
            }
            return;
        }
        if self.scene.solid.is_some() {
            let g = if shadow { GeometryProgram::ShadowSolid } else { GeometryProgram::TerrainSolid };
            out.push(mk(g, "terrain", MeshRef::Solid, stage::TERRAIN_SOLID, self.back_faces(&["solid"])));
        }
        if self.scene.cutout.is_some() {
            let g = if shadow { GeometryProgram::ShadowCutout } else { GeometryProgram::TerrainCutout };
            out.push(mk(g, "leaves", MeshRef::Cutout, stage::TERRAIN_CUTOUT, self.back_faces(&["cutout", "cutoutMipped"])));
        }
    }

    fn entity_batches(&self, out: &mut Vec<Batch>, fs: &FrameState, shadow: bool) {
        if self.scene.entities.is_none() {
            return;
        }
        for e in &self.scene.entity_list {
            let mut state = self.base_state(fs, shadow);
            state.model_offset = [(e.position[0] - fs.camera[0]) as f32, (e.position[1] - fs.camera[1]) as f32, (e.position[2] - fs.camera[2]) as f32];
            state.entity_id = e.entity_id;
            state.texture_size = [64, 32];
            state.render_stage = stage::ENTITIES;
            let g = if shadow { GeometryProgram::ShadowEntities } else { GeometryProgram::Entities };
            out.push(Batch { geometry: g, what: "entities", mesh: MeshRef::Entity(e.draw), state, albedo: self.tex.entity, depth_write: true, cull_back: true, shadow });
        }
    }

    fn dh_batches(&self, out: &mut Vec<Batch>, fs: &FrameState, shadow: bool, water: bool) {
        for (i, (origin, opaque, wat)) in self.scene.dh.iter().enumerate() {
            let present = if water { wat.is_some() } else { opaque.is_some() };
            if !present {
                continue;
            }
            let mut state = self.base_state(fs, shadow);
            if !shadow {
                state.projection = fs.dh_projection;
            }
            state.dh_model_offset = origin.map(|v| v as f32);
            state.texture_size = [256, 16];
            state.render_stage = if water { stage::TERRAIN_TRANSLUCENT } else { stage::TERRAIN_SOLID };
            let (g, mesh, what) = match (shadow, water) {
                (true, _) => (GeometryProgram::DhShadow, MeshRef::DhOpaque(i), "DH LODs"),
                (false, false) => (GeometryProgram::DhTerrain, MeshRef::DhOpaque(i), "DH LODs"),
                (false, true) => (GeometryProgram::DhWater, MeshRef::DhWater(i), "DH water"),
            };
            out.push(Batch { geometry: g, what, mesh, state, albedo: self.tex.white, depth_write: true, cull_back: true, shadow });
        }
    }

    /// Attachment targets of a world-geometry program: the shared list, or its own
    /// draw buffers.
    fn attachment_targets<'p>(&self, program: &'p Program, shadow: bool) -> (Vec<u32>, bool, &'p [u32]) {
        let shared = if shadow { &self.dim.shadow_attachments } else { &self.dim.gbuffer_attachments };
        if shared.is_empty() { (program.draw_buffers.clone(), false, &program.draw_buffers) } else { (shared.clone(), true, &program.draw_buffers) }
    }

    /// A write-only attachment standing in for a missing target at attachment `slot`
    /// (one per format and slot, so one scope never binds the same image twice).
    fn sink(&mut self, format: vk::Format, slot: usize, extent: vk::Extent2D) -> Result<ImageId, RuntimeError> {
        let key = (format, slot);
        if let Some(id) = self.targets.sinks.get(&key) {
            let e = self.arena.image(*id).extent_2d();
            if e.width >= extent.width && e.height >= extent.height {
                return Ok(*id);
            }
        }
        let w = extent.width.max(self.targets.extent.width);
        let h = extent.height.max(self.targets.extent.height);
        let id = self.arena.create_image(self.gpu, ImageDesc::new_2d(format!("sink {format:?}"), format, w, h, vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST))?;
        self.targets.sinks.insert(key, id);
        Ok(id)
    }

    fn sink_format(class: &str) -> vk::Format {
        match class {
            "int" => vk::Format::R32G32B32A32_SINT,
            "uint" => vk::Format::R32G32B32A32_UINT,
            _ => vk::Format::R16G16B16A16_SFLOAT,
        }
    }

    /// The image to bind at attachment `location` (currently `img`) for `program`, and
    /// whether the program writes it:
    /// * no fragment output at the location: bound, not written (Vulkan would write
    ///   undefined values; a GL pack that lists a buffer it never writes keeps its
    ///   contents);
    /// * an output of the attachment's numeric class: written;
    /// * an output of another class (a float output into an integer target is undefined
    ///   in GL and Vulkan alike, even with writes masked): a sink of the output's class is
    ///   bound instead and the mismatch is reported.
    fn output_attachment(&mut self, program: &Program, outputs: &HashMap<u32, String>, location: usize, img: ImageId, extent: vk::Extent2D) -> Result<(ImageId, bool), RuntimeError> {
        let Some(class) = outputs.get(&(location as u32)) else { return Ok((img, false)) };
        let format = self.arena.image(img).desc.format;
        if texel::numeric_class(format) == output_class(class) {
            return Ok((img, true));
        }
        let name = self.arena.image(img).desc.name.clone();
        self.warn(format!("{}: output location {location} is {class} but its target {name} is {format:?}; the output is discarded", program.name));
        Ok((self.sink(Self::sink_format(class), location, extent)?, false))
    }

    fn draw_batches(&mut self, cmd: vk::CommandBuffer, batches: &[Batch], fs: &FrameState, frame_offset: u32) -> Result<(), RuntimeError> {
        let dim = self.dim;
        // Resolve programs and scopes.
        let mut planned: Vec<(Batch, u32, Scope, Vec<AttachmentState>)> = Vec::new();
        for b in batches {
            let Some(slot) = dim.geometry.get(&b.geometry) else {
                self.stats.skip_geometry(format!("{} ({}): no program", b.geometry.file_name(), b.what));
                continue;
            };
            let pi = slot.program;
            let Some(program) = dim.programs.get(pi as usize) else {
                self.warn(format!("geometry {} points at program {pi}, which does not exist", b.geometry.file_name()));
                continue;
            };
            let Some(prepared) = self.program(pi) else { continue };
            if prepared.compute {
                self.stats.skip_program(&program.name, "a compute program is used for geometry");
                continue;
            }
            let outputs = prepared.output_classes();
            // Terrain for programs translated for Sodium's mesh format comes from the
            // Sodium buffers.
            let mut mesh = b.mesh;
            if program.draw_profile.as_deref() == Some(SODIUM_TERRAIN.profile)
                && let Some(layer) = mesh.terrain_layer()
                && self.scene.sodium.is_some()
            {
                mesh = MeshRef::Sodium(layer);
            }
            if let (Some(profile), Some(m)) = (program.draw_profile.as_deref(), self.mesh(mesh))
                && crate::scene::formats::layout_profile(profile) != m.layout.profile
            {
                self.warn(format!(
                    "{}: translated for the `{profile}` draw profile but draws {} in the `{}` vertex layout; inputs are matched by name",
                    program.name, b.what, m.layout.profile
                ));
            }
            let (targets, shared, draw_buffers) = self.attachment_targets(program, b.shadow);
            let max_attachments = self.gpu.limits().max_color_attachments as usize;
            if targets.len() > max_attachments {
                self.stats.skip_program(&program.name, &format!("{} colour attachments exceed the device limit of {max_attachments}", targets.len()));
                continue;
            }
            let ext = if b.shadow { self.arena.image(self.targets.shadow[0]).extent_2d() } else { self.targets.extent };
            let mut colors = Vec::with_capacity(targets.len());
            for (slot_index, &t) in targets.iter().enumerate() {
                let pair = if b.shadow { self.targets.shadow_color.get(&t) } else { self.targets.color.get(&t) };
                let img = match pair {
                    Some(p) => p.images[if b.shadow { self.flips.shadow_read(t) } else { self.flips.read(t) }],
                    None => {
                        let class = outputs.get(&(slot_index as u32)).map_or("float", String::as_str);
                        self.warn(format!("{}: target {t} has no image; its output is discarded", program.name));
                        self.sink(Self::sink_format(class), slot_index, ext)?
                    }
                };
                colors.push(img);
            }
            // Which slots the program's draw buffers write, with their blend.
            let mut states = vec![AttachmentState { write: false, blend: None }; targets.len()];
            let mut mapped = vec![false; targets.len()];
            for (i, &t) in draw_buffers.iter().enumerate() {
                let loc = if shared {
                    match program.output_slots.get(i) {
                        Some(&s) => s as usize,
                        None => targets.iter().position(|x| *x == t).unwrap_or(usize::MAX),
                    }
                } else {
                    i
                };
                if shared && targets.get(loc) != Some(&t) {
                    self.warn(format!("{}: output {i} (target {t}) maps to attachment slot {loc}, which holds another target", program.name));
                }
                if let (Some(st), Some(m)) = (states.get_mut(loc), mapped.get_mut(loc)) {
                    st.blend = output_blend(program, t);
                    *m = true;
                }
            }
            // Every slot is checked against the shader's outputs, mapped or not: an output
            // whose class differs from the slot's format is undefined even when masked.
            for (loc, img) in colors.iter_mut().enumerate() {
                let (bound, write) = self.output_attachment(program, &outputs, loc, *img, ext)?;
                *img = bound;
                states[loc].write = write && mapped[loc];
            }
            let colors: Vec<Option<ImageId>> = colors.into_iter().map(Some).collect();
            let dh = b.geometry.group() == GeometryGroup::DistantHorizons;
            let depth = if b.shadow {
                self.targets.shadow[0]
            } else if dh && !self.unified {
                self.targets.dh_depth[0]
            } else {
                self.targets.depth[0]
            };
            let mut batch = b.clone();
            batch.mesh = mesh;
            batch.state.alpha_test_ref = program.alpha_test.map_or(0.0, |a| a.reference);
            batch.state.blend_func = match program.blend {
                Some(m) => [gl_blend_factor(m.src_color), gl_blend_factor(m.dst_color), gl_blend_factor(m.src_alpha), gl_blend_factor(m.dst_alpha)],
                None => [0; 4],
            };
            planned.push((batch, pi, Scope { colors, depth: Some(depth) }, states));
        }
        // Group consecutive batches with the same scope.
        let mut start = 0;
        while start < planned.len() {
            let mut end = start + 1;
            while end < planned.len() && planned[end].2 == planned[start].2 {
                end += 1;
            }
            self.record_scope(cmd, &planned[start..end], fs, frame_offset)?;
            start = end;
        }
        Ok(())
    }

    fn scope_attachments(scope: &Scope) -> Vec<ImageId> {
        scope.colors.iter().flatten().copied().chain(scope.depth).collect()
    }

    fn render_extent(&self, scope: &Scope) -> vk::Extent2D {
        let mut e = vk::Extent2D { width: u32::MAX, height: u32::MAX };
        for img in Self::scope_attachments(scope) {
            let x = self.arena.image(img).extent_2d();
            e.width = e.width.min(x.width);
            e.height = e.height.min(x.height);
        }
        if e.width == u32::MAX { self.targets.extent } else { e }
    }

    fn begin_scope(&mut self, cmd: vk::CommandBuffer, scope: &Scope, extent: vk::Extent2D) -> Result<(), RuntimeError> {
        let mut color_infos = Vec::new();
        for c in &scope.colors {
            let view = match c {
                Some(img) => self.arena.default_view(self.gpu, *img, false)?,
                None => vk::ImageView::null(),
            };
            color_infos.push(vk::RenderingAttachmentInfo::default().image_view(view).image_layout(vk::ImageLayout::GENERAL).load_op(vk::AttachmentLoadOp::LOAD).store_op(vk::AttachmentStoreOp::STORE));
        }
        let depth_info = match scope.depth {
            Some(img) => Some(
                vk::RenderingAttachmentInfo::default()
                    .image_view(self.arena.default_view(self.gpu, img, false)?)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE),
            ),
            None => None,
        };
        let area = vk::Rect2D { offset: vk::Offset2D::default(), extent };
        let mut info = vk::RenderingInfo::default().render_area(area).layer_count(1).color_attachments(&color_infos);
        if let Some(d) = depth_info.as_ref() {
            info = info.depth_attachment(d);
        }
        self.gpu.cmd_begin_rendering(cmd, &info);
        Ok(())
    }

    fn scope_formats(&self, scope: &Scope) -> (Vec<vk::Format>, vk::Format) {
        let colors = scope.colors.iter().map(|c| c.map_or(vk::Format::UNDEFINED, |i| self.arena.image(i).desc.format)).collect();
        let depth = scope.depth.map_or(vk::Format::UNDEFINED, |i| self.arena.image(i).desc.format);
        (colors, depth)
    }

    /// Viewport covering `viewport` with the scissor (and render area) `area`.
    fn set_viewport_clipped(&self, cmd: vk::CommandBuffer, viewport: vk::Extent2D, area: vk::Extent2D) {
        let vp = [vk::Viewport { x: 0.0, y: 0.0, width: viewport.width.max(1) as f32, height: viewport.height.max(1) as f32, min_depth: 0.0, max_depth: 1.0 }];
        let sc = [vk::Rect2D { offset: vk::Offset2D::default(), extent: area }];
        unsafe {
            self.gpu.device.cmd_set_viewport(cmd, 0, &vp);
            self.gpu.device.cmd_set_scissor(cmd, 0, &sc);
        }
    }

    fn set_viewport(&self, cmd: vk::CommandBuffer, extent: vk::Extent2D, scale: Option<&sb_core::model::ViewportScale>) {
        let (w, h) = (extent.width as f32, extent.height as f32);
        let (mut x, mut y, mut vw, mut vh) = (0.0, 0.0, w, h);
        if let Some(s) = scale {
            let ok = |v: f32| v.is_finite();
            if ok(s.scale) && s.scale > 0.0 && ok(s.offset_x) && ok(s.offset_y) {
                x = s.offset_x * w;
                y = s.offset_y * h;
                vw = (s.scale * w).max(1.0);
                vh = (s.scale * h).max(1.0);
            }
        }
        let vp = [vk::Viewport { x, y, width: vw, height: vh, min_depth: 0.0, max_depth: 1.0 }];
        let sc = [vk::Rect2D { offset: vk::Offset2D::default(), extent }];
        unsafe {
            self.gpu.device.cmd_set_viewport(cmd, 0, &vp);
            self.gpu.device.cmd_set_scissor(cmd, 0, &sc);
        }
    }

    fn record_scope(&mut self, cmd: vk::CommandBuffer, items: &[(Batch, u32, Scope, Vec<AttachmentState>)], fs: &FrameState, frame_offset: u32) -> Result<(), RuntimeError> {
        let Some(first) = items.first() else { return Ok(()) };
        let scope = first.2.clone();
        let attachments = Self::scope_attachments(&scope);
        // Feedback loops: snapshot attachments that are also read.
        let mut conflicts = Vec::new();
        for (b, pi, _, _) in items {
            let ctx = BindContext { geometry: true, albedo: b.albedo, mip_targets: Vec::new(), attachments: Vec::new() };
            for img in self.bound_images(*pi, &ctx)? {
                if attachments.contains(&img) && !conflicts.contains(&img) {
                    conflicts.push(img);
                }
            }
        }
        self.snapshot(cmd, &conflicts)?;
        let extent = self.render_extent(&scope);
        let (color_formats, depth_format) = self.scope_formats(&scope);
        self.begin_scope(cmd, &scope, extent)?;
        // Geometry keeps Minecraft's viewport (the depth buffer's size: the screen, or the
        // shadow map) and is clipped to the smallest attachment, as in GL: a program that
        // writes a `size.buffer`-scaled target fills only its part of the screen.
        let full = scope.depth.map_or(extent, |d| self.arena.image(d).extent_2d());
        self.set_viewport_clipped(cmd, full, extent);
        let mut set_cache: HashMap<(u32, ImageId), Vec<vk::DescriptorSet>> = HashMap::new();
        let dim = self.dim;
        for (b, pi, _, states) in items {
            let program = &dim.programs[*pi as usize];
            let Some(MeshDraws { layout, vertex: vb, index: ib, draws }) = self.mesh(b.mesh) else { continue };
            let cull = if b.shadow {
                vk::CullModeFlags::NONE
            } else {
                match program.cull {
                    Some(true) => vk::CullModeFlags::BACK,
                    Some(false) => vk::CullModeFlags::NONE,
                    None if b.cull_back => vk::CullModeFlags::BACK,
                    None => vk::CullModeFlags::NONE,
                }
            };
            let key = VariantKey {
                color_formats: color_formats.clone(),
                attachments: states.clone(),
                depth_format,
                depth_test: true,
                depth_write: b.depth_write,
                depth_compare: self.depth.compare,
                cull,
                layout: Some(layout.profile),
                neg_one_to_one: self.depth.neg_one_to_one,
            };
            let built = match &mut self.programs[*pi as usize] {
                ProgramState::Ready(p) => {
                    let before = p.input_warnings.len();
                    let r = p.graphics_pipeline(self.gpu, &mut self.arena, &key, Some(layout));
                    let new: Vec<String> = p.input_warnings[before..].to_vec();
                    Some((r, new))
                }
                ProgramState::Skipped => None,
            };
            let Some((result, input_warnings)) = built else { continue };
            for w in input_warnings {
                self.warn(format!("{}: {w}", program.name));
            }
            let pipeline = match result {
                Ok(p) => p,
                Err(e) => {
                    self.stats.skip_program(&program.name, &e);
                    continue;
                }
            };
            let sets = match set_cache.get(&(*pi, b.albedo)) {
                Some(s) => s.clone(),
                None => {
                    let ctx = BindContext { geometry: true, albedo: b.albedo, mip_targets: Vec::new(), attachments: attachments.clone() };
                    let s = match self.write_sets(*pi, &ctx) {
                        Ok(s) => s,
                        Err(e) => {
                            self.stats.skip_program(&program.name, &e.to_string());
                            continue;
                        }
                    };
                    set_cache.insert((*pi, b.albedo), s.clone());
                    s
                }
            };
            let offsets = self.dynamic_offsets(*pi, &b.state, fs, frame_offset);
            let Some(p) = self.program(*pi) else { continue };
            let (playout, push) = (p.layout, p.push_constants);
            // Push constants per sub-draw (Sodium regions differ in `u_RegionOffset`).
            let pushes: Vec<Vec<u8>> = match push {
                Some((_, size)) => draws
                    .iter()
                    .map(|(_, region)| {
                        let mut st = b.state.clone();
                        if let Some(r) = region {
                            st.region_offset = [0, 1, 2].map(|i| (f64::from(r.origin[i]) - fs.camera[i]) as f32);
                            st.region_id = r.id;
                        }
                        push_constant_bytes(&p.push_members, size, fs, &st)
                    })
                    .collect(),
                None => Vec::new(),
            };
            let zero = self.arena.buffer(self.tex.zero).buffer;
            let defaults = self.arena.buffer(self.tex.attribute_defaults).buffer;
            let instances = self.scene.instances.map(|i| self.arena.buffer(i).buffer);
            let d = &self.gpu.device;
            unsafe {
                d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                if !sets.is_empty() {
                    d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, playout, 0, &sets, &offsets);
                }
                d.cmd_bind_vertex_buffers(cmd, 0, &[vb], &[0]);
                if layout.bindings.iter().any(|x| x.binding == 1) {
                    d.cmd_bind_vertex_buffers(cmd, 1, &[instances.unwrap_or(zero)], &[0]);
                }
                d.cmd_bind_vertex_buffers(cmd, NULL_VERTEX_BINDING, &[defaults], &[0]);
                d.cmd_bind_index_buffer(cmd, ib, 0, vk::IndexType::UINT32);
                for (k, (sd, _)) in draws.iter().enumerate() {
                    if let (Some((stages, _)), Some(bytes)) = (push, pushes.get(k))
                        && (k == 0 || pushes[k - 1] != *bytes)
                    {
                        d.cmd_push_constants(cmd, playout, stages, 0, bytes);
                    }
                    d.cmd_draw_indexed(cmd, sd.index_count, 1, sd.first_index, sd.vertex_offset, sd.first_instance);
                }
            }
            self.stats.draws += draws.len() as u32;
        }
        self.gpu.cmd_end_rendering(cmd);
        full_barrier(&self.gpu.device, cmd);
        Ok(())
    }

    /// Vertex layout, vertex buffer, index buffer and sub-draws of a mesh reference.
    fn mesh(&self, m: MeshRef) -> Option<MeshDraws> {
        let s = &self.scene;
        if let MeshRef::Sodium(layer) = m {
            let sodium = s.sodium.as_ref()?;
            let mesh = sodium.layers.get(layer)?.as_ref()?;
            let regions = sodium.draw_regions.get(layer)?;
            let draws = mesh.draws.iter().zip(regions).map(|(d, r)| (*d, sodium.regions.get(*r).copied())).collect();
            return Some(MeshDraws { layout: mesh.layout, vertex: self.arena.buffer(mesh.vertex).buffer, index: self.arena.buffer(mesh.index).buffer, draws });
        }
        let (mesh, only) = match m {
            MeshRef::Solid => (s.solid.as_ref()?, None),
            MeshRef::Cutout => (s.cutout.as_ref()?, None),
            MeshRef::Water => (s.water.as_ref()?, None),
            MeshRef::Entity(i) => (s.entities.as_ref()?, Some(i)),
            MeshRef::Sky(i) => (s.sky_disc.as_ref()?, Some(i)),
            MeshRef::Sun => (s.sun.as_ref()?, None),
            MeshRef::Moon(i) => (s.moon.as_ref()?, Some(i)),
            MeshRef::DhOpaque(i) => (s.dh.get(i)?.1.as_ref()?, None),
            MeshRef::DhWater(i) => (s.dh.get(i)?.2.as_ref()?, None),
            MeshRef::Sodium(_) => return None,
        };
        let draws = match only {
            Some(i) => vec![(*mesh.draws.get(i)?, None)],
            None => mesh.draws.iter().map(|d| (*d, None)).collect(),
        };
        Some(MeshDraws { layout: mesh.layout, vertex: self.arena.buffer(mesh.vertex).buffer, index: self.arena.buffer(mesh.index).buffer, draws })
    }

    // --------------------------------------------------------------------------------
    // Fullscreen passes
    // --------------------------------------------------------------------------------

    /// Run a composite-style program. Returns whether it drew.
    fn fullscreen(&mut self, cmd: vk::CommandBuffer, index: u32, group: PassGroup, fs: &FrameState, frame_offset: u32) -> Result<bool, RuntimeError> {
        let dim = self.dim;
        let Some(program) = dim.programs.get(index as usize) else {
            self.warn(format!("pass {group:?} references program {index}, which does not exist"));
            return Ok(false);
        };
        let Some(prepared) = self.program(index) else { return Ok(false) };
        if prepared.compute {
            self.stats.skip_program(&program.name, "a compute program is used as a fullscreen pass");
            return Ok(false);
        }
        let outputs = prepared.output_classes();
        let shadow = group == PassGroup::ShadowComp;
        // Mipmaps requested by the program.
        for &t in &program.mipmap_targets {
            let img = if shadow {
                self.targets.shadow_color.get(&t).filter(|p| p.mipped).map(|p| p.images[self.flips.shadow_read(t)])
            } else {
                self.targets.color.get(&t).filter(|p| p.mipped).map(|p| p.images[self.flips.read(t)])
            };
            if let Some(img) = img {
                self.generate_mips(cmd, img);
            }
        }
        // Attachments by location. `assigned` catches a target listed twice; `written`
        // holds the targets that are really written (shadowcolor flips).
        let mut colors: Vec<Option<ImageId>> = Vec::new();
        let mut states = Vec::new();
        let mut assigned: Vec<u32> = Vec::new();
        let mut written: Vec<u32> = Vec::new();
        if group == PassGroup::Final {
            let (output, write) = self.output_attachment(program, &outputs, 0, self.targets.output, self.targets.extent)?;
            colors.push(Some(output));
            states.push(AttachmentState { write, blend: program.draw_buffers.first().and_then(|t| output_blend(program, *t)).or(program.blend) });
        } else {
            for (i, &t) in program.draw_buffers.iter().enumerate() {
                let loc = program.output_slots.get(i).copied().unwrap_or(i as u32) as usize;
                if loc >= self.gpu.limits().max_color_attachments as usize {
                    self.warn(format!("{}: output location {loc} exceeds the colour attachment limit; the output is discarded", program.name));
                    continue;
                }
                if colors.len() <= loc {
                    colors.resize(loc + 1, None);
                    states.resize(loc + 1, AttachmentState { write: false, blend: None });
                }
                let pair = if shadow { self.targets.shadow_color.get(&t) } else { self.targets.color.get(&t) };
                let img = match pair {
                    Some(p) if !assigned.contains(&t) => {
                        assigned.push(t);
                        p.images[if shadow { self.flips.shadow_write(t) } else { self.flips.write(t) }]
                    }
                    _ => {
                        let class = outputs.get(&(loc as u32)).map_or("float", String::as_str);
                        self.warn(format!("{}: target {t} of output {i} has no image (or is written twice); the output is discarded", program.name));
                        self.sink(Self::sink_format(class), loc, self.targets.extent)?
                    }
                };
                let (img, write) = self.output_attachment(program, &outputs, loc, img, self.targets.extent)?;
                if write {
                    written.push(t);
                }
                colors[loc] = Some(img);
                states[loc] = AttachmentState { write, blend: output_blend(program, t) };
            }
        }
        if colors.is_empty() {
            self.warn(format!("{}: no draw buffers; nothing to draw", program.name));
            return Ok(false);
        }
        let scope = Scope { colors, depth: None };
        let attachments = Self::scope_attachments(&scope);
        let ctx = BindContext { geometry: false, albedo: self.tex.white, mip_targets: program.mipmap_targets.clone(), attachments: attachments.clone() };
        let conflicts: Vec<ImageId> = self.bound_images(index, &ctx)?.into_iter().filter(|i| attachments.contains(i)).collect();
        self.snapshot(cmd, &conflicts)?;
        let (color_formats, depth_format) = self.scope_formats(&scope);
        let key = VariantKey {
            color_formats,
            attachments: states,
            depth_format,
            depth_test: false,
            depth_write: false,
            depth_compare: vk::CompareOp::ALWAYS,
            cull: vk::CullModeFlags::NONE,
            layout: None,
            neg_one_to_one: self.depth.neg_one_to_one,
        };
        let pipeline = match &mut self.programs[index as usize] {
            ProgramState::Ready(p) => p.graphics_pipeline(self.gpu, &mut self.arena, &key, None),
            ProgramState::Skipped => return Ok(false),
        };
        let pipeline = match pipeline {
            Ok(p) => p,
            Err(e) => {
                self.stats.skip_program(&program.name, &e);
                return Ok(false);
            }
        };
        let sets = match self.write_sets(index, &ctx) {
            Ok(s) => s,
            Err(e) => {
                self.stats.skip_program(&program.name, &e.to_string());
                return Ok(false);
            }
        };
        let offsets = self.dynamic_offsets(index, &DrawState::new(fs), fs, frame_offset);
        let Some(p) = self.program(index) else { return Ok(false) };
        let (playout, push) = (p.layout, p.push_constants);
        let push_bytes = push.map(|(_, size)| push_constant_bytes(&p.push_members, size, fs, &DrawState::new(fs))).unwrap_or_default();
        let extent = self.render_extent(&scope);
        self.begin_scope(cmd, &scope, extent)?;
        self.set_viewport(cmd, extent, Some(&program.viewport));
        let d = &self.gpu.device;
        unsafe {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            if !sets.is_empty() {
                d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, playout, 0, &sets, &offsets);
            }
            if let Some((stages, _)) = push {
                d.cmd_push_constants(cmd, playout, stages, 0, &push_bytes);
            }
            // The `fullscreen` profile's quad: two triangles from gl_VertexIndex.
            d.cmd_draw(cmd, FULLSCREEN_VERTICES, 1, 0, 0);
        }
        self.gpu.cmd_end_rendering(cmd);
        full_barrier(&self.gpu.device, cmd);
        self.stats.draws += 1;
        if shadow {
            self.flips.flip_shadow(&written);
        }
        Ok(true)
    }

    // --------------------------------------------------------------------------------
    // Copies
    // --------------------------------------------------------------------------------

    fn copy_image(&self, cmd: vk::CommandBuffer, src: ImageId, dst: ImageId) {
        let (s, t) = (self.arena.image(src), self.arena.image(dst));
        if s.desc.format != t.desc.format || s.desc.extent != t.desc.extent {
            return;
        }
        let sub = vk::ImageSubresourceLayers { aspect_mask: s.aspect, mip_level: 0, base_array_layer: 0, layer_count: 1 };
        let region = vk::ImageCopy { src_subresource: sub, src_offset: vk::Offset3D::default(), dst_subresource: sub, dst_offset: vk::Offset3D::default(), extent: vk::Extent3D { width: s.desc.extent[0], height: s.desc.extent[1], depth: 1 } };
        let d = &self.gpu.device;
        full_barrier(d, cmd);
        unsafe { d.cmd_copy_image(cmd, s.image, vk::ImageLayout::GENERAL, t.image, vk::ImageLayout::GENERAL, &[region]) };
        full_barrier(d, cmd);
    }

    fn generate_mips(&mut self, cmd: vk::CommandBuffer, image: ImageId) {
        let img = self.arena.image(image);
        let levels = img.desc.mip_levels;
        if levels <= 1 {
            return;
        }
        let feats = self.gpu.format_features(img.desc.format);
        if !crate::mipgen::can_blit(feats) {
            // Render the levels instead (lavapipe cannot blit into B10G11R11_UFLOAT).
            let (name, format) = (img.desc.name.clone(), img.desc.format);
            let result = if crate::mipgen::can_render(format, feats) {
                self.render_mips(cmd, image)
            } else {
                Err(RuntimeError::Unsupported(format!("format {format:?} can neither be blitted nor rendered")))
            };
            if let Err(e) = result {
                self.warn(format!("{name}: mipmaps are not generated: {e}"));
            }
            return;
        }
        let filter = if texel::numeric_class(img.desc.format) == NumericClass::Float && feats.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR) { vk::Filter::LINEAR } else { vk::Filter::NEAREST };
        let d = &self.gpu.device;
        let (w, h) = (img.desc.extent[0] as i32, img.desc.extent[1] as i32);
        for level in 1..levels {
            full_barrier(d, cmd);
            let src = |l: u32| vk::ImageSubresourceLayers { aspect_mask: img.aspect, mip_level: l, base_array_layer: 0, layer_count: 1 };
            let e = |l: u32| vk::Offset3D { x: (w >> l).max(1), y: (h >> l).max(1), z: 1 };
            let blit = vk::ImageBlit { src_subresource: src(level - 1), src_offsets: [vk::Offset3D::default(), e(level - 1)], dst_subresource: src(level), dst_offsets: [vk::Offset3D::default(), e(level)] };
            unsafe { d.cmd_blit_image(cmd, img.image, vk::ImageLayout::GENERAL, img.image, vk::ImageLayout::GENERAL, &[blit], filter) };
        }
        full_barrier(d, cmd);
    }

    /// Without a `final` program, colortex0 is copied to the output.
    fn copy_to_output(&mut self, cmd: vk::CommandBuffer) {
        let out = self.arena.image(self.targets.output);
        let (out_img, out_ext) = (out.image, out.extent_2d());
        let Some(pair) = self.targets.color.get(&0) else { return };
        let src = self.arena.image(pair.images[self.flips.read(0)]);
        let feats = self.gpu.format_features(src.desc.format);
        let d = &self.gpu.device;
        full_barrier(d, cmd);
        if texel::numeric_class(src.desc.format) != NumericClass::Float || !feats.contains(vk::FormatFeatureFlags::BLIT_SRC) {
            unsafe { d.cmd_clear_color_image(cmd, out_img, vk::ImageLayout::GENERAL, &vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] }, &[self.arena.image(self.targets.output).full_range()]) };
            full_barrier(d, cmd);
            self.warn("colortex0 cannot be copied to the output (integer format)".to_string());
            return;
        }
        let sub = vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 };
        let se = src.extent_2d();
        let blit = vk::ImageBlit {
            src_subresource: sub,
            src_offsets: [vk::Offset3D::default(), vk::Offset3D { x: se.width as i32, y: se.height as i32, z: 1 }],
            dst_subresource: sub,
            dst_offsets: [vk::Offset3D::default(), vk::Offset3D { x: out_ext.width as i32, y: out_ext.height as i32, z: 1 }],
        };
        unsafe { d.cmd_blit_image(cmd, src.image, vk::ImageLayout::GENERAL, out_img, vk::ImageLayout::GENERAL, &[blit], vk::Filter::NEAREST) };
        full_barrier(d, cmd);
    }

    /// End-of-frame alt → main copies for buffers flipped an odd number of times.
    fn end_of_frame(&mut self, cmd: vk::CommandBuffer) {
        let dim = self.dim;
        let mut copies: Vec<(ImageId, ImageId)> = Vec::new();
        for i in self.flips.end_of_frame_copies(&dim.end_of_frame_copies) {
            if let Some(pair) = self.targets.color.get(&i) {
                copies.push((pair.images[1], pair.images[0]));
            }
        }
        let keep: Vec<u32> = self.targets.shadow_color.iter().filter(|(_, p)| !p.clear).map(|(i, _)| *i).collect();
        for i in self.flips.shadow_copies(&keep) {
            if let Some(pair) = self.targets.shadow_color.get(&i) {
                copies.push((pair.images[1], pair.images[0]));
            }
        }
        for (a, m) in copies {
            self.copy_image(cmd, a, m);
        }
    }
}
