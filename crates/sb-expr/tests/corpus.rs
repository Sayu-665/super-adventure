//! Integration tests over the real shader pack corpus.
//!
//! The corpus location is `$SB_CORPUS_DIR` or the session scratchpad default; every
//! test is skipped (passes with a note) when it is missing. A second, larger corpus
//! (`$SB_CORPUS2_DIR`, default `corpus2` next to the first; one directory per pack,
//! each with a `shaders/shaders.properties`) is checked the same way when present.
//!
//! `shaders.properties` is scanned as raw text: every `uniform.*`/`variable.*` line
//! (from all `#if` branches) and every `*.enabled` condition is checked.

use indexmap::IndexMap;
use sb_core::model::{BlockLayout, BlockMember, CustomUniform, UniformSource};
use sb_core::{GlslType, Severity};
use sb_expr::{
    BlockInputs, CustomUniforms, Value, eval_bool, parse, read_value, standard_constants,
    write_value,
};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data/corpus");
const DEFAULT_CORPUS2: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data/corpus2");

/// (pack directory, minimum number of custom uniform/variable lines expected)
const PACKS: &[(&str, usize)] = &[
    ("photon", 60),
    ("Bliss-Shader", 60),
    ("ComplementaryReimagined", 25),
    ("Super-Duper-Vanilla", 20),
    ("glimmer-shaders", 10),
    ("spectrum", 25),
    ("Ominous-Shaderpack", 0),
];

fn dir_from_env(var: &str, default: &str) -> Option<PathBuf> {
    let dir = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default));
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("corpus not found at {}; skipping", dir.display());
        None
    }
}

fn corpus_dir() -> Option<PathBuf> {
    dir_from_env("SB_CORPUS_DIR", DEFAULT_CORPUS)
}

/// `(pack name, shaders.properties)` of every pack in the second corpus.
fn corpus2_packs() -> Vec<(String, PathBuf)> {
    let Some(dir) = dir_from_env("SB_CORPUS2_DIR", DEFAULT_CORPUS2) else {
        return Vec::new();
    };
    let mut packs: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        e.path().join("shaders/shaders.properties"),
                    )
                })
                .filter(|(_, p)| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    packs.sort();
    packs
}

/// Logical lines of a properties file: `\` continuations joined, leading
/// whitespace of continuation lines dropped (java.util.Properties rules). As in
/// Iris, which hides backslashes from its preprocessor, a `#` directive line is
/// never part of a continuation: it ends the pending logical line.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (i, raw) in text.lines().enumerate() {
        let piece = raw.trim_start();
        if !current.is_empty() && piece.starts_with('#') {
            out.push((start, std::mem::take(&mut current)));
        }
        if current.is_empty() {
            start = i + 1;
        }
        let trailing = piece.chars().rev().take_while(|&c| c == '\\').count();
        if trailing % 2 == 1 {
            current.push_str(&piece[..piece.len() - 1]);
        } else {
            current.push_str(piece);
            out.push((start, std::mem::take(&mut current)));
        }
    }
    if !current.is_empty() {
        out.push((start, current));
    }
    out
}

struct Entry {
    line: usize,
    key: String,
    value: String,
}

fn entries(path: &Path) -> Vec<Entry> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    // Properties files are ISO-8859-1.
    let text: String = bytes.iter().map(|&b| b as char).collect();
    logical_lines(&text)
        .into_iter()
        .filter(|(_, l)| !l.starts_with('#') && !l.starts_with('!'))
        .filter_map(|(line, l)| {
            let sep = l.find(['=', ':'])?;
            Some(Entry {
                line,
                key: l[..sep].trim().to_string(),
                value: l[sep + 1..].trim().to_string(),
            })
        })
        .collect()
}

fn custom_defs(entries: &[Entry]) -> Vec<(usize, CustomUniform)> {
    entries
        .iter()
        .filter_map(|e| {
            let mut parts = e.key.splitn(3, '.');
            let kind = parts.next()?;
            if kind != "uniform" && kind != "variable" {
                return None;
            }
            let ty = parts.next()?;
            let name = parts.next()?;
            let ty = GlslType::parse(ty)
                .unwrap_or_else(|| panic!("line {}: bad type in {}", e.line, e.key));
            Some((
                e.line,
                CustomUniform {
                    name: name.to_string(),
                    ty,
                    expression: e.value.clone(),
                    is_variable: kind == "variable",
                    location: None,
                },
            ))
        })
        .collect()
}

/// Types of the builtin uniforms used by the corpus expressions; anything else is
/// treated as a float input (permissive).
fn input_type(name: &str) -> Option<GlslType> {
    Some(match name {
        "sunPosition"
        | "moonPosition"
        | "upPosition"
        | "shadowLightPosition"
        | "cameraPosition"
        | "previousCameraPosition"
        | "skyColor"
        | "fogColor"
        | "eyePosition"
        | "relativeEyePosition"
        | "playerBodyVector"
        | "playerLookVector"
        | "cameraPositionFract"
        | "previousCameraPositionFract" => GlslType::VEC3,
        "eyeBrightness" | "eyeBrightnessSmooth" | "atlasSize" => GlslType::IVEC2,
        "cameraPositionInt" | "previousCameraPositionInt" => GlslType::IVEC3,
        "lightningBoltPosition" | "entityColor" => GlslType::VEC4,
        n if n.starts_with("gbuffer")
            || n.starts_with("shadowModelView")
            || n.starts_with("shadowProjection") =>
        {
            GlslType::MAT4
        }
        n if n.starts_with("dhProjection") || n.starts_with("dhPreviousProjection") => {
            GlslType::MAT4
        }
        "frameCounter"
        | "worldTime"
        | "worldDay"
        | "moonPhase"
        | "isEyeInWater"
        | "heldItemId"
        | "heldItemId2"
        | "heldBlockLightValue"
        | "heldBlockLightValue2"
        | "biome"
        | "biome_category"
        | "biome_precipitation"
        | "bossBattle"
        | "renderStage" => GlslType::INT,
        n if n.starts_with("BIOME_") => GlslType::INT,
        n if n.starts_with("is_") => GlslType::BOOL,
        "firstPersonCamera" | "isSpectator" | "isRightHanded" | "hasSkylight" => GlslType::BOOL,
        _ => GlslType::FLOAT,
    })
}

/// Pack-global std140 layout: every referenced builtin, then every output.
fn layout_for(cu: &CustomUniforms) -> BlockLayout {
    let mut members = Vec::new();
    let mut offset = 0u32;
    let mut push = |name: String, ty: GlslType, source: UniformSource| {
        offset = offset.next_multiple_of(ty.std140_align());
        members.push(BlockMember {
            name,
            ty,
            offset,
            source,
            default: None,
        });
        offset += ty.std140_size();
    };
    for name in cu.referenced_inputs() {
        let ty = input_type(&name).unwrap_or(GlslType::FLOAT);
        push(name.clone(), ty, UniformSource::Builtin(name));
    }
    for (name, ty) in cu.outputs() {
        push(name.clone(), ty, UniformSource::Custom(name));
    }
    BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: offset.next_multiple_of(16),
        members,
    }
}

/// Deterministic, plausible-ish input values.
fn input_value(name: &str) -> Option<Value> {
    let ty = input_type(name)?;
    let base = match name {
        "viewWidth" => 1920.0,
        "viewHeight" => 1080.0,
        "aspectRatio" => 16.0 / 9.0,
        "far" | "dhRenderDistance" => 512.0,
        "near" => 0.05,
        "frameTime" => 1.0 / 60.0,
        "frameCounter" => 1234.0,
        "worldTime" => 6000.0,
        "sunAngle" => 0.25,
        _ => 0.5,
    };
    Some(match ty {
        t if t == GlslType::MAT4 => {
            Value::Mat4(std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.1 }))
        }
        _ => Value::Float(base).convert(sb_expr::ValueType::from_glsl(ty)?),
    })
}

/// Bitwise-equal values, treating NaN as equal to NaN.
fn same(a: Value, b: Value) -> bool {
    let eq = |x: &f32, y: &f32| x == y || (x.is_nan() && y.is_nan());
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => eq(&x, &y),
        (Value::Vec2(x), Value::Vec2(y)) => x.iter().zip(&y).all(|(x, y)| eq(x, y)),
        (Value::Vec3(x), Value::Vec3(y)) => x.iter().zip(&y).all(|(x, y)| eq(x, y)),
        (Value::Vec4(x), Value::Vec4(y)) => x.iter().zip(&y).all(|(x, y)| eq(x, y)),
        (a, b) => a == b,
    }
}

/// Problems found in one pack's custom uniforms.
#[derive(Default)]
struct Findings {
    lines: usize,
    parse_failures: Vec<String>,
    compile_errors: Vec<String>,
}

/// Parse, print/re-parse, compile (all `#if` branches at once, with a permissive
/// input map) and evaluate every custom uniform of one pack, through both the map
/// API and `evaluate_into_block`, and check that they agree.
fn check_pack(pack: &str, path: &Path, constants: &IndexMap<String, Value>) -> Findings {
    let mut found = Findings::default();
    let entries = entries(path);
    let defs = custom_defs(&entries);
    found.lines = defs.len();
    for (line, d) in &defs {
        match parse(&d.expression) {
            Ok(e) => {
                // The fully parenthesized form re-parses to the same tree.
                let printed = e.to_string();
                let reparsed = parse(&printed).map(|r| r.to_string());
                assert_eq!(reparsed.as_deref(), Ok(printed.as_str()), "{pack}:{line}");
            }
            Err(e) => found
                .parse_failures
                .push(format!("{pack}:{line}: `{}`: {e}", d.expression)),
        }
    }

    // Compile every line of every #if branch at once (later identical keys win,
    // like java.util.Properties), with a permissive input map.
    let defs: Vec<CustomUniform> = defs.into_iter().map(|(_, d)| d).collect();
    // Permissive builtins, except the pack's own custom names.
    let custom_names: HashSet<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    let pack_input_type = |name: &str| {
        if custom_names.contains(name) {
            None
        } else {
            input_type(name)
        }
    };
    let (mut cu, diags) = CustomUniforms::compile(&defs, &pack_input_type, constants);
    for d in diags.iter().filter(|d| d.severity == Severity::Error) {
        found.compile_errors.push(format!("{pack}: {d}"));
    }
    let mut codes: BTreeMap<&str, usize> = BTreeMap::new();
    for d in diags.iter() {
        *codes.entry(d.code.as_str()).or_default() += 1;
    }
    // Raw scanning sees every #if branch, so only duplicate-name diagnostics are
    // expected (the pack's own names are never offered as builtins above).
    for code in codes.keys() {
        assert!(
            matches!(*code, "expr.redefined" | "expr.duplicate"),
            "{pack}: unexpected {code}: {diags:?}"
        );
    }
    assert!(
        !cu.outputs().is_empty() || defs.is_empty(),
        "{pack}: no outputs"
    );
    eprintln!(
        "{pack}: {} definitions, {} outputs, {} inputs, diagnostics {codes:?}",
        cu.len(),
        cu.outputs().len(),
        cu.referenced_inputs().len(),
    );

    // Block-based evaluation must match map-based evaluation of the same inputs
    // (read back from the block, so integer members truncate identically).
    let layout = layout_for(&cu);
    let mut block = vec![0u8; layout.size as usize];
    for m in &layout.members {
        if let UniformSource::Builtin(name) = &m.source
            && let Some(v) = input_value(name)
        {
            write_value(m.ty, v, &mut block[m.offset as usize..]).expect("write input");
        }
    }
    let (mut reference, _) = CustomUniforms::compile(&defs, &pack_input_type, constants);
    for _ in 0..3 {
        let snapshot = block.clone();
        let expected = reference.evaluate(&BlockInputs::new(&layout, &snapshot), 1.0 / 60.0);
        assert_eq!(expected.len(), cu.outputs().len());
        for ((name, value), (oname, oty)) in expected.iter().zip(cu.outputs()) {
            assert_eq!(name, &oname);
            assert_eq!(
                value.ty().glsl(),
                oty,
                "{pack}: {name} has the declared type"
            );
        }
        cu.evaluate_into_block(&layout, &mut block, 1.0 / 60.0);
        for (name, value) in &expected {
            let m = layout.member(name).expect("output member");
            let got = read_value(m.ty, &block[m.offset as usize..]).expect("read output");
            assert!(
                same(got, *value),
                "{pack}: {name}: block {got:?} vs map {value:?}"
            );
        }
    }
    assert!(
        cu.check_block(&layout).is_empty(),
        "{pack}: {:?}",
        cu.check_block(&layout)
    );
    found
}

#[test]
fn corpus_custom_uniforms_parse_compile_and_evaluate() {
    let Some(corpus) = corpus_dir() else { return };
    let constants: IndexMap<String, Value> = standard_constants();
    let mut total = 0;
    let mut parse_failures = Vec::new();
    let mut compile_errors = Vec::new();
    for (pack, min) in PACKS {
        let path = corpus.join(pack).join("shaders/shaders.properties");
        if !path.is_file() {
            eprintln!("{pack}: no shaders.properties; skipping");
            continue;
        }
        let found = check_pack(pack, &path, &constants);
        assert!(
            found.lines >= *min,
            "{pack}: expected at least {min} custom uniform lines, found {}",
            found.lines
        );
        total += found.lines;
        parse_failures.extend(found.parse_failures);
        compile_errors.extend(found.compile_errors);
    }
    assert!(
        parse_failures.is_empty(),
        "unparsable expressions:\n{}",
        parse_failures.join("\n")
    );
    assert!(
        compile_errors.is_empty(),
        "compile errors:\n{}",
        compile_errors.join("\n")
    );
    eprintln!("{total} custom uniform/variable lines parsed");
    assert!(total >= 250, "only {total} lines found");
}

#[test]
fn extended_corpus_custom_uniforms_parse_compile_and_evaluate() {
    let packs = corpus2_packs();
    if packs.is_empty() {
        eprintln!("second corpus not available; skipping");
        return;
    }
    let constants: IndexMap<String, Value> = standard_constants();
    let mut total = 0;
    let mut problems = Vec::new();
    for (pack, path) in &packs {
        let found = check_pack(pack, path, &constants);
        total += found.lines;
        problems.extend(found.parse_failures);
        problems.extend(found.compile_errors);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    eprintln!(
        "{} packs, {total} custom uniform/variable lines",
        packs.len()
    );
    assert!(
        total >= 1000,
        "only {total} lines found in {} packs",
        packs.len()
    );
}

#[test]
fn corpus_enabled_conditions_evaluate() {
    let Some(corpus) = corpus_dir() else { return };
    let mut count = 0;
    let mut failures = Vec::new();
    let files = PACKS
        .iter()
        .map(|(pack, _)| {
            (
                pack.to_string(),
                corpus.join(pack).join("shaders/shaders.properties"),
            )
        })
        .chain(corpus2_packs());
    for (pack, path) in files {
        if !path.is_file() {
            continue;
        }
        for e in entries(&path) {
            if !e.key.ends_with(".enabled") {
                continue;
            }
            count += 1;
            // All options off: unknown identifiers are false.
            if let Err(err) = eval_bool(&e.value, &|_| None) {
                failures.push(format!("{pack}:{}: {} = {}: {err}", e.line, e.key, e.value));
            }
            if let Err(err) = eval_bool(&e.value, &|_| Some(true)) {
                failures.push(format!("{pack}:{}: {} = {}: {err}", e.line, e.key, e.value));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{count} enabled conditions evaluated");
    assert!(count > 20);
}

#[test]
fn continuation_lines_are_joined() {
    let lines = logical_lines(
        "a = if( \\\n\t x, 1, \\\n  2)\nb = 3\nc = 4 \\\\\nd = 5\ne = f( \\\n1) \\\n#else\ne = 2",
    );
    let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "a = if( x, 1, 2)",
            "b = 3",
            "c = 4 \\\\",
            "d = 5",
            "e = f( 1) ",
            "#else",
            "e = 2"
        ]
    );
    assert_eq!(lines[1].0, 4);
}
