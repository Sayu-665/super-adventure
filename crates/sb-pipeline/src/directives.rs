//! Source directives, parsed from preprocessed GLSL exactly like Iris does:
//!
//! * comment directives `/* DRAWBUFFERS:0123 */` and `/* RENDERTARGETS: 0,2,11 */`
//!   (Iris `CommentDirectiveParser` / `ProgramDirectives`);
//! * const directives `const <type> <name> = <value>;`, matched line by line even inside
//!   comments (Iris `ConstDirectiveParser`), and their effect on render targets, the shadow
//!   map and global settings (Iris `PackRenderTargetDirectives`, `PackShadowDirectives`,
//!   `PackDirectives`), per-program mipmap requests and compute work groups.

use sb_core::model::{ShadowSettings, WorkGroups};
use sb_core::{Diagnostic, Diagnostics, TextureFormat};
use sb_pack::shaders_properties::LEGACY_BUFFER_NAMES;
use std::collections::{BTreeMap, BTreeSet};

/// Highest number of colortex buffers (Iris `MAX_COLOR_BUFFERS`).
pub const MAX_COLORTEX: u32 = sb_uniforms::MAX_COLOR_TEX;
/// Highest number of shadowcolor buffers (Iris, with `HIGHER_SHADOWCOLOR`).
pub const MAX_SHADOWCOLOR: u32 = sb_uniforms::MAX_SHADOW_COLOR;

/// A comment directive found in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentDirective {
    /// Trimmed text between `KEY:` and `*/`.
    pub value: String,
    /// Byte offset of `KEY:` in the source (later wins between directive kinds).
    pub position: usize,
}

/// Find comment directive `key` (`DRAWBUFFERS`, `RENDERTARGETS`) in `source`, as Iris's
/// `CommentDirectiveParser.findDirective` does: only the **last** occurrence of `KEY:` is
/// considered, it must be preceded by `/*` (whitespace allowed), and it ends at the next
/// `*/`. If the last occurrence is malformed, earlier ones are not consulted.
pub fn find_comment_directive(source: &str, key: &str) -> Option<CommentDirective> {
    let prefix = format!("{key}:");
    let pos = source.rfind(&prefix)?;
    if !source[..pos].trim_end().ends_with("/*") {
        return None;
    }
    let rest = &source[pos + prefix.len()..];
    let end = rest.find("*/")?;
    Some(CommentDirective { value: rest[..end].trim().to_string(), position: pos })
}

/// Draw buffers of a fragment source: the later of the last `DRAWBUFFERS` and the last
/// `RENDERTARGETS` directive (Iris `ProgramDirectives.getAppliedDirective`). `None` when
/// neither exists (the caller applies the program-class default). Malformed entries are
/// dropped with a warning (Iris would fail to load the pack).
pub fn parse_draw_buffers(fragment: &str, diags: &mut Diagnostics) -> Option<Vec<u32>> {
    let draw = find_comment_directive(fragment, "DRAWBUFFERS");
    let targets = find_comment_directive(fragment, "RENDERTARGETS");
    let (is_drawbuffers, d) = match (draw, targets) {
        (Some(a), Some(b)) => {
            if a.position > b.position {
                (true, a)
            } else {
                (false, b)
            }
        }
        (Some(a), None) => (true, a),
        (None, Some(b)) => (false, b),
        (None, None) => return None,
    };
    let mut out = Vec::new();
    if is_drawbuffers {
        for c in d.value.chars() {
            match c.to_digit(10) {
                Some(v) => out.push(v),
                None => diags.push(Diagnostic::warning(
                    "dir.drawbuffers",
                    format!("DRAWBUFFERS `{}`: `{c}` is not a digit; ignored", d.value),
                )),
            }
        }
    } else {
        for part in d.value.split(',') {
            match part.trim().parse::<u32>() {
                Ok(v) => out.push(v),
                Err(_) => diags.push(Diagnostic::warning(
                    "dir.rendertargets",
                    format!("RENDERTARGETS `{}`: `{}` is not a buffer index; ignored", d.value, part.trim()),
                )),
            }
        }
    }
    Some(out)
}

/// Type keyword of a const directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstType {
    /// `const int` (also used for string-valued directives such as formats).
    Int,
    /// `const float`.
    Float,
    /// `const vec2`.
    Vec2,
    /// `const ivec3`.
    Ivec3,
    /// `const vec4`.
    Vec4,
    /// `const bool`.
    Bool,
}

/// One `const <type> <key> = <value>;` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDirective {
    /// Declared type.
    pub ty: ConstType,
    /// Constant name.
    pub key: String,
    /// Raw value text (trimmed, before the `;`).
    pub value: String,
    /// 1-based line in the scanned text.
    pub line: u32,
}

/// Java `Character.isWhitespace` approximation (ASCII whitespace and Unicode spaces
/// except no-break spaces).
fn java_ws(c: char) -> bool {
    matches!(c, '\u{1C}'..='\u{1F}') || (c.is_whitespace() && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}'))
}

/// Java `String.trim()` (strips chars `<= ' '`).
fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Parse one line as a const directive (Iris `ConstDirectiveParser.findDirectiveInLine`).
pub fn parse_const_line(line: &str) -> Option<(ConstType, String, String)> {
    if !line.contains("const") || !line.contains('=') || !line.contains(';') {
        return None;
    }
    let l = java_trim(line).strip_prefix("const")?;
    if !l.starts_with(java_ws) {
        return None;
    }
    let l = java_trim(l);
    let (ty, rest) = [
        ("int", ConstType::Int),
        ("float", ConstType::Float),
        ("vec2", ConstType::Vec2),
        ("ivec3", ConstType::Ivec3),
        ("vec4", ConstType::Vec4),
        ("bool", ConstType::Bool),
    ]
    .iter()
    .find_map(|(kw, t)| l.strip_prefix(kw).map(|r| (*t, r)))?;
    if !rest.starts_with(java_ws) {
        return None;
    }
    let eq = rest.find('=')?;
    let key = java_trim(&rest[..eq]);
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let remaining = &rest[eq + 1..];
    let semi = remaining.find(';')?;
    Some((ty, key.to_string(), java_trim(&remaining[..semi]).to_string()))
}

/// Every const directive of `source`, line by line (Java `\R` line breaks).
pub fn find_const_directives(source: &str) -> Vec<ConstDirective> {
    let mut out = Vec::new();
    for (i, line) in source.split(['\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}']).enumerate() {
        if let Some((ty, key, value)) = parse_const_line(line) {
            out.push(ConstDirective { ty, key, value, line: (i + 1) as u32 });
        }
    }
    out
}

/// Java `Integer.parseInt`.
fn parse_int(v: &str) -> Option<i64> {
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse::<i64>().ok().filter(|x| i32::try_from(*x).is_ok())
}

/// Java `Float.parseFloat` (approximately: a decimal float with an optional `f`/`d`
/// suffix).
fn parse_float(v: &str) -> Option<f32> {
    let t = v.trim();
    let t = t.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(t);
    if t.is_empty() || t.chars().any(|c| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E')) {
        return None;
    }
    t.parse::<f32>().ok().filter(|f| f.is_finite())
}

/// Arguments of a `name(a, b, ...)` constructor value.
fn constructor_args<'a>(value: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let inner = value.strip_prefix(name)?.trim();
    let inner = inner.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner.split(',').map(str::trim).collect())
}

fn parse_floats<const N: usize>(value: &str, name: &str) -> Option<[f32; N]> {
    let args = constructor_args(value, name)?;
    let vals: Option<Vec<f32>> = args.iter().map(|a| parse_float(a)).collect();
    let vals = vals?;
    match vals.len() {
        // Lenient: a single argument splats (Iris rejects it).
        1 => Some([vals[0]; N]),
        n if n == N => vals.try_into().ok(),
        _ => None,
    }
}

/// Settings of one colour buffer collected from directives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BufferDirectives {
    /// `<buf>Format`.
    pub format: Option<TextureFormat>,
    /// `<buf>Clear`.
    pub clear: Option<bool>,
    /// `<buf>ClearColor`.
    pub clear_color: Option<[f32; 4]>,
}

/// The pack-level directive state of one program folder (Iris `PackDirectives`).
#[derive(Debug, Clone, PartialEq)]
pub struct PackDirectiveState {
    /// colortex buffer settings by index (legacy aliases resolved).
    pub colortex: BTreeMap<u32, BufferDirectives>,
    /// shadowcolor buffer settings by index.
    pub shadowcolor: BTreeMap<u32, BufferDirectives>,
    /// Shadow-map consts (`shadowMapResolution`, filtering, mipmaps, ...).
    pub shadow: ShadowSettings,
    /// Names of the numeric consts that were set explicitly.
    pub shadow_explicit: BTreeSet<String>,
    /// `noiseTextureResolution`.
    pub noise_texture_resolution: u32,
    /// `sunPathRotation`.
    pub sun_path_rotation: f32,
    /// `ambientOcclusionLevel` (clamped to 0..1).
    pub ambient_occlusion_level: f32,
    /// `wetnessHalflife` (and, mirroring Iris, `drynessHalflife`).
    pub wetness_half_life: f32,
    /// Dryness half-life (Iris never changes it from the default).
    pub dryness_half_life: f32,
    /// `eyeBrightnessHalflife`.
    pub eye_brightness_half_life: f32,
    /// `centerDepthHalflife`.
    pub center_depth_half_life: f32,
}

impl Default for PackDirectiveState {
    fn default() -> Self {
        let s = sb_core::model::PackSettings::default();
        Self {
            colortex: BTreeMap::new(),
            shadowcolor: BTreeMap::new(),
            shadow: ShadowSettings::default(),
            shadow_explicit: BTreeSet::new(),
            noise_texture_resolution: 256,
            sun_path_rotation: s.sun_path_rotation,
            ambient_occlusion_level: s.ambient_occlusion_level,
            wetness_half_life: s.wetness_half_life,
            dryness_half_life: s.dryness_half_life,
            eye_brightness_half_life: s.eye_brightness_half_life,
            center_depth_half_life: s.center_depth_half_life,
        }
    }
}

/// What a recognized directive key controls (Iris's handler registrations).
enum Handler {
    Format { shadow: bool, index: u32 },
    Clear { shadow: bool, index: u32 },
    ClearColor { shadow: bool, index: u32 },
    Int(fn(&mut PackDirectiveState, i64)),
    Float(fn(&mut PackDirectiveState, f32)),
    Bool(fn(&mut PackDirectiveState, bool)),
    /// Per-index boolean (`shadowtexNMipmap`, `shadowcolorNNearest`, ...).
    IndexedBool(fn(&mut PackDirectiveState, u32, bool), u32),
}

impl Handler {
    fn expected(&self) -> ConstType {
        match self {
            Handler::Format { .. } | Handler::Int(_) => ConstType::Int,
            Handler::Clear { .. } | Handler::Bool(_) | Handler::IndexedBool(..) => ConstType::Bool,
            Handler::ClearColor { .. } => ConstType::Vec4,
            Handler::Float(_) => ConstType::Float,
        }
    }
}

/// Buffer name -> (shadow, index): `colortexN` (N < 32), legacy aliases, `shadowcolorN`
/// (N < 8).
fn buffer_of(name: &str) -> Option<(bool, u32)> {
    if let Some(i) = LEGACY_BUFFER_NAMES.iter().position(|n| *n == name) {
        return Some((false, i as u32));
    }
    let num = |s: &str| -> Option<u32> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s.len() == 1 || !s.starts_with('0')))
            .then(|| s.parse().ok())
            .flatten()
    };
    if let Some(i) = name.strip_prefix("colortex").and_then(num) {
        return (i < MAX_COLORTEX).then_some((false, i));
    }
    if let Some(i) = name.strip_prefix("shadowcolor").and_then(num) {
        return (i < MAX_SHADOWCOLOR).then_some((true, i));
    }
    None
}

fn handler_for(key: &str) -> Option<Handler> {
    // Buffer directives.
    for (suffix, kind) in [("ClearColor", 2u8), ("Format", 0), ("Clear", 1)] {
        if let Some(buf) = key.strip_suffix(suffix)
            && let Some((shadow, index)) = buffer_of(buf)
        {
            return Some(match kind {
                0 => Handler::Format { shadow, index },
                1 => Handler::Clear { shadow, index },
                _ => Handler::ClearColor { shadow, index },
            });
        }
    }
    type S = PackDirectiveState;
    let h = match key {
        "shadowMapResolution" => Handler::Int(|s: &mut S, v| s.shadow.resolution = v.clamp(1, 16384) as u32),
        "shadowMapFov" => Handler::Float(|s: &mut S, v| s.shadow.fov = Some(v)),
        "shadowDistance" => Handler::Float(|s: &mut S, v| s.shadow.distance = v),
        "shadowNearPlane" => Handler::Float(|s: &mut S, v| s.shadow.near_plane = v),
        "shadowFarPlane" => Handler::Float(|s: &mut S, v| s.shadow.far_plane = v),
        "voxelDistance" => Handler::Float(|s: &mut S, v| s.shadow.voxel_distance = v),
        "entityShadowDistanceMul" => Handler::Float(|s: &mut S, v| s.shadow.entity_distance_mul = v),
        "shadowDistanceRenderMul" => Handler::Float(|s: &mut S, v| s.shadow.distance_render_mul = v),
        "shadowIntervalSize" => Handler::Float(|s: &mut S, v| s.shadow.interval_size = v),
        "shadowHardwareFiltering" => Handler::Bool(|s: &mut S, v| s.shadow.hardware_filtering = [v, v]),
        "generateShadowMipmap" => Handler::Bool(|s: &mut S, v| s.shadow.mipmap = [v, v]),
        "shadowtexMipmap" => Handler::IndexedBool(|s: &mut S, i, v| s.shadow.mipmap[i as usize] = v, 0),
        "shadowtexNearest" => Handler::IndexedBool(|s: &mut S, i, v| s.shadow.nearest[i as usize] = v, 0),
        "generateShadowColorMipmap" => Handler::Bool(|s: &mut S, v| s.shadow.color_mipmap.iter_mut().for_each(|m| *m = v)),
        "noiseTextureResolution" => Handler::Int(|s: &mut S, v| s.noise_texture_resolution = v.clamp(1, 8192) as u32),
        "sunPathRotation" => Handler::Float(|s: &mut S, v| s.sun_path_rotation = v),
        "ambientOcclusionLevel" => Handler::Float(|s: &mut S, v| s.ambient_occlusion_level = v.clamp(0.0, 1.0)),
        "wetnessHalflife" => Handler::Float(|s: &mut S, v| s.wetness_half_life = v),
        // Iris bug, mirrored for compatibility: `drynessHalflife` sets the wetness half-life
        // (`PackDirectives.acceptDirectivesFrom`), so packs tuned on Iris look the same.
        "drynessHalflife" => Handler::Float(|s: &mut S, v| s.wetness_half_life = v),
        "eyeBrightnessHalflife" => Handler::Float(|s: &mut S, v| s.eye_brightness_half_life = v),
        "centerDepthHalflife" => Handler::Float(|s: &mut S, v| s.center_depth_half_life = v),
        _ => return indexed_handler(key),
    };
    Some(h)
}

/// `shadowHardwareFilteringN`, `shadowtexNMipmap`, `shadowtexNNearest`, `shadowNMinMagNearest`
/// (N < 2) and `shadowcolorNMipmap`, `shadowColorNMipmap`, `shadowcolorNNearest`,
/// `shadowColorNNearest`, `shadowColorNMinMagNearest` (N < 8).
fn indexed_handler(key: &str) -> Option<Handler> {
    type S = PackDirectiveState;
    let digit = |s: &str, max: u32| -> Option<u32> { s.parse::<u32>().ok().filter(|i| *i < max && s.len() == 1) };
    if let Some(n) = key.strip_prefix("shadowHardwareFiltering").and_then(|s| digit(s, 2)) {
        return Some(Handler::IndexedBool(|s: &mut S, i, v| s.shadow.hardware_filtering[i as usize] = v, n));
    }
    if let Some(rest) = key.strip_prefix("shadowtex") {
        if let Some(n) = rest.strip_suffix("Mipmap").and_then(|s| digit(s, 2)) {
            return Some(Handler::IndexedBool(|s: &mut S, i, v| s.shadow.mipmap[i as usize] = v, n));
        }
        if let Some(n) = rest.strip_suffix("Nearest").and_then(|s| digit(s, 2)) {
            return Some(Handler::IndexedBool(|s: &mut S, i, v| s.shadow.nearest[i as usize] = v, n));
        }
    }
    if let Some(n) = key.strip_prefix("shadow").and_then(|r| r.strip_suffix("MinMagNearest")).and_then(|s| digit(s, 2)) {
        return Some(Handler::IndexedBool(|s: &mut S, i, v| s.shadow.nearest[i as usize] = v, n));
    }
    for prefix in ["shadowcolor", "shadowColor"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            if let Some(n) = rest.strip_suffix("Mipmap").and_then(|s| digit(s, MAX_SHADOWCOLOR)) {
                return Some(Handler::IndexedBool(|s: &mut S, i, v| set_vec(&mut s.shadow.color_mipmap, i, v), n));
            }
            if let Some(n) = rest.strip_suffix("Nearest").and_then(|s| digit(s, MAX_SHADOWCOLOR)) {
                // `shadowcolorNMinMagNearest` is not an Iris key; `shadowColorNMinMagNearest` is.
                if prefix == "shadowcolor" && rest.ends_with("MinMagNearest") {
                    return None;
                }
                return Some(Handler::IndexedBool(|s: &mut S, i, v| set_vec(&mut s.shadow.color_nearest, i, v), n));
            }
            if prefix == "shadowColor"
                && let Some(n) = rest.strip_suffix("MinMagNearest").and_then(|s| digit(s, MAX_SHADOWCOLOR))
            {
                return Some(Handler::IndexedBool(|s: &mut S, i, v| set_vec(&mut s.shadow.color_nearest, i, v), n));
            }
        }
    }
    None
}

fn set_vec(v: &mut Vec<bool>, i: u32, value: bool) {
    let i = i as usize;
    if v.len() <= i {
        v.resize(i + 1, false);
    }
    v[i] = value;
}

impl PackDirectiveState {
    /// Apply one directive (later directives override earlier ones). Unrecognized keys
    /// are ignored (they are ordinary constants); recognized keys with the wrong type or
    /// an unparsable value produce a warning located at `file:line`.
    pub fn apply(&mut self, d: &ConstDirective, file: &str, diags: &mut Diagnostics) {
        let Some(h) = handler_for(&d.key) else { return };
        let loc = sb_core::SourceLocation::new(file, d.line);
        let bad = |diags: &mut Diagnostics, msg: String| {
            diags.push(Diagnostic::warning("dir.const", msg).at(loc.clone()));
        };
        if h.expected() != d.ty {
            bad(diags, format!("const `{}` is ignored: a {:?} directive is expected", d.key, h.expected()));
            return;
        }
        match h {
            Handler::Format { shadow, index } => match TextureFormat::parse(&d.value) {
                Some(f) => self.buffer(shadow, index).format = Some(f),
                None => bad(diags, format!("`{}`: unknown texture format `{}`; ignored", d.key, d.value)),
            },
            Handler::Clear { shadow, index } => match d.value.as_str() {
                "true" => self.buffer(shadow, index).clear = Some(true),
                "false" => self.buffer(shadow, index).clear = Some(false),
                _ => bad(diags, format!("`{}`: `{}` is not a boolean", d.key, d.value)),
            },
            Handler::ClearColor { shadow, index } => match parse_floats::<4>(&d.value, "vec4") {
                Some(c) => self.buffer(shadow, index).clear_color = Some(c),
                None => bad(diags, format!("`{}`: `{}` is not a vec4 constructor", d.key, d.value)),
            },
            Handler::Int(f) => match parse_int(&d.value) {
                Some(v) => {
                    f(self, v);
                    self.shadow_explicit.insert(d.key.clone());
                }
                None => bad(diags, format!("`{}`: `{}` is not an integer", d.key, d.value)),
            },
            Handler::Float(f) => match parse_float(&d.value) {
                Some(v) => {
                    f(self, v);
                    self.shadow_explicit.insert(d.key.clone());
                }
                None => bad(diags, format!("`{}`: `{}` is not a float", d.key, d.value)),
            },
            Handler::Bool(f) => match d.value.as_str() {
                "true" => f(self, true),
                "false" => f(self, false),
                _ => bad(diags, format!("`{}`: `{}` is not a boolean", d.key, d.value)),
            },
            Handler::IndexedBool(f, i) => match d.value.as_str() {
                "true" => f(self, i, true),
                "false" => f(self, i, false),
                _ => bad(diags, format!("`{}`: `{}` is not a boolean", d.key, d.value)),
            },
        }
    }

    fn buffer(&mut self, shadow: bool, index: u32) -> &mut BufferDirectives {
        if shadow { self.shadowcolor.entry(index).or_default() } else { self.colortex.entry(index).or_default() }
    }
}

/// `<buf>MipmapEnabled` of one program (colortex indices), from its fragment source.
pub fn mipmapped_buffers(fragment: &str) -> Vec<u32> {
    mipmapped_buffers_in(&find_const_directives(fragment))
}

/// [`mipmapped_buffers`] from the [`find_const_directives`] of the fragment source.
pub fn mipmapped_buffers_in(consts: &[ConstDirective]) -> Vec<u32> {
    let mut set = BTreeSet::new();
    for d in consts {
        if d.ty != ConstType::Bool {
            continue;
        }
        let Some(buf) = d.key.strip_suffix("MipmapEnabled") else { continue };
        let Some((false, index)) = buffer_of(buf) else { continue };
        match d.value.as_str() {
            "true" => {
                set.insert(index);
            }
            "false" => {
                set.remove(&index);
            }
            _ => {}
        }
    }
    set.into_iter().collect()
}

/// `workGroups` / `workGroupsRender` of a compute source (the last valid one wins;
/// default `workGroupsRender = vec2(1, 1)`).
pub fn compute_work_groups(source: &str, diags: &mut Diagnostics, file: &str) -> WorkGroups {
    compute_work_groups_in(&find_const_directives(source), diags, file)
}

/// [`compute_work_groups`] from the [`find_const_directives`] of the compute source.
pub fn compute_work_groups_in(consts: &[ConstDirective], diags: &mut Diagnostics, file: &str) -> WorkGroups {
    let mut wg = WorkGroups::Relative { x: 1.0, y: 1.0 };
    for d in consts {
        let loc = sb_core::SourceLocation::new(file, d.line);
        match (d.ty, d.key.as_str()) {
            (ConstType::Ivec3, "workGroups") => {
                let parsed = constructor_args(&d.value, "ivec3").and_then(|a| {
                    let v: Option<Vec<u32>> =
                        a.iter().map(|x| parse_int(x).and_then(|i| u32::try_from(i).ok())).collect();
                    v.filter(|v| v.len() == 3)
                });
                match parsed {
                    Some(v) => wg = WorkGroups::Absolute { x: v[0], y: v[1], z: v[2] },
                    None => diags.push(
                        Diagnostic::warning("dir.work-groups", format!("`workGroups = {}` is not a valid ivec3", d.value))
                            .at(loc),
                    ),
                }
            }
            (ConstType::Vec2, "workGroupsRender") => match parse_floats::<2>(&d.value, "vec2") {
                Some([x, y]) => wg = WorkGroups::Relative { x, y },
                None => diags.push(
                    Diagnostic::warning("dir.work-groups", format!("`workGroupsRender = {}` is not a valid vec2", d.value))
                        .at(loc),
                ),
            },
            _ => {}
        }
    }
    wg
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn db(src: &str) -> Option<Vec<u32>> {
        parse_draw_buffers(src, &mut Diagnostics::new())
    }

    #[test]
    fn comment_directive_forms() {
        // Iris's own CommentDirectiveParser test cases.
        let f = |s: &str| find_comment_directive(s, "DRAWBUFFERS").map(|d| d.value);
        assert_eq!(f("Some normal text that doesn't contain a DRAWBUFFERS directive"), None);
        assert_eq!(f("text with a /* DRAWBUFFERS: directive of any sort"), None);
        assert_eq!(f("/*DRAWBUFFERS:321*/ OptiFine detects this"), Some("321".into()));
        assert_eq!(f("A line: /* DRAWBUFFERS:321 */"), Some("321".into()));
        assert_eq!(f("This is a line /* DRAWBUFFERS:31 */ containing"), Some("31".into()));
        assert_eq!(f("/* DRAWBUFFERS: */ empty"), Some("".into()));
        assert_eq!(f("    /*DRAWBUFFERS:12*/"), Some("12".into()));
        let multi = "/* RENDERTARGETS:Duplicate? */\nuniform sampler2D test;\n/* RENDERTARGETS:Dup in line? */ x /* RENDERTARGETS:It works */\n";
        assert_eq!(find_comment_directive(multi, "RENDERTARGETS").map(|d| d.value), Some("It works".into()));
        // The last occurrence is not a comment directive: nothing is found (Iris).
        assert_eq!(f("/* DRAWBUFFERS:01 */\n// DRAWBUFFERS:2\n"), None);
        // Prefix inside a comment that started earlier on the line is accepted only if `/*` directly precedes.
        assert_eq!(f("/* note DRAWBUFFERS:01 */"), None);
    }

    #[test]
    fn draw_buffers_last_wins() {
        assert_eq!(db("void main(){}"), None);
        assert_eq!(db("/* DRAWBUFFERS:0257 */"), Some(vec![0, 2, 5, 7]));
        assert_eq!(db("/* RENDERTARGETS: 0,2,11,15 */"), Some(vec![0, 2, 11, 15]));
        assert_eq!(db("/* RENDERTARGETS: 0, 2 */"), Some(vec![0, 2]));
        // Both: the later one wins.
        assert_eq!(db("/* RENDERTARGETS: 3,4 */\n/* DRAWBUFFERS:01 */"), Some(vec![0, 1]));
        assert_eq!(db("/* DRAWBUFFERS:01 */\n/* RENDERTARGETS: 3,4 */"), Some(vec![3, 4]));
        // Last occurrence of each kind.
        assert_eq!(db("/* DRAWBUFFERS:01 */\n/* DRAWBUFFERS:7 */"), Some(vec![7]));
        // Malformed entries are dropped with a warning.
        let mut d = Diagnostics::new();
        assert_eq!(parse_draw_buffers("/* RENDERTARGETS: 1,x,3 */", &mut d), Some(vec![1, 3]));
        assert_eq!(d.len(), 1);
        let mut d = Diagnostics::new();
        assert_eq!(parse_draw_buffers("/* DRAWBUFFERS:0a1 */", &mut d), Some(vec![0, 1]));
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn const_line_grammar() {
        assert_eq!(
            parse_const_line("  const int colortex0Format = RGBA16F; // comment"),
            Some((ConstType::Int, "colortex0Format".into(), "RGBA16F".into()))
        );
        assert_eq!(
            parse_const_line("const vec4 colortex1ClearColor = vec4(1.0, 0.0, 0.0, 1.0);"),
            Some((ConstType::Vec4, "colortex1ClearColor".into(), "vec4(1.0, 0.0, 0.0, 1.0)".into()))
        );
        // Inside block comments (matched per line regardless of comments).
        assert_eq!(
            parse_const_line("const bool gaux1Clear = false;"),
            Some((ConstType::Bool, "gaux1Clear".into(), "false".into()))
        );
        assert_eq!(parse_const_line("// const int shadowMapResolution = 2048;"), None);
        assert_eq!(parse_const_line("constint x = 1;"), None);
        assert_eq!(parse_const_line("const uint x = 1;"), None);
        assert_eq!(parse_const_line("const float a-b = 1;"), None);
        assert_eq!(parse_const_line("const float sunPathRotation = -40.0"), None);
        assert_eq!(
            parse_const_line("const ivec3 workGroups = ivec3(4, 2, 1);"),
            Some((ConstType::Ivec3, "workGroups".into(), "ivec3(4, 2, 1)".into()))
        );
        let src = "/*\nconst int colortex2Format = RGB16;\n*/\nconst float sunPathRotation = -30.0;\n";
        let all = find_const_directives(src);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 2);
        assert_eq!(all[1].key, "sunPathRotation");
    }

    #[test]
    fn pack_directives_apply() {
        let src = "\
const int colortex0Format = RGBA16F;
const int gaux1Format = R11F_G11F_B10F;
const bool colortex2Clear = false;
const vec4 colortex3ClearColor = vec4(1.0, 0.5, 0.25, 1.0);
const int shadowcolor1Format = RG16;
const int shadowMapResolution = 2048;
const float shadowDistance = 128.0f;
const bool shadowHardwareFiltering = true;
const bool shadowHardwareFiltering1 = false;
const bool shadowtex0Nearest = true;
const bool shadowColor2Mipmap = true;
const float sunPathRotation = -40.0;
const float ambientOcclusionLevel = 2.0;
const float wetnessHalflife = 300.0;
const float drynessHalflife = 50.0;
const int noiseTextureResolution = 512;
const float shadowMapFov = 90.0;
const float shadowDistance = 160.0;
const int colortex5Format = NOT_A_FORMAT;
const float shadowIntervalSize = abc;
const bool colortex4Clear = 1;
const float colortex6Format = 1.0;
const int myOwnConstant = 5;
";
        let mut s = PackDirectiveState::default();
        let mut diags = Diagnostics::new();
        for d in find_const_directives(src) {
            s.apply(&d, "composite.fsh", &mut diags);
        }
        assert_eq!(s.colortex[&0].format, Some(TextureFormat::RGBA16F));
        assert_eq!(s.colortex[&4].format, Some(TextureFormat::R11F_G11F_B10F));
        assert_eq!(s.colortex[&2].clear, Some(false));
        assert_eq!(s.colortex[&3].clear_color, Some([1.0, 0.5, 0.25, 1.0]));
        assert_eq!(s.shadowcolor[&1].format, Some(TextureFormat::RG16));
        assert_eq!(s.shadow.resolution, 2048);
        assert_eq!(s.shadow.distance, 160.0); // last wins
        assert_eq!(s.shadow.hardware_filtering, [true, false]);
        assert_eq!(s.shadow.nearest, [true, false]);
        assert!(s.shadow.color_mipmap[2]);
        assert_eq!(s.shadow.fov, Some(90.0));
        assert_eq!(s.sun_path_rotation, -40.0);
        assert_eq!(s.ambient_occlusion_level, 1.0); // clamped
        assert_eq!(s.wetness_half_life, 50.0); // Iris: drynessHalflife sets wetness
        assert_eq!(s.dryness_half_life, 200.0);
        assert_eq!(s.noise_texture_resolution, 512);
        assert!(!s.colortex.contains_key(&5));
        // Bad format, bad float, bad bool, wrong type: 4 warnings; unknown key: none.
        assert_eq!(diags.len(), 4, "{diags:?}");
        assert!(diags.iter().all(|d| d.location.as_ref().is_some_and(|l| l.file == "composite.fsh")));
    }

    #[test]
    fn mipmap_and_work_groups() {
        let fsh = "const bool colortex3MipmapEnabled = true;\nconst bool gaux2MipmapEnabled = true;\nconst bool colortex3MipmapEnabled = false;\nconst bool colortex9MipmapEnabled = true;";
        assert_eq!(mipmapped_buffers(fsh), vec![5, 9]);
        let mut d = Diagnostics::new();
        assert_eq!(compute_work_groups("void main(){}", &mut d, "a.csh"), WorkGroups::Relative { x: 1.0, y: 1.0 });
        assert_eq!(
            compute_work_groups("const ivec3 workGroups = ivec3(8, 4, 1);", &mut d, "a.csh"),
            WorkGroups::Absolute { x: 8, y: 4, z: 1 }
        );
        assert_eq!(
            compute_work_groups("const vec2 workGroupsRender = vec2(0.5, 0.25);", &mut d, "a.csh"),
            WorkGroups::Relative { x: 0.5, y: 0.25 }
        );
        assert!(d.is_empty());
        compute_work_groups("const ivec3 workGroups = ivec3(8, -4);", &mut d, "a.csh");
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn buffer_names() {
        assert_eq!(buffer_of("gcolor"), Some((false, 0)));
        assert_eq!(buffer_of("gaux4"), Some((false, 7)));
        assert_eq!(buffer_of("colortex31"), Some((false, 31)));
        assert_eq!(buffer_of("colortex32"), None);
        assert_eq!(buffer_of("colortex01"), None);
        assert_eq!(buffer_of("shadowcolor7"), Some((true, 7)));
        assert_eq!(buffer_of("shadowcolor8"), None);
        assert!(parse_float("1.0f").is_some() && parse_float("1e3").is_some() && parse_float("abc").is_none());
        assert_eq!(parse_int("+12"), Some(12));
        assert_eq!(parse_int("1.0"), None);
    }
}
