//! The `shaderbridge` command-line tool.
//!
//! * `inspect <pack>`: programs per folder, options, profiles, feature flags, DH strategy.
//! * `compile <pack> -o DIR`: writes `pack.json`, `blobs.bin`, one file per translated
//!   stage (`<folder>/<program>.<stage ext>.{spv,vk.glsl,rp.glsl}`) and `diagnostics.txt`.
//! * `validate <packs>...`: compiles every pack with `spirv-val` and prints a table — the
//!   corpus referee. Directories that are not packs are searched for packs.
//! * `options <pack>`: options with defaults and allowed values, screens, profiles.
//! * `render <pack> -o out.png`: compiles for the local Vulkan device and renders the
//!   synthetic scene with `sb-runtime`.
//! * `profiles`: the built-in draw profiles.
//!
//! [`run`] is the testable entry point (`main` only forwards `std::env::args`).

use clap::{Parser, Subcommand, ValueEnum};
use indexmap::IndexMap;
use sb_core::model::{CompiledPack, DepthMode, OutputTarget, ScreenEntry};
use sb_core::{Diagnostics, Severity, ShaderStage};
use sb_pack::{OptionValues, ShaderPack};
use sb_pipeline::{CompileOutput, CompileSettings};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Depth convention (ARCHITECTURE §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DepthArg {
    /// Forward Z in [0,1] (LESS, clear 1).
    Forward,
    /// Reversed Z in [0,1] (GEQUAL, clear 0; Minecraft 26.2+).
    Reversed,
    /// GL [-1,1] (no remap; needs depth clip control).
    Gl,
}

impl From<DepthArg> for DepthMode {
    fn from(d: DepthArg) -> Self {
        match d {
            DepthArg::Forward => DepthMode::ForwardZeroToOne,
            DepthArg::Reversed => DepthMode::ReversedZeroToOne,
            DepthArg::Gl => DepthMode::GlNegOneToOne,
        }
    }
}

/// Output target(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TargetArg {
    Vulkan,
    Renderpearl,
    Both,
}

impl TargetArg {
    fn targets(self) -> Vec<OutputTarget> {
        match self {
            TargetArg::Vulkan => vec![OutputTarget::Vulkan],
            TargetArg::Renderpearl => vec![OutputTarget::Renderpearl],
            TargetArg::Both => vec![OutputTarget::Vulkan, OutputTarget::Renderpearl],
        }
    }
}

/// Options shared by the compiling commands.
#[derive(Debug, Clone, clap::Args)]
pub struct CompileArgs {
    /// Only compile these world folders (`""` = pack root); repeatable.
    #[arg(long = "dim")]
    pub dims: Vec<String>,
    /// Output target(s).
    #[arg(long, value_enum, default_value = "both")]
    pub target: TargetArg,
    /// Depth convention.
    #[arg(long, value_enum, default_value = "forward")]
    pub depth: DepthArg,
    /// Set an option (`NAME=VALUE`); repeatable.
    #[arg(long = "set", value_name = "OPT=VAL")]
    pub set: Vec<String>,
    /// Compile without Distant Horizons.
    #[arg(long)]
    pub no_dh: bool,
    /// Load option values from an Iris settings file (`shaderpacks/<pack>.txt`).
    #[arg(long, value_name = "FILE")]
    pub settings: Option<PathBuf>,
    /// Language for option names.
    #[arg(long, default_value = "en_us")]
    pub lang: String,
}

#[derive(Debug, Parser)]
#[command(name = "shaderbridge", version, about = "Translate OptiFine/Iris shader packs to Vulkan GLSL and SPIR-V")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Summarize a pack: programs per folder, options, profiles, features, DH strategy.
    Inspect {
        pack: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compile a pack and write the model, blobs and translated stages.
    Compile {
        pack: PathBuf,
        /// Output directory.
        #[arg(short, long, default_value = "shaderbridge-out")]
        output: PathBuf,
        #[command(flatten)]
        args: CompileArgs,
        /// Run spirv-val on every module.
        #[arg(long)]
        validate: bool,
        /// Compile cache directory.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
    /// Compile packs with spirv-val and report pass/fail per pack (exit code 1 on failures).
    Validate {
        /// Packs (directories or zips); other directories are searched for packs.
        #[arg(required = true)]
        packs: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        args: CompileArgs,
        /// Skip spirv-val (glslang only).
        #[arg(long)]
        no_spirv_val: bool,
        /// Number of failure messages shown per pack.
        #[arg(long, default_value_t = 5)]
        top: usize,
    },
    /// List options (defaults, allowed values, current values), screens and profiles.
    Options {
        pack: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "en_us")]
        lang: String,
    },
    /// Compile a pack for the local Vulkan device and render the synthetic scene to a PNG.
    Render {
        pack: PathBuf,
        /// Output PNG.
        #[arg(short, long, default_value = "render.png")]
        output: PathBuf,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        #[arg(long, default_value_t = 3)]
        frames: u32,
        /// World time in ticks (0 = sunrise, 6000 = noon).
        #[arg(long)]
        time: Option<i64>,
        /// World folder to render (default: world0, else the root).
        #[arg(long = "dim", default_value = "world0")]
        dim: String,
        #[arg(long, value_enum, default_value = "forward")]
        depth: DepthArg,
        /// Disable the Vulkan validation layer.
        #[arg(long)]
        no_validation: bool,
        /// Write every render target as PNG into this directory.
        #[arg(long)]
        capture: Option<PathBuf>,
        /// Prefer a CPU device (lavapipe).
        #[arg(long)]
        cpu: bool,
        /// Set an option (`NAME=VALUE`); repeatable.
        #[arg(long = "set", value_name = "OPT=VAL")]
        set: Vec<String>,
        /// Render without Distant Horizons.
        #[arg(long)]
        no_dh: bool,
    },
    /// List the built-in draw profiles.
    Profiles {
        #[arg(long)]
        json: bool,
    },
}

/// Run the tool with `args` (including the program name), writing to `out`/`err`.
/// Returns the process exit code.
pub fn run(args: impl IntoIterator<Item = String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = if e.use_stderr() { write!(err, "{e}") } else { write!(out, "{e}") };
            return code;
        }
    };
    let r = match cli.command {
        Command::Inspect { pack, json } => cmd_inspect(&pack, json, out),
        Command::Compile { pack, output, args, validate, cache } => cmd_compile(&pack, &output, &args, validate, cache, out),
        Command::Validate { packs, json, args, no_spirv_val, top } => cmd_validate(&packs, &args, !no_spirv_val, top, json, out),
        Command::Options { pack, json, lang } => cmd_options(&pack, json, &lang, out),
        Command::Render { pack, output, width, height, frames, time, dim, depth, no_validation, capture, cpu, set, no_dh } => {
            let r = RenderArgs { output, width, height, frames, time, dim, depth, validation: !no_validation, capture, cpu, set, no_dh };
            cmd_render(&pack, &r, out)
        }
        Command::Profiles { json } => cmd_profiles(json, out),
    };
    match r {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            1
        }
    }
}

type CmdResult = Result<i32, String>;

fn open_pack(path: &Path) -> Result<ShaderPack, String> {
    ShaderPack::open(path).map_err(|e| format!("cannot open pack {}: {e}", path.display()))
}

fn parse_sets(sets: &[String]) -> Result<OptionValues, String> {
    let mut v = OptionValues::new();
    for s in sets {
        let (k, val) = s.split_once('=').ok_or_else(|| format!("--set expects NAME=VALUE, got `{s}`"))?;
        v.set(k.trim(), val.trim());
    }
    Ok(v)
}

/// Compile settings from command-line arguments.
pub fn settings_from(args: &CompileArgs) -> Result<CompileSettings, String> {
    let mut s = CompileSettings::default();
    s.env.depth_mode = args.depth.into();
    s.env.targets = args.target.targets();
    s.env.distant_horizons = !args.no_dh;
    if let Some(path) = &args.settings {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        s.option_values = OptionValues::parse_settings_file(&sb_pack::text::decode_latin1(&bytes));
    }
    for (k, v) in parse_sets(&args.set)?.iter() {
        s.option_values.set(k, v);
    }
    if !args.dims.is_empty() {
        s.dimension_filter = Some(args.dims.clone());
    }
    s.language = args.lang.clone();
    Ok(s)
}

fn count_by_severity(d: &Diagnostics) -> (usize, usize, usize) {
    let e = d.iter().filter(|x| x.severity == Severity::Error).count();
    let w = d.iter().filter(|x| x.severity == Severity::Warning).count();
    (e, w, d.len() - e - w)
}

fn dh_label(s: sb_core::model::DhStrategy) -> &'static str {
    match s {
        sb_core::model::DhStrategy::Native => "native",
        sb_core::model::DhStrategy::Synthesized => "synthesized",
        sb_core::model::DhStrategy::Disabled => "disabled",
    }
}

// ---------------------------------------------------------------------------------------
// inspect
// ---------------------------------------------------------------------------------------

fn cmd_inspect(path: &Path, json: bool, out: &mut dyn Write) -> CmdResult {
    let pack = open_pack(path)?;
    let s = sb_pipeline::inspect(&pack);
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let w = |out: &mut dyn Write, t: String| writeln!(out, "{t}").map_err(|e| e.to_string());
    w(out, format!("pack: {}", s.name))?;
    w(out, format!(
        "options: {}  profiles: {}{}  screens: {}  custom uniforms: {}  custom textures: {}  languages: {}",
        s.option_count,
        s.profile_count,
        s.current_profile.as_ref().map(|p| format!(" (current: {p})")).unwrap_or_default(),
        s.screen_count,
        s.custom_uniform_count,
        s.custom_texture_count,
        if s.languages.is_empty() { "-".to_string() } else { s.languages.join(" ") }
    ))?;
    w(out, format!(
        "features: required [{}] optional [{}]{}",
        s.features_required.join(" "),
        s.features_optional.join(" "),
        if s.features_unsupported.is_empty() { String::new() } else { format!(" UNSUPPORTED [{}]", s.features_unsupported.join(" ")) }
    ))?;
    for f in &s.folders {
        let name = if f.folder.is_empty() { "<root>" } else { &f.folder };
        w(out, format!("folder {name}  dimensions [{}]  DH: {}", f.dimension_ids.join(" "), dh_label(f.dh_strategy)))?;
        for p in &f.programs {
            w(out, format!("  {:32} {}", p.name, p.stages.join(" ")))?;
        }
        if !f.disabled.is_empty() {
            w(out, format!("  disabled: {}", f.disabled.join(" ")))?;
        }
    }
    let (e, wn, i) = count_by_severity(&s.diagnostics);
    w(out, format!("diagnostics: {e} errors, {wn} warnings, {i} notes"))?;
    for d in s.diagnostics.errors() {
        w(out, format!("  {d}"))?;
    }
    Ok(0)
}

// ---------------------------------------------------------------------------------------
// compile
// ---------------------------------------------------------------------------------------

/// File stem for a program's stage outputs, unique within the output directory.
fn stage_file_base(name: &str, profile: Option<&str>, unique: bool) -> String {
    let base = name.strip_suffix(".csh").unwrap_or(name);
    match (unique, profile) {
        (false, Some(p)) => format!("{base}@{p}"),
        _ => base.to_string(),
    }
}

/// Write the model, blob buffer, per-stage files and diagnostics into `dir`.
pub fn write_output(out: &CompileOutput, dir: &Path) -> Result<usize, String> {
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(dir).map_err(io)?;
    let (infos, buf) = out.blobs.concat();
    let mut model = out.pack.clone();
    model.blobs = infos;
    std::fs::write(dir.join("pack.json"), serde_json::to_string_pretty(&model).map_err(|e| e.to_string())?).map_err(io)?;
    std::fs::write(dir.join("blobs.bin"), buf).map_err(io)?;
    let mut files = 0;
    for dim in &out.pack.dimensions {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for p in &dim.programs {
            *counts.entry(p.name.as_str()).or_default() += 1;
        }
        let mut written: BTreeMap<String, ()> = BTreeMap::new();
        for p in &dim.programs {
            let base = stage_file_base(&p.name, p.draw_profile.as_deref(), counts[p.name.as_str()] == 1);
            if written.insert(base.clone(), ()).is_some() {
                continue; // duplicated program entries (per-pass bindings) share stages
            }
            for s in &p.stages {
                let stem = format!("{base}.{}", s.stage.pack_extension());
                let target = dir.join(&stem);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                let mut write = |ext: &str, bytes: &[u8]| -> Result<(), String> {
                    files += 1;
                    std::fs::write(dir.join(format!("{stem}.{ext}")), bytes).map_err(io)
                };
                if let Some(b) = s.spirv.and_then(|id| out.blobs.get(id)) {
                    write("spv", b)?;
                }
                if let Some(b) = s.glsl_vulkan.and_then(|id| out.blobs.get(id)) {
                    write("vk.glsl", b)?;
                }
                if let Some(b) = s.glsl_renderpearl.and_then(|id| out.blobs.get(id)) {
                    write("rp.glsl", b)?;
                }
            }
        }
    }
    let text: String = out.pack.diagnostics.iter().map(|d| format!("{d}\n")).collect();
    std::fs::write(dir.join("diagnostics.txt"), text).map_err(io)?;
    Ok(files)
}

fn print_summary(out: &mut dyn Write, c: &CompileOutput) -> Result<(), String> {
    let w = |out: &mut dyn Write, t: String| writeln!(out, "{t}").map_err(|e| e.to_string());
    for d in &c.pack.dimensions {
        let name = if d.folder.is_empty() { "<root>" } else { &d.folder };
        w(out, format!(
            "{name}: {} programs, {} geometry slots, {} passes, DH {}{}, gbuffer attachments {:?}, shadow {}",
            d.programs.len(),
            d.geometry.len(),
            d.passes.len(),
            dh_label(d.distant_horizons.strategy),
            if d.distant_horizons.shadow_enabled { " (+shadow)" } else { "" },
            d.gbuffer_attachments,
            if d.targets.shadow.enabled { format!("{}px", d.targets.shadow.resolution) } else { "off".into() }
        ))?;
    }
    let (e, wn, i) = count_by_severity(&c.pack.diagnostics);
    w(out, format!(
        "programs: {} ok, {} failed; modules: {} ({} validated); diagnostics: {e} errors, {wn} warnings, {i} notes; {:.0} ms{}",
        c.stats.programs_ok,
        c.stats.programs_failed.len(),
        c.stats.modules,
        c.stats.modules_validated,
        c.timings.total_ms,
        if c.timings.cache_hit { " (cached)" } else { "" }
    ))?;
    for f in &c.stats.programs_failed {
        w(out, format!("  FAILED {f}"))?;
    }
    Ok(())
}

fn cmd_compile(path: &Path, dir: &Path, args: &CompileArgs, validate: bool, cache: Option<PathBuf>, out: &mut dyn Write) -> CmdResult {
    let pack = open_pack(path)?;
    let mut settings = settings_from(args)?;
    settings.validate_spirv = validate;
    settings.cache_dir = cache;
    let c = sb_pipeline::compile_pack(&pack, &settings);
    let files = write_output(&c, dir)?;
    print_summary(out, &c)?;
    writeln!(out, "wrote {} ({files} stage files)", dir.display()).map_err(|e| e.to_string())?;
    Ok(if c.stats.programs_failed.is_empty() { 0 } else { 1 })
}

// ---------------------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------------------

/// Validation result of one pack.
#[derive(Debug, Clone, Serialize)]
pub struct PackReport {
    pub pack: String,
    pub path: String,
    pub dimensions: Vec<String>,
    pub programs_ok: usize,
    pub programs_failed: Vec<String>,
    pub modules: usize,
    pub modules_validated: usize,
    pub errors: usize,
    pub warnings: usize,
    /// Error diagnostics per code.
    pub errors_by_code: BTreeMap<String, usize>,
    /// The first error messages.
    pub top_failures: Vec<String>,
    pub dh: Vec<String>,
    pub elapsed_ms: f64,
    /// The pack could not be opened.
    pub open_error: Option<String>,
}

impl PackReport {
    /// Whether the pack passed: opened, and every program compiled (and validated).
    pub fn passed(&self) -> bool {
        self.open_error.is_none() && self.programs_failed.is_empty() && self.programs_ok > 0
    }
}

/// Packs below `path`: `path` itself if it is a pack (zip, a directory with `shaders/`, or
/// a shaders root), else every pack in its subdirectories (up to three levels). While
/// searching, a directory counts as a bare shaders root only if it holds a shader program
/// file (a lone `shaders.properties`, e.g. OptiFine's documentation, does not).
pub fn find_packs(path: &Path) -> Vec<PathBuf> {
    fn has_program_file(p: &Path) -> bool {
        std::fs::read_dir(p).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.rsplit_once('.').is_some_and(|(_, ext)| ShaderStage::from_pack_extension(ext).is_some())
            })
        })
    }
    fn is_pack(p: &Path, explicit: bool) -> bool {
        if p.is_file() {
            return p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        }
        p.join("shaders").is_dir() || if explicit { sb_pack::DirVfs::looks_like_shaders_root(p) } else { has_program_file(p) }
    }
    fn walk(dir: &Path, depth: u32, explicit: bool, out: &mut Vec<PathBuf>) {
        if is_pack(dir, explicit) {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 || !dir.is_dir() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        subs.sort();
        for s in subs {
            if s.is_dir() || is_pack(&s, false) {
                walk(&s, depth - 1, false, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(path, 3, true, &mut out);
    out
}

/// Compile and validate one pack.
pub fn validate_pack(path: &Path, settings: &CompileSettings, top: usize) -> PackReport {
    let start = Instant::now();
    let mut r = PackReport {
        pack: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.display().to_string(),
        dimensions: Vec::new(),
        programs_ok: 0,
        programs_failed: Vec::new(),
        modules: 0,
        modules_validated: 0,
        errors: 0,
        warnings: 0,
        errors_by_code: BTreeMap::new(),
        top_failures: Vec::new(),
        dh: Vec::new(),
        elapsed_ms: 0.0,
        open_error: None,
    };
    let pack = match ShaderPack::open(path) {
        Ok(p) => p,
        Err(e) => {
            r.open_error = Some(e.to_string());
            return r;
        }
    };
    r.pack = pack.name().to_string();
    let c = sb_pipeline::compile_pack(&pack, settings);
    r.dimensions = c.pack.dimensions.iter().map(|d| if d.folder.is_empty() { "<root>".into() } else { d.folder.clone() }).collect();
    r.dh = c.pack.dimensions.iter().map(|d| dh_label(d.distant_horizons.strategy).to_string()).collect();
    r.programs_ok = c.stats.programs_ok;
    r.programs_failed = c.stats.programs_failed.clone();
    r.modules = c.stats.modules;
    r.modules_validated = c.stats.modules_validated;
    for d in c.pack.diagnostics.iter() {
        match d.severity {
            Severity::Error => {
                r.errors += 1;
                *r.errors_by_code.entry(d.code.clone()).or_default() += 1;
                if r.top_failures.len() < top {
                    r.top_failures.push(d.to_string());
                }
            }
            Severity::Warning => r.warnings += 1,
            Severity::Info => {}
        }
    }
    r.elapsed_ms = start.elapsed().as_secs_f64() * 1e3;
    r
}

fn cmd_validate(paths: &[PathBuf], args: &CompileArgs, spirv_val: bool, top: usize, json: bool, out: &mut dyn Write) -> CmdResult {
    let mut settings = settings_from(args)?;
    settings.validate_spirv = spirv_val;
    let mut packs = Vec::new();
    for p in paths {
        let found = find_packs(p);
        if found.is_empty() {
            writeln!(out, "warning: no shader pack found in {}", p.display()).map_err(|e| e.to_string())?;
        }
        packs.extend(found);
    }
    let w = |out: &mut dyn Write, t: String| writeln!(out, "{t}").map_err(|e| e.to_string());
    let mut reports = Vec::new();
    if !json {
        w(out, format!("{:40} {:>7} {:>6} {:>8} {:>7} {:>6} {:>8}  {}", "pack", "result", "ok", "failed", "modules", "valid", "ms", "dimensions/DH"))?;
    }
    for p in &packs {
        let r = validate_pack(p, &settings, top);
        if !json {
            let dims: Vec<String> = r.dimensions.iter().zip(&r.dh).map(|(d, h)| format!("{d}:{h}")).collect();
            w(out, format!(
                "{:40} {:>7} {:>6} {:>8} {:>7} {:>6} {:>8.0}  {}",
                truncate(&r.pack, 40),
                if r.passed() { "PASS" } else { "FAIL" },
                r.programs_ok,
                r.programs_failed.len(),
                r.modules,
                r.modules_validated,
                r.elapsed_ms,
                dims.join(" ")
            ))?;
            if let Some(e) = &r.open_error {
                w(out, format!("    cannot open: {e}"))?;
            }
            if !r.errors_by_code.is_empty() {
                let codes: Vec<String> = r.errors_by_code.iter().map(|(k, v)| format!("{k}×{v}")).collect();
                w(out, format!("    errors: {}", codes.join(" ")))?;
            }
            for f in r.programs_failed.iter().take(top) {
                w(out, format!("    failed: {f}"))?;
            }
            for m in &r.top_failures {
                w(out, format!("    {}", truncate(m, 300)))?;
            }
        }
        reports.push(r);
    }
    let passed = reports.iter().filter(|r| r.passed()).count();
    let ok: usize = reports.iter().map(|r| r.programs_ok).sum();
    let failed: usize = reports.iter().map(|r| r.programs_failed.len()).sum();
    if json {
        w(out, serde_json::to_string_pretty(&reports).map_err(|e| e.to_string())?)?;
    } else {
        let rate = if ok + failed == 0 { 100.0 } else { 100.0 * ok as f64 / (ok + failed) as f64 };
        w(out, format!("{passed}/{} packs pass; programs {ok} ok, {failed} failed ({rate:.2}%)", reports.len()))?;
    }
    Ok(if passed == reports.len() && !reports.is_empty() { 0 } else { 1 })
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

// ---------------------------------------------------------------------------------------
// options
// ---------------------------------------------------------------------------------------

fn cmd_options(path: &Path, json: bool, lang: &str, out: &mut dyn Write) -> CmdResult {
    let pack = open_pack(path)?;
    let settings = CompileSettings {
        language: lang.to_string(),
        dimension_filter: Some(Vec::new()),
        ..Default::default()
    };
    // Options only: compile no folder.
    let c = sb_pipeline::compile_pack(&pack, &settings);
    let model = &c.pack.options;
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(model).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let w = |out: &mut dyn Write, t: String| writeln!(out, "{t}").map_err(|e| e.to_string());
    w(out, format!("{} options", model.options.len()))?;
    for o in &model.options {
        let label = model.lang.get(&format!("option.{}", o.name)).map(|l| format!("  \"{l}\"")).unwrap_or_default();
        let allowed = if o.allowed.is_empty() { String::new() } else { format!(" [{}]", o.allowed.join(" ")) };
        let current = if o.value != o.default { format!(" (current {})", o.value) } else { String::new() };
        w(out, format!("  {} = {}{allowed}{current}  {:?} {}:{}{label}", o.name, o.default, o.kind, o.file, o.line))?;
    }
    let entry = |e: &ScreenEntry| match e {
        ScreenEntry::Option(n) => n.clone(),
        ScreenEntry::Screen(n) => format!("[{n}]"),
        ScreenEntry::Profile => "<profile>".into(),
        ScreenEntry::Empty => "<empty>".into(),
        ScreenEntry::Rest => "*".into(),
    };
    w(out, format!("screen: {}", model.main_screen.iter().map(entry).collect::<Vec<_>>().join(" ")))?;
    for (name, s) in &model.screens {
        w(out, format!("screen.{name}: {}", s.entries.iter().map(entry).collect::<Vec<_>>().join(" ")))?;
    }
    if !model.sliders.is_empty() {
        w(out, format!("sliders: {}", model.sliders.join(" ")))?;
    }
    for (name, p) in &model.profiles {
        let marker = if model.current_profile.as_deref() == Some(name.as_str()) { " (current)" } else { "" };
        let settings: Vec<String> = p.iter().map(|(k, v)| format!("{k}={v}")).collect();
        w(out, format!("profile.{name}{marker}: {}", settings.join(" ")))?;
    }
    Ok(0)
}

// ---------------------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------------------

/// Arguments of `render`.
#[derive(Debug, Clone)]
pub struct RenderArgs {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub time: Option<i64>,
    pub dim: String,
    pub depth: DepthArg,
    pub validation: bool,
    pub capture: Option<PathBuf>,
    pub cpu: bool,
    pub set: Vec<String>,
    pub no_dh: bool,
}

fn cmd_render(path: &Path, a: &RenderArgs, out: &mut dyn Write) -> CmdResult {
    let pack = open_pack(path)?;
    let runtime = sb_runtime::RuntimeOptions { validation: a.validation, prefer_cpu_device: a.cpu, device_name_filter: None };
    // Compile for the device that renders (comparison samplers, limits).
    let caps = sb_runtime::Runtime::new(&runtime).map_err(|e| format!("Vulkan: {e}"))?.device_info();
    let w = |out: &mut dyn Write, t: String| writeln!(out, "{t}").map_err(|e| e.to_string());
    w(out, format!("device: {} ({})", caps.name, caps.device_type))?;
    let mut settings = CompileSettings::default();
    settings.env.device = caps.caps.clone();
    settings.env.depth_mode = a.depth.into();
    settings.env.targets = vec![OutputTarget::Vulkan];
    settings.env.distant_horizons = !a.no_dh;
    settings.option_values = parse_sets(&a.set)?;
    let c = sb_pipeline::compile_pack(&pack, &settings);
    print_summary(out, &c)?;
    let mut scene = sb_runtime::SceneParams::default();
    if let Some(t) = a.time {
        scene.world_time = t;
    }
    if a.no_dh {
        scene.dh_render_distance = 0;
    }
    let dimension = if c.pack.dimensions.iter().any(|d| d.folder == a.dim) {
        a.dim.clone()
    } else if c.pack.dimensions.iter().any(|d| d.folder.is_empty()) {
        String::new()
    } else {
        c.pack.dimensions.first().map(|d| d.folder.clone()).unwrap_or_default()
    };
    let png = sb_runtime::PngRenderSettings {
        output: a.output.clone(),
        width: a.width,
        height: a.height,
        frames: a.frames,
        dimension,
        scene,
        depth_mode: Some(a.depth.into()),
        runtime,
        capture_dir: a.capture.clone(),
    };
    let textures = |p: &str| pack.read_bytes(p);
    let r = sb_runtime::render_to_png(&c.pack, &c.blobs, &textures, &png).map_err(|e| format!("render failed: {e}"))?;
    let s = &r.stats;
    w(out, format!(
        "rendered {} frames: {} passes, {} draws, {} dispatches; {} programs skipped, {} geometry types without program; validation: {} errors, {} warnings",
        s.frames,
        s.passes_run,
        s.draws,
        s.dispatches,
        s.programs_skipped.len(),
        s.geometry_skipped.len(),
        s.validation_errors,
        s.validation_warnings
    ))?;
    for p in &s.programs_skipped {
        w(out, format!("  skipped {}: {}", p.name, p.reason))?;
    }
    for m in s.warnings.iter().take(20) {
        w(out, format!("  warning: {m}"))?;
    }
    for m in r.validation_messages.iter().take(20) {
        w(out, format!("  {}", truncate(m, 400)))?;
    }
    w(out, format!("wrote {}", a.output.display()))?;
    Ok(if s.validation_errors == 0 { 0 } else { 1 })
}

// ---------------------------------------------------------------------------------------
// profiles
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
struct ProfileRow<'a> {
    name: &'a str,
    description: &'a str,
    fullscreen: bool,
    world_space: bool,
    inputs: Vec<String>,
    blocks: Vec<&'a str>,
    samplers: Vec<String>,
    push_constants: &'a str,
}

fn cmd_profiles(json: bool, out: &mut dyn Write) -> CmdResult {
    let rows: Vec<ProfileRow> = sb_transform::builtin_profiles()
        .iter()
        .map(|p| ProfileRow {
            name: &p.name,
            description: &p.description,
            fullscreen: p.fullscreen,
            world_space: p.world_space,
            inputs: p.inputs.iter().map(|i| format!("{} {}@{}", i.ty, i.name, i.location)).collect(),
            blocks: p.blocks.iter().map(|b| b.name.as_str()).collect(),
            samplers: p.samplers.iter().map(|s| format!("{}→[{}]", s.name, s.provides.join(","))).collect(),
            push_constants: &p.push_constants,
        })
        .collect();
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        return Ok(0);
    }
    for r in &rows {
        let first = r.description.split(". ").next().unwrap_or("");
        writeln!(out, "{:28} {}", r.name, truncate(first, 100)).map_err(|e| e.to_string())?;
        if !r.inputs.is_empty() {
            writeln!(out, "    inputs: {}", r.inputs.join(", ")).map_err(|e| e.to_string())?;
        }
        if !r.blocks.is_empty() || !r.samplers.is_empty() {
            writeln!(out, "    blocks: {}  samplers: {}", r.blocks.join(", "), r.samplers.join(", ")).map_err(|e| e.to_string())?;
        }
        if !r.push_constants.is_empty() {
            writeln!(out, "    push constants: {}", r.push_constants).map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}

/// Model statistics used by tests and tools: stages per program kind.
pub fn stage_counts(pack: &CompiledPack) -> IndexMap<ShaderStage, usize> {
    let mut m = IndexMap::new();
    for d in &pack.dimensions {
        for p in &d.programs {
            for s in &p.stages {
                *m.entry(s.stage).or_default() += 1;
            }
        }
    }
    m
}
