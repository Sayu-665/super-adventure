//! Robustness: every parser must accept arbitrary (malformed) input without panicking.
//!
//! A small deterministic mutation fuzzer feeds mutated seed texts (and, when the
//! corpus is available, mutated real pack files) through every text-level entry
//! point of the crate.

use sb_core::SourceProvider;
use sb_pack::options::{self, OptionValues, annotate};
use sb_pack::{
    EditedSources, ProgramSet, ShaderPack, ZipVfs, idmap, includes, lang, properties,
    shaders_properties, vfs,
};
use std::sync::Arc;

/// xorshift64* — deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Fragments that exercise the interesting parser states.
const FRAGMENTS: &[&str] = &[
    "\\",
    "\\\n",
    "\\\r\n",
    "\r",
    "\n",
    "\r\n",
    "#",
    "!",
    "=",
    ":",
    " ",
    "\t",
    "\u{c}",
    "[",
    "]",
    "//",
    "/*",
    "*/",
    "\"",
    "<",
    ">",
    "%",
    ".",
    "..",
    "/",
    "\\u",
    "\\u12",
    "\\uD83D",
    "\\uDE00",
    "é",
    "😀",
    "\u{2003}",
    "\u{a0}",
    "\0",
    "#define ",
    "#ifdef ",
    "#ifndef ",
    "const int ",
    "const bool ",
    "shadowMapResolution",
    " = ",
    ";",
    " // [",
    "1 2 3",
    "]",
    "#include ",
    "\"/lib/a.glsl\"",
    "block.",
    "layer.",
    "item.",
    "entity.",
    "dimension.",
    "texture.",
    "composite.",
    "colortex",
    "gaux1",
    "image.",
    "bufferObject.",
    "uniform.",
    "variable.",
    "float.",
    "vec3.",
    "screen",
    "profile.",
    "sliders",
    "program.",
    ".enabled",
    "world0/",
    "blend.",
    "flip.",
    "scale.",
    "alphaTest.",
    "size.buffer.",
    "indirect.",
    "TEXTURE_3D ",
    "RGBA8 ",
    "RGBA ",
    "UNSIGNED_BYTE",
    "true",
    "false",
    "-1",
    "0.5",
    "5.",
    "1e5",
    "0x10",
    "f",
    "_a",
    "composite3_b",
    "!program.",
    "OPT:1",
    "!OPT",
    "*",
];

const SEEDS: &[&str] = &[
    "sun=false\nclouds=fast\nweather=true false\nscale.composite=0.5 0.1 0.2\n\
     texture.composite.colortex8=tex/a.png\ntexture.gbuffers.x.1=a.dat TEXTURE_2D RGBA8 4 4 RGBA UNSIGNED_BYTE\n\
     image.i=s RGBA RGBA8 UNSIGNED_BYTE true true 0.5 0.5\nbufferObject.1=16 true 1 1\n\
     uniform.vec3.x=vec3(1)\nscreen=<profile> [A] B *\nscreen.A=C\nprofile.LOW=!B C=1 profile.HIGH\n\
     profile.HIGH=B profile.LOW\nblend.gbuffers_water.colortex1=ONE ZERO ONE ZERO\nflip.composite.gaux1=true\n\
     program.world0/composite2.enabled=A && !B\nsize.buffer.colortex1=0.5 64\nalphaTest.shadow=GREATER 0.1\n",
    "#version 120\n#include \"/lib/settings.glsl\"\n#define A // comment\n//#define B\n\
     #define Q 2 // [1 2 3] quality\nconst int shadowMapResolution = 2048; // [1024 2048]\n\
     const bool shadowHardwareFiltering = true;\n#ifdef A\n#endif\n#ifndef B\n#endif\nvoid main() {}\n",
    "block.10=minecraft:stone dirt \\\n  %minecraft:logs wheat:age=7\nlayer.cutout=glass\n\
     item.5=torch\nentity.3=pig\ndimension.world0=minecraft:overworld *\n",
    "option.A=Alpha \\u00e9\noption.A.comment=Line \\\n  two\nvalue.Q.1=Low\n",
];

fn mutate(rng: &mut Rng, seed: &str) -> String {
    let mut s: Vec<char> = seed.chars().collect();
    let ops = 1 + rng.below(12);
    for _ in 0..ops {
        match rng.below(4) {
            0 if !s.is_empty() => {
                let i = rng.below(s.len());
                s.remove(i);
            }
            1 if !s.is_empty() => {
                // Truncate.
                let i = rng.below(s.len());
                s.truncate(i);
            }
            _ => {
                let frag = FRAGMENTS[rng.below(FRAGMENTS.len())];
                let i = rng.below(s.len() + 1);
                for (k, c) in frag.chars().enumerate() {
                    s.insert(i + k, c);
                }
            }
        }
    }
    s.into_iter().collect()
}

/// Run every text-level parser over `text`.
fn exercise(text: &str) {
    let entries = properties::parse(text);
    let _ = properties::parse_preprocessed(text);
    let _ = properties::parse_with_diagnostics(text, "x.properties");
    let (props, _) = shaders_properties::parse(&entries, &entries);
    let _ = props.settings();
    let _ = props.custom_textures_model();
    let _ = props.screens_model();
    let _ = props.texture_directives();
    let _ = idmap::parse_block_properties(text);
    let _ = idmap::parse_item_properties(text);
    let _ = idmap::parse_entity_properties(text);
    let _ = idmap::parse_dimension_properties(text);
    let _ = lang::parse_lang(text);
    let _ = OptionValues::parse_settings_file(text);
    let _ = shaders_properties::parse_texture_source(text);
    let _ = shaders_properties::parse_blend_value(text);
    let _ = shaders_properties::parse_bool(text);
    let _ = shaders_properties::parse_buffer_name(text);
    let _ = shaders_properties::parse_screen_entry(text);
    let _ = vfs::clean_path(text);
    for line in text.lines() {
        let _ = annotate::annotate_line(line);
        let _ = annotate::annotate_line_with(line, true);
        let _ = annotate::set_boolean_define(line, true);
        let _ = annotate::set_boolean_define(line, false);
        let _ = includes::parse_include_line(line);
    }
    let names: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    let _ = ProgramSet::discover("", &names);

    // Option discovery + profiles + editing over a pack made of this text.
    let pack = Arc::new(ShaderPack::from_files(
        "fuzz",
        [
            ("composite.fsh", text.to_string()),
            ("lib/settings.glsl", text.to_string()),
            ("shaders.properties", text.to_string()),
        ],
    ));
    let (opts, _) = options::discover(&pack, &pack.option_start_files());
    let (profiles, _) = options::resolve_profiles(&props.profiles, &opts);
    let mut values = OptionValues::new();
    for p in profiles.values() {
        values.apply_profile(p, &opts);
    }
    for o in &opts.options {
        let _ = values.cycle(&o.name, &opts);
    }
    let _ = options::detect_profile(&profiles, &opts, &values);
    let _ = options::build_options_model(&props, &opts, &values, Default::default());
    let _ = opts.property_macros(&values);
    let sources = EditedSources::new(pack.clone(), opts, values);
    let _ = sources.read("composite.fsh");
    let _ = sources.read("lib/settings.glsl");
    let _ = props.resolve_custom_textures(&pack);
}

#[test]
fn parsers_never_panic_on_mutated_seeds() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for seed in SEEDS {
        exercise(seed);
        for _ in 0..400 {
            exercise(&mutate(&mut rng, seed));
        }
    }
}

#[test]
fn parsers_never_panic_on_mutated_corpus_files() {
    let corpus = std::env::var_os("SB_CORPUS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus".into()
        });
    let dir = corpus.join("ComplementaryReimagined");
    let Ok(pack) = ShaderPack::open(&dir) else {
        eprintln!("corpus not found; skipping");
        return;
    };
    let mut rng = Rng(42);
    for file in ["shaders.properties", "block.properties", "lib/common.glsl"] {
        let Some(text) = pack.read_text(file) else {
            continue;
        };
        exercise(&text);
        for _ in 0..20 {
            exercise(&mutate(&mut rng, &text));
        }
    }
}

#[test]
fn zip_reader_never_panics_on_garbage() {
    let mut rng = Rng(7);
    for len in [0usize, 1, 4, 22, 64, 512] {
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = ZipVfs::from_bytes(bytes.clone());
        // An end-of-central-directory signature with garbage around it.
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend((0..18).map(|_| rng.next() as u8));
        let _ = ZipVfs::from_bytes(bytes);
    }
}
