//! Textures: the procedural block atlas (with mip levels), normal/specular atlases,
//! lightmap, overlay, noise, DH block atlas, sun/moon and entity textures, plus loading of
//! pack custom textures through a [`TextureSource`].

use crate::scene::world::hash64;
use crate::texel::{self, PixelTransfer};
use ash::vk;
use sb_core::model;
use std::path::{Component, Path, PathBuf};

/// Supplies files of the shader pack (custom textures, `texture.noise`, SSBO initial
/// contents) and, optionally, resource-pack textures. Every method may return `None`;
/// the runtime then binds a fallback texture and records it in the stats.
pub trait TextureSource {
    /// Read a file of the pack. `path` is relative to the `shaders/` directory with `/`
    /// separators (as in the model).
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// Read a resource location such as `minecraft:textures/block/stone.png` (PNG bytes).
    /// When this returns `None`, the block atlas, sun, moon and cloud locations fall back to
    /// the synthetic scene's own textures.
    fn resource(&self, location: &str) -> Option<Vec<u8>> {
        let _ = location;
        None
    }
}

/// A [`TextureSource`] without any files.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTextures;

impl TextureSource for NoTextures {
    fn read(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }
}

impl<F: Fn(&str) -> Option<Vec<u8>>> TextureSource for F {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self(path)
    }
}

/// A [`TextureSource`] reading from a pack's `shaders/` directory on disk. Paths that
/// are absolute or contain `..` are rejected.
#[derive(Debug, Clone)]
pub struct DirTextureSource {
    root: PathBuf,
}

impl DirTextureSource {
    /// Read files below `shaders_dir`.
    pub fn new(shaders_dir: impl Into<PathBuf>) -> Self {
        Self { root: shaders_dir.into() }
    }
}

impl TextureSource for DirTextureSource {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = Path::new(path.trim_start_matches('/'));
        if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            return None;
        }
        std::fs::read(self.root.join(rel)).ok()
    }
}

/// Dimensionality of a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TexDim {
    D1,
    D2,
    D3,
}

/// Texture contents ready for upload: tightly packed levels, level 0 first.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextureData {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub dim: TexDim,
    pub format: vk::Format,
    pub levels: Vec<Vec<u8>>,
}

impl TextureData {
    pub fn rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self { width, height, depth: 1, dim: TexDim::D2, format: vk::Format::R8G8B8A8_UNORM, levels: vec![pixels] }
    }

    /// Add box-filtered mip levels (RGBA8 2D only) down to `min_size`.
    pub fn with_mips(mut self, min_size: u32) -> Self {
        if self.format != vk::Format::R8G8B8A8_UNORM || self.dim != TexDim::D2 {
            return self;
        }
        let (mut w, mut h) = (self.width, self.height);
        while w / 2 >= min_size.max(1) && h / 2 >= min_size.max(1) {
            let prev = self.levels.last().cloned().unwrap_or_default();
            let (nw, nh) = (w / 2, h / 2);
            let mut out = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    for c in 0..4 {
                        let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                            .iter()
                            .map(|(dx, dy)| u32::from(prev[(((y * 2 + dy) * w + x * 2 + dx) * 4 + c) as usize]))
                            .sum();
                        out[((y * nw + x) * 4 + c) as usize] = ((s + 2) / 4) as u8;
                    }
                }
            }
            self.levels.push(out);
            w = nw;
            h = nh;
        }
        self
    }

    pub fn mip_levels(&self) -> u32 {
        self.levels.len().max(1) as u32
    }
}

// ------------------------------------------------------------------------------------
// Block atlas
// ------------------------------------------------------------------------------------

/// Tiles per row/column of the block atlas.
pub(crate) const ATLAS_GRID: u32 = 4;
/// Tile size in pixels.
pub(crate) const TILE: u32 = 16;
/// Mip levels of the atlases (16 px tiles down to 1 px, as `MC_MIPMAP_LEVEL = 4`).
pub(crate) const ATLAS_MIPS: u32 = 5;

/// Tiles of the procedural block atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tile {
    GrassTop = 0,
    GrassSide = 1,
    Dirt = 2,
    Stone = 3,
    Sand = 4,
    Water = 5,
    Leaves = 6,
    LogSide = 7,
    LogTop = 8,
    Planks = 9,
}

impl Tile {
    pub const ALL: [Tile; 10] =
        [Tile::GrassTop, Tile::GrassSide, Tile::Dirt, Tile::Stone, Tile::Sand, Tile::Water, Tile::Leaves, Tile::LogSide, Tile::LogTop, Tile::Planks];

    /// `(u0, v0, u1, v1)` of the tile in the atlas.
    pub fn uv_rect(self) -> (f32, f32, f32, f32) {
        let i = self as u32;
        let (c, r) = ((i % ATLAS_GRID) as f32, (i / ATLAS_GRID) as f32);
        let g = ATLAS_GRID as f32;
        (c / g, r / g, (c + 1.0) / g, (r + 1.0) / g)
    }
}

fn pixel_noise(x: u32, y: u32, salt: u64) -> f32 {
    (hash64(u64::from(x) | (u64::from(y) << 16) | (salt << 32)) >> 40) as f32 / (1u64 << 24) as f32
}

/// RGBA (0..1) of pixel `(x, y)` of a tile.
fn tile_pixel(tile: Tile, x: u32, y: u32) -> [f32; 4] {
    let n = pixel_noise(x, y, tile as u64 + 1);
    let v = 0.85 + 0.3 * (n - 0.5);
    let mul = |c: [f32; 3], k: f32| [c[0] * k, c[1] * k, c[2] * k, 1.0];
    match tile {
        Tile::GrassTop => mul([0.62, 0.62, 0.62], v),
        Tile::Dirt => mul([0.53, 0.38, 0.26], v),
        Tile::GrassSide => {
            if y < 3 || (y == 3 && n > 0.5) {
                mul([0.36, 0.55, 0.22], v)
            } else {
                mul([0.53, 0.38, 0.26], v)
            }
        }
        Tile::Stone => mul([0.5, 0.5, 0.5], 0.8 + 0.4 * (n - 0.5)),
        Tile::Sand => mul([0.86, 0.81, 0.63], 0.95 + 0.1 * (n - 0.5)),
        Tile::Water => [0.8 * v, 0.8 * v, 0.8 * v, 0.72],
        Tile::Leaves => {
            let hole = pixel_noise(x, y, 99) < 0.22;
            let c = mul([0.55, 0.55, 0.55], v);
            [c[0], c[1], c[2], if hole { 0.0 } else { 1.0 }]
        }
        Tile::LogSide => mul([0.42, 0.32, 0.2], if x.is_multiple_of(4) { 0.75 } else { v }),
        Tile::LogTop => {
            let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
            if d > 7.0 { mul([0.42, 0.32, 0.2], v) } else { mul([0.69, 0.56, 0.36], if (d as u32).is_multiple_of(3) { 0.85 } else { 1.0 }) }
        }
        Tile::Planks => mul([0.66, 0.53, 0.33], if y.is_multiple_of(4) { 0.7 } else { v }),
    }
}

fn to_u8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The procedural block atlas (64x64, 4x4 tiles of 16 px, unused tiles magenta) with
/// [`ATLAS_MIPS`] levels.
pub(crate) fn block_atlas() -> TextureData {
    let size = ATLAS_GRID * TILE;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = (y / TILE) * ATLAS_GRID + x / TILE;
            let c = match Tile::ALL.get(i as usize) {
                Some(t) => to_u8(tile_pixel(*t, x % TILE, y % TILE)),
                None => [255, 0, 255, 255],
            };
            px[((y * size + x) * 4) as usize..][..4].copy_from_slice(&c);
        }
    }
    TextureData::rgba8(size, size, px).with_mips(size >> (ATLAS_MIPS - 1))
}

/// A flat RGBA8 texture of `size` with mips like the atlas (normals/specular atlases).
pub(crate) fn flat_atlas(color: [u8; 4]) -> TextureData {
    let size = ATLAS_GRID * TILE;
    TextureData::rgba8(size, size, color.repeat((size * size) as usize)).with_mips(size >> (ATLAS_MIPS - 1))
}

/// Tiles of the DH block atlas (`textureTile`; tile `t` occupies x = 16t..16t+16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // the full palette; the LOD generator uses a subset
pub(crate) enum DhTile {
    Flat = 0,
    GrassTop = 1,
    GrassSide = 2,
    Dirt = 3,
    Stone = 4,
    Sand = 5,
    Water = 6,
    Leaves = 7,
    Log = 8,
}

/// The DH block atlas: 256x16, one 16 px tile per `textureTile` (tile 0 = white).
pub(crate) fn dh_atlas() -> TextureData {
    let tiles = [None, Some(Tile::GrassTop), Some(Tile::GrassSide), Some(Tile::Dirt), Some(Tile::Stone), Some(Tile::Sand), Some(Tile::Water), Some(Tile::Leaves), Some(Tile::LogSide)];
    let (w, h) = (256u32, TILE);
    let mut px = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let t = (x / TILE) as usize;
            let c = match tiles.get(t) {
                Some(Some(tile)) => to_u8(tile_pixel(*tile, x % TILE, y)),
                Some(None) => [255; 4],
                None => [128, 128, 128, 255],
            };
            px[((y * w + x) * 4) as usize..][..4].copy_from_slice(&c);
        }
    }
    TextureData::rgba8(w, h, px)
}

/// Minecraft's light brightness curve (`LightTexture`, gamma 0).
fn brightness(level: u32) -> f32 {
    let f = level as f32 / 15.0;
    f / (4.0 - 3.0 * f)
}

/// The 16x16 lightmap: x = block light, y = sky light; warm block light, cool sky light
/// scaled by `daylight` (0..1).
pub(crate) fn lightmap(daylight: f32) -> TextureData {
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for sky in 0..16u32 {
        for block in 0..16u32 {
            let b = brightness(block);
            let s = brightness(sky) * (0.15 + 0.85 * daylight.clamp(0.0, 1.0));
            let c = [
                0.04 + b * 1.0 + s * 0.95,
                0.04 + b * 0.85 + s * 0.97,
                0.06 + b * 0.62 + s * 1.0,
                1.0,
            ];
            px.extend_from_slice(&to_u8(c));
        }
    }
    TextureData::rgba8(16, 16, px)
}

/// Minecraft's 16x16 overlay texture (`OverlayTexture`): rows 0..7 are the red hurt
/// flash (alpha 0.7), the others white with an alpha ramp along x (white flash).
pub(crate) fn overlay() -> TextureData {
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            if y < 8 {
                px.extend_from_slice(&[255, 0, 0, 178]);
            } else {
                let a = ((1.0 - x as f32 / 15.0 * 0.75) * 255.0) as u8;
                px.extend_from_slice(&[255, 255, 255, a]);
            }
        }
    }
    TextureData::rgba8(16, 16, px)
}

/// `noisetex`: deterministic white noise, RGB8 stored as RGBA8 (alpha 255).
pub(crate) fn noise(resolution: u32) -> TextureData {
    let res = resolution.clamp(1, 4096);
    let mut px = Vec::with_capacity((res * res * 4) as usize);
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..res * res {
        state = hash64(state);
        px.extend_from_slice(&[state as u8, (state >> 8) as u8, (state >> 16) as u8, 255]);
    }
    TextureData::rgba8(res, res, px)
}

/// A 1x1 texture.
pub(crate) fn solid(color: [u8; 4]) -> TextureData {
    TextureData::rgba8(1, 1, color.to_vec())
}

/// The entity texture (64x32): body on the left half, head on the right.
pub(crate) fn entity_texture() -> TextureData {
    let (w, h) = (64u32, 32u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = pixel_noise(x, y, 500);
            let c = if x < 32 {
                [0.93 * (0.9 + 0.2 * n), 0.62, 0.62, 1.0]
            } else if y < 16 && (x == 40 || x == 52) && (6..9).contains(&y) {
                [0.05, 0.05, 0.05, 1.0]
            } else {
                [0.95, 0.7 * (0.9 + 0.2 * n), 0.7, 1.0]
            };
            px.extend_from_slice(&to_u8(c));
        }
    }
    TextureData::rgba8(w, h, px)
}

/// The sun texture (32x32): a bright disc on black (vanilla blends it additively).
pub(crate) fn sun_texture() -> TextureData {
    let mut px = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let d = ((x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2)).sqrt();
            let c = if d < 10.0 { [1.0, 0.95, 0.7, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
            px.extend_from_slice(&to_u8(c));
        }
    }
    TextureData::rgba8(32, 32, px)
}

/// The cloud map (256x256, like vanilla `clouds.png`): opaque white cells where smoothed
/// block noise is high, transparent elsewhere.
pub(crate) fn cloud_texture() -> TextureData {
    const SIZE: u32 = 256;
    const CELL: u32 = 8;
    let cells = SIZE / CELL;
    let coarse = |cx: u32, cy: u32| pixel_noise(cx % cells, cy % cells, 900);
    let mut px = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (cx, cy) = (x / CELL + cells, y / CELL + cells);
            let mut sum = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (cells - 1, 0), (0, cells - 1)] {
                sum += coarse(cx + dx, cy + dy);
            }
            let v = sum / 5.0 + 0.15 * (pixel_noise(x / 2, y / 2, 901) - 0.5);
            px.extend_from_slice(&if v > 0.56 { [255, 255, 255, 255] } else { [255, 255, 255, 0] });
        }
    }
    TextureData::rgba8(SIZE, SIZE, px)
}

/// The moon phases texture (64x32, 4x2 phases of 16 px).
pub(crate) fn moon_texture() -> TextureData {
    let mut px = Vec::with_capacity(64 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..64u32 {
            let phase = (y / 16) * 4 + x / 16;
            let (lx, ly) = ((x % 16) as f32 - 7.5, (y % 16) as f32 - 7.5);
            let inside = lx * lx + ly * ly < 36.0;
            let lit = lx < (phase as f32 - 3.5) * 2.0 || phase == 0;
            let c = if inside && lit { [0.85, 0.85, 0.9, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
            px.extend_from_slice(&to_u8(c));
        }
    }
    TextureData::rgba8(64, 32, px)
}

// ------------------------------------------------------------------------------------
// Pack textures
// ------------------------------------------------------------------------------------

/// Largest accepted texture dimension.
const MAX_DIM: u32 = 16384;
/// Largest accepted texture payload.
const MAX_BYTES: u64 = 512 << 20;

fn decode_png(bytes: &[u8]) -> Result<TextureData, String> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| format!("cannot decode PNG: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(format!("unsupported image size {w}x{h}"));
    }
    Ok(TextureData::rgba8(w, h, rgba.into_raw()))
}

/// The synthetic scene's own texture for a vanilla resource location, used when the
/// [`TextureSource`] has no resources: packs bind `minecraft:textures/atlas/blocks.png`
/// as a custom texture (Rethinking Voxels voxelizes block colours from it), the
/// celestial textures and `clouds.png` (bloop samples it as a cloud map). The namespace
/// defaults to `minecraft`, as in resource locations.
pub(crate) fn builtin_resource(location: &str) -> Option<TextureData> {
    let location = location.trim();
    let path = location.strip_prefix("minecraft:").unwrap_or(location);
    if location.contains(':') && !location.starts_with("minecraft:") {
        return None;
    }
    match path {
        "textures/atlas/blocks.png" => Some(block_atlas()),
        "textures/environment/sun.png" => Some(sun_texture()),
        "textures/environment/moon_phases.png" => Some(moon_texture()),
        "textures/environment/clouds.png" => Some(cloud_texture()),
        _ => None,
    }
}

/// Load a pack texture. `dynamic` resolves `minecraft:dynamic/*` names.
pub(crate) fn load(src: &model::TextureSource, files: &dyn TextureSource, dynamic: &dyn Fn(&str) -> Option<TextureData>) -> Result<TextureData, String> {
    match src {
        model::TextureSource::PackImage { path } => {
            let bytes = files.read(path).ok_or_else(|| format!("`{path}` not found"))?;
            decode_png(&bytes).map_err(|e| format!("`{path}`: {e}"))
        }
        model::TextureSource::Resource { location } => match files.resource(location) {
            Some(bytes) => decode_png(&bytes).map_err(|e| format!("`{location}`: {e}")),
            None => builtin_resource(location).ok_or_else(|| format!("resource `{location}` not available")),
        },
        model::TextureSource::Dynamic { name } => dynamic(name).ok_or_else(|| format!("dynamic texture `{name}` is not provided")),
        model::TextureSource::Raw { path, target, format, size, pixel_format, pixel_type, .. } => {
            let transfer = PixelTransfer::parse(pixel_format, pixel_type).ok_or_else(|| format!("`{path}`: unsupported pixel format {pixel_format}/{pixel_type}"))?;
            let vk_format = vk::Format::from_raw(format.vk_format_renderable() as i32);
            let (dim, w, h, d) = match target.as_str() {
                "1d" => (TexDim::D1, size[0], 1, 1),
                "2d" | "2d_rect" => (TexDim::D2, size[0], size[1], 1),
                "3d" => (TexDim::D3, size[0], size[1], size[2]),
                other => return Err(format!("`{path}`: unsupported texture target `{other}`")),
            };
            if w == 0 || h == 0 || d == 0 || w > MAX_DIM || h > MAX_DIM || d > 2048 {
                return Err(format!("`{path}`: unsupported size {w}x{h}x{d}"));
            }
            let texels = u64::from(w) * u64::from(h) * u64::from(d);
            let bytes_needed = texels * texel::texel_size(vk_format).unwrap_or(16) as u64;
            if bytes_needed > MAX_BYTES {
                return Err(format!("`{path}`: texture too large ({bytes_needed} bytes)"));
            }
            let bytes = files.read(path).ok_or_else(|| format!("`{path}` not found"))?;
            let data = texel::convert(&bytes, &transfer, texels as usize, vk_format).ok_or_else(|| format!("`{path}`: cannot convert to {format}"))?;
            Ok(TextureData { width: w, height: h, depth: d, dim, format: vk_format, levels: vec![data] })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_layout_and_mips() {
        let a = block_atlas();
        assert_eq!((a.width, a.height), (64, 64));
        assert_eq!(a.mip_levels(), ATLAS_MIPS);
        assert_eq!(a.levels.last().unwrap().len(), 4 * 4 * 4);
        let (u0, v0, u1, v1) = Tile::Planks.uv_rect();
        assert_eq!((u0, v0, u1, v1), (0.25, 0.5, 0.5, 0.75));
        // Leaves have alpha holes; water is translucent.
        let px = |x: u32, y: u32| &a.levels[0][((y * 64 + x) * 4) as usize..][..4];
        let (lu, lv, _, _) = Tile::Leaves.uv_rect();
        let (lx, ly) = ((lu * 64.0) as u32, (lv * 64.0) as u32);
        let holes = (0..16).flat_map(|y| (0..16).map(move |x| (x, y))).filter(|&(x, y)| px(lx + x, ly + y)[3] == 0).count();
        assert!(holes > 10 && holes < 128, "{holes}");
        let (wu, wv, _, _) = Tile::Water.uv_rect();
        assert_eq!(px((wu * 64.0) as u32, (wv * 64.0) as u32)[3], 184);
    }

    #[test]
    fn vanilla_resources_fall_back_to_scene_textures() {
        let atlas = model::TextureSource::Resource { location: "minecraft:textures/atlas/blocks.png".into() };
        let loaded = load(&atlas, &NoTextures, &|_| None).expect("atlas");
        assert_eq!(loaded, block_atlas());
        let clouds = load(&model::TextureSource::Resource { location: "minecraft:textures/environment/clouds.png".into() }, &NoTextures, &|_| None).expect("clouds");
        let covered = clouds.levels[0].chunks(4).filter(|p| p[3] == 255).count() as f64 / f64::from(256 * 256);
        assert!((0.1..0.6).contains(&covered), "cloud cover {covered}");
        let bare = model::TextureSource::Resource { location: "textures/environment/sun.png".into() };
        assert_eq!(load(&bare, &NoTextures, &|_| None).expect("sun"), sun_texture());
        for missing in ["minecraft:textures/block/stone.png", "mymod:textures/atlas/blocks.png", ""] {
            let src = model::TextureSource::Resource { location: missing.into() };
            assert!(load(&src, &NoTextures, &|_| None).is_err(), "{missing}");
        }
        // A source that has the resource wins over the built-in texture.
        let png = {
            let mut buf = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 4])).write_to(&mut buf, image::ImageFormat::Png).expect("png");
            buf.into_inner()
        };
        struct One(Vec<u8>);
        impl TextureSource for One {
            fn read(&self, _: &str) -> Option<Vec<u8>> {
                None
            }
            fn resource(&self, _: &str) -> Option<Vec<u8>> {
                Some(self.0.clone())
            }
        }
        let own = load(&atlas, &One(png), &|_| None).expect("own atlas");
        assert_eq!((own.width, own.height), (2, 2));
    }

    #[test]
    fn procedural_textures() {
        let n = noise(64);
        assert_eq!(n.levels[0].len(), 64 * 64 * 4);
        assert_eq!(n, noise(64));
        assert!(n.levels[0].chunks(4).all(|p| p[3] == 255));
        let l = lightmap(1.0);
        assert_eq!(l.levels[0][0..3], [10, 10, 15]);
        let bright = &l.levels[0][(15 * 16 + 15) * 4..][..4];
        assert_eq!(bright[3], 255);
        assert!(bright[0] == 255);
        let o = overlay();
        // NO_OVERLAY (0, 10) is opaque white.
        assert_eq!(&o.levels[0][(10 * 16) * 4..][..4], &[255, 255, 255, 255]);
        assert_eq!(&o.levels[0][..4], &[255, 0, 0, 178]);
        assert_eq!(dh_atlas().width, 256);
        assert_eq!(&dh_atlas().levels[0][..4], &[255; 4]);
        assert_eq!(flat_atlas([128, 128, 255, 255]).levels[0][..4], [128, 128, 255, 255]);
    }

    #[test]
    fn dir_source_rejects_escapes() {
        let dir = std::env::temp_dir();
        let s = DirTextureSource::new(&dir);
        assert!(s.read("../etc/passwd").is_none());
        assert!(s.read("a/../../b").is_none());
    }

    #[test]
    fn loads_raw_and_png_textures() {
        let raw = model::TextureSource::Raw {
            path: "tex/lut.bin".into(),
            target: "3d".into(),
            dimensions: 3,
            format: sb_core::TextureFormat::RG8,
            size: [2, 2, 2],
            pixel_format: "RG".into(),
            pixel_type: "UNSIGNED_BYTE".into(),
        };
        let files = |p: &str| (p == "tex/lut.bin").then(|| (0u8..16).collect::<Vec<u8>>());
        let t = load(&raw, &files, &|_| None).unwrap();
        assert_eq!((t.width, t.height, t.depth, t.dim), (2, 2, 2, TexDim::D3));
        assert_eq!(t.format, vk::Format::R8G8_UNORM);
        assert_eq!(t.levels[0], (0u8..16).collect::<Vec<u8>>());
        // PNG round trip through the image crate.
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let files = move |p: &str| (p == "a.png").then(|| png.clone());
        let t = load(&model::TextureSource::PackImage { path: "a.png".into() }, &files, &|_| None).unwrap();
        assert_eq!((t.width, t.height), (3, 2));
        assert_eq!(&t.levels[0][..4], &[1, 2, 3, 4]);
        // Failures are errors, not panics.
        assert!(load(&model::TextureSource::PackImage { path: "missing.png".into() }, &NoTextures, &|_| None).is_err());
        let garbage = |_: &str| Some(vec![0u8; 10]);
        assert!(load(&model::TextureSource::PackImage { path: "x.png".into() }, &garbage, &|_| None).is_err());
        assert!(load(&model::TextureSource::Dynamic { name: "minecraft:dynamic/light_map_1".into() }, &NoTextures, &|_| Some(solid([1, 2, 3, 4]))).is_ok());
    }
}
