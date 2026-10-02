//! Integration tests over the real shader pack corpus.
//!
//! The corpus location is `$SB_CORPUS_DIR` or the session scratchpad default; every
//! test is skipped (passes with a note) when it is missing. Run with `--nocapture` to
//! see the reports.
//!
//! The scan is textual (comments stripped, no preprocessing), so it sees the union of
//! every `#ifdef` branch: a strict superset of what any configuration compiles.

mod common;

use regex::Regex;
use sb_core::GlslType;
use sb_core::model::{ResourceKind, ResourceRef};
use sb_core::program::ProgramName;
use sb_uniforms::{
    BindingTableBuilder, CUSTOM_STAGE, LayoutBuilder, ProgramClass, ResourceContext, SAMPLER_SET,
    STORAGE_SET, UniformDecl, canonicalize_with_kind, custom_uniform_input_type, find_binding,
    image_kind, is_opaque_type, registry, sampler_kind, validate_block,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const DEFAULT_CORPUS: &str = "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus";

/// Pack directories; MinecraftShaderProgramming (several tutorial packs) counts as one.
const PACKS: &[&str] = &[
    "photon",
    "Bliss-Shader",
    "ComplementaryReimagined",
    "Super-Duper-Vanilla",
    "glimmer-shaders",
    "spectrum",
    "Ominous-Shaderpack",
    "MinecraftShaderProgramming",
];

/// Loose uniforms declared by several packs that are known not to be OptiFine/Iris
/// builtins (none so far).
const SHARED_CUSTOM_NAMES: &[&str] = &[];

/// Custom-uniform inputs that are not OptiFine/Iris builtins: Voxy's
/// `vxRenderDistance` (glimmer, behind `#ifdef VOXY`).
const FOREIGN_CUSTOM_INPUTS: &[&str] = &["vxRenderDistance"];

fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("SB_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CORPUS));
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("corpus not found at {}; skipping", dir.display());
        None
    }
}

#[derive(Debug, Clone)]
struct OpaqueDecl {
    ty: String,
    format: Option<String>,
    readonly: bool,
    writeonly: bool,
    /// (class, texture stage) of every program whose source contains the declaration
    /// (directly or through `#include`); empty for orphan include files, which are
    /// checked in both a gbuffers and a fullscreen context.
    programs: Vec<(ProgramClass, &'static str)>,
}

/// Class and texture stage of a program file (`gbuffers_water.fsh`,
/// `world0/composite3_a.csh`), given its path relative to the `shaders/` root.
fn file_program(rel: &str) -> Option<(ProgramClass, &'static str)> {
    let path = Path::new(rel);
    if rel.matches('/').count() > 1 {
        return None; // programs live in the root or in a world folder
    }
    let ext = path.extension()?.to_str()?;
    if !matches!(ext, "vsh" | "fsh" | "gsh" | "csh" | "tcs" | "tes") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let compute = ext == "csh";
    let (base, _) = ProgramName::split_compute_letter(stem);
    Some(match ProgramName::parse(base)? {
        ProgramName::Geometry { program } => (ProgramClass::from_geometry(program), "gbuffers"),
        ProgramName::Composite { group, .. } => (
            if compute {
                ProgramClass::Compute
            } else {
                ProgramClass::Fullscreen
            },
            group.texture_stage(),
        ),
    })
}

#[derive(Debug, Default)]
struct PackScan {
    name: String,
    files: usize,
    /// loose uniform name → declared types (with array suffix, e.g. `vec3[4]`)
    loose: BTreeMap<String, BTreeSet<String>>,
    /// opaque uniform name → declarations
    opaque: BTreeMap<String, Vec<OpaqueDecl>>,
    /// `shaders.properties` files (one per pack directory)
    properties: Vec<String>,
}

const GLSL_EXTENSIONS: &[&str] = &["glsl", "vsh", "fsh", "gsh", "csh", "tcs", "tes", "inc"];

/// Remove `//` and `/* */` comments, keeping line structure.
fn strip_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            out.push_str(&src[last..i]);
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            last = i;
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            out.push_str(&src[last..i]);
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(b.len());
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&src[last.min(src.len())..]);
    out
}

static UNIFORM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\buniform\s+((?:layout\s*\([^)]*\)\s*|(?:lowp|mediump|highp|readonly|writeonly|restrict|coherent|volatile|flat)\s+)*)([A-Za-z_]\w*)\s+([^;{}]*);",
    )
    .unwrap()
});
static DECLARATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([A-Za-z_]\w*)\s*(\[[^\]]*\])?").unwrap());
static FORMAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(rgba32f|rgba16f|rg32f|rg16f|r11f_g11f_b10f|r32f|r16f|rgba16|rgb10_a2|rgba8|rg16|rg8|r16|r8|rgba16_snorm|rgba8_snorm|rg16_snorm|rg8_snorm|r16_snorm|r8_snorm|rgba32i|rgba16i|rgba8i|rg32i|rg16i|rg8i|r32i|r16i|r8i|rgba32ui|rgba16ui|rgb10_a2ui|rgba8ui|rg32ui|rg16ui|rg8ui|r32ui|r16ui|r8ui)\b").unwrap()
});

/// Split a declarator list on commas outside parentheses/brackets.
fn split_declarators(list: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in list.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    parts
}

fn scan_source(src: &str, programs: &[(ProgramClass, &'static str)], pack: &mut PackScan) {
    // `src` is already comment-free.
    for cap in UNIFORM_RE.captures_iter(src) {
        let qualifiers = &cap[1];
        let ty = &cap[2];
        for decl in split_declarators(&cap[3]) {
            let Some(d) = DECLARATOR_RE.captures(decl) else {
                continue;
            };
            let name = d[1].to_string();
            if is_opaque_type(ty) {
                pack.opaque.entry(name).or_default().push(OpaqueDecl {
                    ty: ty.to_string(),
                    format: FORMAT_RE.captures(qualifiers).map(|f| f[1].to_string()),
                    readonly: qualifiers.contains("readonly"),
                    writeonly: qualifiers.contains("writeonly"),
                    programs: programs.to_vec(),
                });
            } else if GlslType::parse(ty).is_some() {
                let array = d
                    .get(2)
                    .map(|m| m.as_str().replace(char::is_whitespace, ""))
                    .unwrap_or_default();
                pack.loose
                    .entry(name)
                    .or_default()
                    .insert(format!("{ty}{array}"));
            }
        }
    }
}

static INCLUDE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s+["<]([^">]+)[">]"#).unwrap());

/// The `shaders/` root containing `path` and the path relative to it.
fn split_root(path: &Path) -> Option<(PathBuf, String)> {
    let root = path
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "shaders"))?;
    let rel = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    Some((root.to_path_buf(), rel))
}

fn scan_pack(dir: &Path, name: &str) -> PackScan {
    let mut pack = PackScan {
        name: name.to_string(),
        ..PackScan::default()
    };
    // (shaders root, relative path) → (source, resolved includes)
    let mut sources: BTreeMap<(PathBuf, String), (String, Vec<String>)> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if GLSL_EXTENSIONS.contains(&ext) {
            let (Ok(bytes), Some((root, rel))) = (std::fs::read(path), split_root(path)) else {
                continue;
            };
            let src = strip_comments(&String::from_utf8_lossy(&bytes));
            let includes = INCLUDE_RE
                .captures_iter(&src)
                .filter_map(|c| sb_core::normalize_pack_path(&rel, &c[1]))
                .collect();
            sources.insert((root, rel), (src, includes));
        } else if path.file_name().is_some_and(|f| f == "shaders.properties")
            && let Ok(bytes) = std::fs::read(path)
        {
            // ISO-8859-1
            pack.properties
                .push(bytes.iter().map(|&b| char::from(b)).collect());
        }
    }
    // Propagate each program's (class, stage) through its include graph.
    let mut programs_of: BTreeMap<(PathBuf, String), BTreeSet<(ProgramClass, &'static str)>> =
        BTreeMap::new();
    for (root, rel) in sources.keys() {
        let Some(program) = file_program(rel) else {
            continue;
        };
        let mut stack = vec![rel.clone()];
        let mut seen = HashSet::new();
        while let Some(file) = stack.pop() {
            if !seen.insert(file.clone()) {
                continue;
            }
            let key = (root.clone(), file);
            if let Some((_, includes)) = sources.get(&key) {
                stack.extend(includes.iter().cloned());
                programs_of.entry(key).or_default().insert(program);
            }
        }
    }
    for (key, (src, _)) in &sources {
        pack.files += 1;
        let programs: Vec<_> = programs_of
            .get(key)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        scan_source(src, &programs, &mut pack);
    }
    pack
}

fn scan_corpus() -> Option<Vec<PackScan>> {
    let corpus = corpus_dir()?;
    let packs: Vec<PackScan> = PACKS
        .iter()
        .filter(|p| corpus.join(p).is_dir())
        .map(|p| scan_pack(&corpus.join(p), p))
        .collect();
    if packs.is_empty() {
        eprintln!("no corpus packs found; skipping");
        return None;
    }
    Some(packs)
}

/// Parse a declared loose type text (`vec3`, `vec3[4]`). Arrays with non-literal
/// lengths (macros) return `None`.
fn parse_decl_type(text: &str) -> Option<GlslType> {
    match text.split_once('[') {
        None => GlslType::parse(text),
        Some((base, rest)) => {
            let len: u32 = rest.trim_end_matches(']').parse().ok()?;
            Some(GlslType::parse(base)?.with_array(len))
        }
    }
}

/// Join `\` continuations of a properties file and return its lines.
fn property_lines(text: &str) -> Vec<String> {
    text.replace("\\\r\n", "")
        .replace("\\\n", "")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Strip the `.0`-`.9` suffix Iris removes from texture names.
fn texture_name(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((base, suffix)) if suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit() => base,
        _ => name,
    }
}

/// The resource context of a program of `class` whose texture stage is `stage`
/// (`None` for include files: only `customTexture.*` and images apply).
fn context_for(pack: &PackScan, class: ProgramClass, stage: Option<&str>) -> ResourceContext {
    let mut ctx = ResourceContext::new(class);
    for text in &pack.properties {
        for line in property_lines(text) {
            let line = line.trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if let Some(name) = key.strip_prefix("customTexture.") {
                ctx = ctx.with_custom_texture(CUSTOM_STAGE, texture_name(name));
            } else if let Some(rest) = key.strip_prefix("texture.") {
                let Some((s, name)) = rest.split_once('.') else {
                    continue;
                };
                if Some(s) != stage {
                    continue;
                }
                // Iris (ShaderProperties): a multi-part value is a raw texture whose type
                // is TEXTURE_1D for 6 parts, parts[1] for 7 and TEXTURE_3D for 8. Raw
                // textures rename matching declarations instead of overriding the group.
                let parts: Vec<&str> = value.split_whitespace().collect();
                let raw_type = match parts.len() {
                    0 | 1 => None,
                    6 => Some("TEXTURE_1D"),
                    7 => Some(parts[1]),
                    8 => Some("TEXTURE_3D"),
                    _ => continue, // Iris warns and ignores it
                };
                match raw_type {
                    None => ctx = ctx.with_custom_texture(s, texture_name(name)),
                    Some(t) => {
                        let dim = sb_uniforms::raw_texture_dim(t)
                            .unwrap_or_else(|| panic!("{}: bad raw texture type {t}", pack.name));
                        ctx = ctx.with_raw_texture(s, texture_name(name), dim);
                    }
                }
            } else if let Some(image) = key.strip_prefix("image.") {
                ctx = ctx.with_custom_image(image, value.split_whitespace().next());
            }
        }
    }
    ctx.detect_watershadow(pack.opaque.keys().map(String::as_str))
}

/// name → packs declaring it
fn by_pack_count<'a, V: 'a>(
    packs: &'a [PackScan],
    f: impl Fn(&'a PackScan) -> &'a BTreeMap<String, V>,
) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let mut out: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for p in packs {
        for name in f(p).keys() {
            out.entry(name.as_str())
                .or_default()
                .insert(p.name.as_str());
        }
    }
    out
}

#[test]
fn loose_uniforms_used_by_three_packs_are_registered() {
    let Some(packs) = scan_corpus() else { return };
    let total: usize = packs.iter().map(|p| p.loose.len()).sum();
    let files: usize = packs.iter().map(|p| p.files).sum();
    eprintln!(
        "scanned {files} GLSL files in {} packs: {total} distinct (pack, loose uniform) pairs",
        packs.len()
    );
    assert!(
        files > 500 && total > 200,
        "the scan found too little ({files} files, {total} uniforms)"
    );

    let usage = by_pack_count(&packs, |p| &p.loose);
    let spec: HashSet<&str> = common::SPEC_SECTION_3.iter().map(|(n, _)| *n).collect();

    eprintln!("\nloose uniforms NOT in the registry (packs using them):");
    let mut missing_shared = Vec::new();
    for (name, users) in &usage {
        if registry::is_builtin(name) {
            continue;
        }
        if users.len() >= 2 {
            eprintln!(
                "  {:2} {name}: {}",
                users.len(),
                users.iter().copied().collect::<Vec<_>>().join(", ")
            );
        }
        if users.len() >= 3 && !SHARED_CUSTOM_NAMES.contains(name) {
            missing_shared.push(*name);
        }
        assert!(
            !(users.len() >= 3 && spec.contains(name)),
            "spec builtin `{name}` used by {} packs is not registered",
            users.len()
        );
    }
    let single = usage
        .iter()
        .filter(|(n, u)| !registry::is_builtin(n) && u.len() == 1)
        .count();
    eprintln!("  ... plus {single} names used by a single pack (pack-specific custom uniforms)");
    assert!(
        missing_shared.is_empty(),
        "names used by >= 3 packs but not registered: {missing_shared:?}"
    );

    let registered: BTreeSet<&str> = usage
        .keys()
        .copied()
        .filter(|n| registry::is_builtin(n))
        .collect();
    eprintln!(
        "\n{} registry builtins are declared by the corpus",
        registered.len()
    );
}

#[test]
fn declared_builtin_types_match_the_registry() {
    let Some(packs) = scan_corpus() else { return };
    // builtin → (packs with a compatible type, packs with an incompatible type + types)
    let mut good: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let mut bad: BTreeMap<&str, BTreeSet<(String, &str)>> = BTreeMap::new();
    for p in &packs {
        for (name, types) in &p.loose {
            let Some(b) = registry::get(name) else {
                continue;
            };
            for t in types {
                let Some(declared) = parse_decl_type(t) else {
                    continue;
                };
                if registry::is_type_compatible(b.ty, declared) {
                    good.entry(b.name).or_default().insert(&p.name);
                } else {
                    bad.entry(b.name).or_default().insert((t.clone(), &p.name));
                }
            }
        }
    }
    eprintln!("builtin declarations whose type differs from Iris (zero-filled, as in Iris):");
    for (name, decls) in &bad {
        let list: Vec<String> = decls.iter().map(|(t, p)| format!("{t} in {p}")).collect();
        eprintln!(
            "  {name} ({}): {}",
            registry::get(name).unwrap().ty,
            list.join(", ")
        );
        let wrong_packs: BTreeSet<&str> = decls.iter().map(|(_, p)| *p).collect();
        let right = good.get(name).map_or(0, BTreeSet::len);
        assert!(
            right >= wrong_packs.len(),
            "`{name}`: {right} packs use the registry type, {} do not",
            wrong_packs.len()
        );
    }
}

#[test]
fn pack_layouts_are_valid() {
    let Some(packs) = scan_corpus() else { return };
    for p in &packs {
        let mut builder = LayoutBuilder::new();
        let mut skipped = 0;
        for (name, types) in &p.loose {
            for t in types {
                match parse_decl_type(t) {
                    Some(ty) => builder.add(
                        UniformDecl::from_registry(name.clone(), ty).in_program(p.name.clone()),
                    ),
                    None => skipped += 1,
                }
            }
        }
        let (layout, index, diags) = builder.build();
        for block in [&layout.frame, &layout.draw] {
            let d = validate_block(block);
            assert!(d.is_empty(), "{}: {d:?}", p.name);
            assert!(
                block.size <= sb_uniforms::layout::PORTABLE_MAX_BLOCK_SIZE,
                "{}: {} is {} bytes",
                p.name,
                block.name,
                block.size
            );
        }
        assert!(
            !diags.has_errors(),
            "{}: {:?}",
            p.name,
            diags.errors().collect::<Vec<_>>()
        );
        // Every declaration can be found through the index.
        for (name, types) in &p.loose {
            for ty in types.iter().filter_map(|t| parse_decl_type(t)) {
                let m = index
                    .get(name, ty)
                    .unwrap_or_else(|| panic!("{}: {name} {ty} not indexed", p.name));
                let block = if m.block == registry::Frequency::Frame {
                    &layout.frame
                } else {
                    &layout.draw
                };
                assert_eq!(block.member(&m.member).map(|b| b.offset), Some(m.offset));
            }
        }
        let unset = layout
            .frame
            .members
            .iter()
            .filter(|m| m.source == sb_core::model::UniformSource::Unset)
            .count();
        let conflicts = diags
            .iter()
            .filter(|d| d.code == "uniform.type-conflict")
            .count();
        eprintln!(
            "{:28} sb_Frame {:5} B / {:3} members ({unset} unset), sb_Draw {:4} B / {:2} members, {conflicts} type conflicts, {skipped} macro-sized arrays skipped",
            p.name,
            layout.frame.size,
            layout.frame.members.len(),
            layout.draw.size,
            layout.draw.members.len()
        );
    }
}

#[test]
fn samplers_and_images_canonicalize_and_bind() {
    let Some(packs) = scan_corpus() else { return };
    let usage = by_pack_count(&packs, |p| &p.opaque);
    let mut resolved_somewhere: HashSet<String> = HashSet::new();
    for p in &packs {
        let mut builder = BindingTableBuilder::new();
        let mut cache: HashMap<(ProgramClass, Option<&str>), ResourceContext> = HashMap::new();
        let mut ctx_for = |class: ProgramClass, stage: Option<&'static str>| -> ResourceContext {
            cache
                .entry((class, stage))
                .or_insert_with(|| context_for(p, class, stage))
                .clone()
        };
        let mut declared = Vec::new();
        let mut unknown = BTreeSet::new();
        for (name, decls) in &p.opaque {
            for d in decls {
                let kind = sampler_kind(&d.ty)
                    .or_else(|| image_kind(&d.ty, d.format.as_deref(), d.readonly, d.writeonly))
                    .unwrap_or_else(|| panic!("{}: unparsed opaque type {}", p.name, d.ty));
                let ctxs: Vec<ResourceContext> = if d.programs.is_empty() {
                    vec![
                        ctx_for(ProgramClass::Gbuffers, None),
                        ctx_for(ProgramClass::Fullscreen, None),
                    ]
                } else {
                    d.programs
                        .iter()
                        .map(|&(class, stage)| ctx_for(class, Some(stage)))
                        .collect()
                };
                for ctx in &ctxs {
                    let c = canonicalize_with_kind(name, &kind, ctx);
                    match &c.resource {
                        ResourceRef::Unknown(_) => {
                            unknown.insert(name.clone());
                        }
                        _ => {
                            resolved_somewhere.insert(name.clone());
                        }
                    }
                    let binding = builder.add_canonical(&c, kind.clone());
                    declared.push((c, kind.clone(), binding));
                }
            }
        }
        let (table, diags) = builder.finish();
        assert!(!diags.has_errors(), "{}: {diags:?}", p.name);
        // Names are unique; sets follow the kind; non-SSBO bindings are unique per set.
        let mut names = HashSet::new();
        let mut slots = HashSet::new();
        for e in &table.entries {
            assert!(
                names.insert(e.name.as_str()),
                "{}: duplicate binding name {}",
                p.name,
                e.name
            );
            match e.kind {
                ResourceKind::Sampler { .. } => assert_eq!(e.set, SAMPLER_SET),
                ResourceKind::StorageImage { .. } => assert_eq!(e.set, STORAGE_SET),
                _ => {}
            }
            if e.kind != ResourceKind::StorageBuffer {
                assert!(
                    slots.insert((e.set, e.binding)),
                    "{}: binding ({}, {}) reused",
                    p.name,
                    e.set,
                    e.binding
                );
            }
        }
        for (c, kind, binding) in &declared {
            let e = find_binding(&table, c, kind)
                .unwrap_or_else(|| panic!("{}: no binding for {c:?}", p.name));
            assert_eq!(&e.name, binding);
        }
        // Custom textures never need conflict variants: a raw stage texture is selected
        // by the declared type, so its binding always has the raw texture's dimension
        // (regression: photon's `sampler2D colortex0` scene-color reads were bound to the
        // raw 3D `texture.composite.colortex0` noise).
        for e in &table.entries {
            let ResourceRef::CustomTexture(id) = &e.resource else {
                continue;
            };
            assert!(
                !e.name.contains("__"),
                "{}: custom texture {id} needs a conflict variant {}",
                p.name,
                e.name
            );
            if let Some((_, _, Some(dim))) = sb_uniforms::parse_custom_texture_id(id) {
                assert!(
                    matches!(&e.kind, ResourceKind::Sampler { dim: d, .. } if d == dim),
                    "{}: raw texture {id} bound as {:?}",
                    p.name,
                    e.kind
                );
            }
        }
        if p.name == "photon" {
            let scene = table.get("colortex0").expect("photon reads colortex0");
            assert_eq!(scene.resource, ResourceRef::ColorTex(0));
            let noise = table
                .get("sb_tex_composite_colortex0_3d")
                .expect("photon's raw 3D composite colortex0");
            assert_eq!(
                noise.resource,
                ResourceRef::CustomTexture("composite.colortex0.3d".into())
            );
        }
        let conflicts: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        eprintln!(
            "{:28} {:3} bindings ({} samplers), unknown samplers: {:?}",
            p.name,
            table.entries.len(),
            table
                .entries
                .iter()
                .filter(|e| e.set == SAMPLER_SET)
                .count(),
            unknown
        );
        for c in conflicts {
            eprintln!("    {c}");
        }
    }
    for (name, users) in &usage {
        if users.len() >= 3 {
            assert!(
                resolved_somewhere.contains(*name),
                "opaque uniform `{name}` used by {} packs is never resolved",
                users.len()
            );
        }
    }
}

#[test]
fn custom_uniform_inputs_are_known() {
    let Some(packs) = scan_corpus() else { return };
    let def_re = Regex::new(r"^\s*(uniform|variable)\.(\w+)\.(\w+)\s*=(.*)$").unwrap();
    let ident_re = Regex::new(r"[A-Za-z_]\w*").unwrap();
    let member_re = Regex::new(r"\.[A-Za-z0-9_]+").unwrap();
    let directive_re = Regex::new(r"#\s*\w+").unwrap();
    let functions: HashSet<&str> = "abs acos asin atan atan2 between ceil clamp cos degrees edge equals exp exp10 exp2 floor fmod frac if ifb in inversesqrt lerp log log10 log2 max min mix pow print radians random randomInt round sign signum sin smooth sqrt tan todeg torad vec2 vec3 vec4 true false pi"
        .split_whitespace()
        .collect();
    let mut total = 0;
    for p in &packs {
        let mut defs = HashSet::new();
        let mut exprs = Vec::new();
        for text in &p.properties {
            for line in property_lines(text) {
                if let Some(c) = def_re.captures(&line) {
                    defs.insert(c[3].to_string());
                    exprs.push(c[4].to_string());
                }
            }
        }
        let mut unknown = BTreeSet::new();
        for e in &exprs {
            let e = directive_re.replace_all(e, " ");
            let e = member_re.replace_all(&e, "");
            for id in ident_re.find_iter(&e).map(|m| m.as_str()) {
                total += 1;
                let known = functions.contains(id)
                    || defs.contains(id)
                    || id.starts_with("BIOME_")
                    || id.starts_with("CAT_")
                    || id.starts_with("PPT_")
                    || custom_uniform_input_type(id).is_some();
                if !known {
                    unknown.insert(id.to_string());
                }
            }
        }
        eprintln!(
            "{:28} {:3} custom uniform definitions, unresolved inputs: {unknown:?}",
            p.name,
            exprs.len()
        );
        for id in &unknown {
            assert!(
                FOREIGN_CUSTOM_INPUTS.contains(&id.as_str()),
                "{}: custom uniform input `{id}` is not a known builtin",
                p.name
            );
        }
    }
    assert!(total > 100, "too few identifiers scanned ({total})");
}
