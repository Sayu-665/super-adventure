//! Integration tests against real shader packs (see `common` for the corpus
//! location; tests skip when it is missing).

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{DirSources, corpus_options, corpus_root, program_files, shader_roots};
use glsl_lang::ast::TranslationUnit;
use glsl_lang::parse::DefaultParse;
use sb_core::Diagnostic;
use sb_preprocess::escape::CONTEXTUAL_RESERVED;
use sb_preprocess::{
    ESCAPE_PREFIX, PreprocessOptions, Preprocessed, Preprocessor, Profile, preprocess_properties,
};

/// Errors that are genuine properties of the packs, not preprocessor bugs.
fn known_pack_problem(pack: &str, program: &str, d: &Diagnostic) -> Option<&'static str> {
    // photon guards programs that shaders.properties disables (program.X.enabled)
    // when the feature is off with an active `#error`. Preprocessing every
    // program unconditionally (as this test does) reaches them.
    if pack == "photon" && d.code == "pp.error" && d.message.contains("should be disabled") {
        return Some(
            "photon: #error in a program disabled by shaders.properties for the default options",
        );
    }
    let _ = program;
    None
}

struct ProgramResult {
    pack: String,
    program: String,
    out: Preprocessed,
}

fn preprocess_corpus() -> Option<(Vec<ProgramResult>, Duration)> {
    let corpus = corpus_root()?;
    let opts = corpus_options();
    let mut results = Vec::new();
    let start = Instant::now();
    for (pack, shaders) in shader_roots(&corpus) {
        let sources = DirSources {
            root: shaders.clone(),
        };
        let mut pp = Preprocessor::new(&sources);
        for program in program_files(&shaders) {
            let out = pp.preprocess(&program, &opts);
            results.push(ProgramResult {
                pack: pack.clone(),
                program,
                out,
            });
        }
    }
    Some((results, start.elapsed()))
}

#[test]
fn corpus_programs_preprocess_cleanly() {
    let Some((results, elapsed)) = preprocess_corpus() else {
        return;
    };
    assert!(
        results.len() > 100,
        "expected a real corpus, found {} programs",
        results.len()
    );
    let mut per_pack: BTreeMap<&str, usize> = BTreeMap::new();
    let mut warnings: BTreeMap<String, usize> = BTreeMap::new();
    let mut known: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unexpected = Vec::new();
    for r in &results {
        *per_pack.entry(&r.pack).or_default() += 1;
        let out = &r.out;
        assert_eq!(
            out.line_map.len(),
            out.code.lines().count(),
            "{} {}: line map size",
            r.pack,
            r.program
        );
        assert!(out.code.is_empty() || out.code.ends_with('\n'));
        assert_eq!(
            out.files.first().map(String::as_str),
            Some(r.program.as_str())
        );
        for loc in &out.line_map {
            assert!(
                out.files.contains(&loc.file),
                "{} {}: line map file {} not in files",
                r.pack,
                r.program,
                loc.file
            );
            assert!(loc.line >= 1);
        }
        // Includes are always expanded (an `#include` may only survive inside a comment).
        assert!(
            !out.code
                .lines()
                .any(|l| l.trim_start().starts_with("#include")),
            "{} {}: unexpanded include",
            r.pack,
            r.program
        );
        for d in out.diagnostics.iter() {
            if d.is_error() {
                match known_pack_problem(&r.pack, &r.program, d) {
                    Some(why) => *known.entry(why).or_default() += 1,
                    None => unexpected.push(format!("{} {}: {d}", r.pack, r.program)),
                }
            } else {
                *warnings.entry(d.code.clone()).or_default() += 1;
            }
        }
    }
    println!("preprocessed {} programs in {elapsed:?}", results.len());
    println!("programs per pack: {per_pack:?}");
    println!("warnings by code: {warnings:?}");
    println!("known pack problems: {known:?}");
    assert!(
        unexpected.is_empty(),
        "unexpected errors:\n{}",
        unexpected.join("\n")
    );
}

/// Every occurrence of a context-sensitive reserved word in the corpora is a
/// keyword use (`flat`, `layout(`, `switch`, `case`, `default`, `double`, ...;
/// checked with grep: none is used as an identifier), so escaping any of them
/// would be a false positive that breaks valid code.
#[test]
fn corpus_contextual_words_are_never_escaped() {
    let Some((results, _)) = preprocess_corpus() else {
        return;
    };
    let mut escaped: BTreeMap<String, usize> = BTreeMap::new();
    let mut false_escapes = Vec::new();
    for r in &results {
        for w in r
            .out
            .code
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter_map(|w| w.strip_prefix(ESCAPE_PREFIX))
        {
            *escaped.entry(w.to_owned()).or_default() += 1;
            if CONTEXTUAL_RESERVED.iter().any(|c| c.word == w) {
                false_escapes.push(format!("{} {}: {w}", r.pack, r.program));
            }
        }
    }
    println!("escaped identifiers in the corpus: {escaped:?}");
    assert!(
        false_escapes.is_empty(),
        "contextual words escaped:\n{}",
        false_escapes
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn corpus_outputs_parse_with_glsl_lang() {
    let Some((results, _)) = preprocess_corpus() else {
        return;
    };
    let mut failures = Vec::new();
    let mut parsed = 0usize;
    for r in &results {
        if r.out.diagnostics.has_errors() {
            continue;
        }
        // Parse as at least 130 (uint/uvec lex as types), like sb-transform does.
        let v = r.out.version_number().max(130);
        let profile = match r.out.version.and_then(|v| v.profile) {
            Some(Profile::Es) => " es",
            _ => "",
        };
        let src = format!("#version {v}{profile}\n{}", r.out.code);
        match TranslationUnit::parse(src.as_str()) {
            Ok(_) => parsed += 1,
            Err(e) => {
                let msg = e.to_string();
                failures.push(format!(
                    "{} {}: {}",
                    r.pack,
                    r.program,
                    msg.lines().next().unwrap_or("")
                ));
            }
        }
    }
    println!(
        "glsl-lang parsed {parsed}/{} preprocessed programs",
        parsed + failures.len()
    );
    for f in &failures {
        println!("  parse failure: {f}");
    }
    // Known remaining failures are not preprocessing defects:
    // * Bliss-Shader composite/composite3: glsl-lang rejects the valid float
    //   literal `1e-8` ("invalid float literal").
    // * photon *_voxels: use the `RayJob` API that the Photonics mod injects at
    //   runtime (the pack ships an empty stub `photonics/photonics.glsl`).
    let rate = failures.len() as f64 / (parsed + failures.len()).max(1) as f64;
    assert!(
        rate < 0.05,
        "too many glsl-lang parse failures: {}",
        failures.len()
    );
}

#[test]
fn photon_preprocessing_speed() {
    let Some(corpus) = corpus_root() else { return };
    let shaders = corpus.join("photon/shaders");
    if !shaders.is_dir() {
        return;
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&shaders)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        let p = entry.path();
        if p.is_file()
            && matches!(p.extension().and_then(|e| e.to_str()), Some("vsh" | "fsh"))
            && let Ok(rel) = p.strip_prefix(&shaders)
        {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    let sources = DirSources { root: shaders };
    let mut pp = Preprocessor::new(&sources);
    let opts = corpus_options();
    let start = Instant::now();
    let mut bytes = 0usize;
    for f in &files {
        bytes += pp.preprocess(f, &opts).code.len();
    }
    let elapsed = start.elapsed();
    println!(
        "photon: {} .vsh/.fsh files, {} MB output, {elapsed:?}",
        files.len(),
        bytes / (1 << 20)
    );
    if !cfg!(debug_assertions) {
        assert!(
            elapsed < Duration::from_secs(2),
            "photon preprocessing took {elapsed:?} (target < 2 s in release)"
        );
    }
}

#[test]
fn corpus_properties_files() {
    let Some(corpus) = corpus_root() else { return };
    let mut defines = corpus_options().defines;
    defines.insert("MC_RENDER_QUALITY".into(), Some("1.0".into()));
    let mut count = 0;
    let mut unexpected = Vec::new();
    for (pack, shaders) in shader_roots(&corpus) {
        let Ok(rd) = std::fs::read_dir(&shaders) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "properties"))
            .collect();
        files.sort();
        for path in files {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            // Iris reads .properties files as ISO-8859-1.
            let text: String = bytes.iter().map(|&b| b as char).collect();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let (out, diags) = preprocess_properties(&text, &name, &defines);
            count += 1;
            let non_blank = text
                .split(['\n', '\r'])
                .filter(|l| !l.trim().is_empty())
                .count();
            assert!(
                out.lines().count() <= non_blank + 1,
                "{pack} {name}: more output lines than input lines"
            );
            for d in diags.errors() {
                unexpected.push(format!("{pack} {name}: {d}"));
            }
        }
    }
    println!("preprocessed {count} .properties files");
    assert!(count > 10);
    assert!(
        unexpected.is_empty(),
        "unexpected errors:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn corpus_include_closures() {
    let Some(corpus) = corpus_root() else { return };
    for (pack, shaders) in shader_roots(&corpus) {
        let sources = DirSources {
            root: shaders.clone(),
        };
        let mut pp = Preprocessor::new(&sources);
        for program in program_files(&shaders) {
            let (files, diags) = pp.include_closure(&program);
            assert_eq!(files.first(), Some(&program));
            assert!(!diags.has_errors(), "{pack} {program}: {:?}", diags);
            let full = pp.preprocess(&program, &PreprocessOptions::default());
            for f in &full.files {
                assert!(
                    files.contains(f),
                    "{pack} {program}: {f} missing from include closure"
                );
            }
        }
    }
}
