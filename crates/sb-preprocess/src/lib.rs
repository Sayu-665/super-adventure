//! # sb-preprocess
//!
//! A GLSL preprocessor that reproduces how Iris (via JCPP) preprocesses
//! OptiFine/Iris shader packs, plus Iris's `.properties` preprocessing mode.
//!
//! ## GLSL mode ([`Preprocessor::preprocess`])
//!
//! 1. **Includes first, unconditionally.** Every line whose trimmed text
//!    starts with `#include` is replaced by the lines of the included file
//!    before anything else happens (Iris semantics: include guards cannot
//!    prevent cycles, and includes inside `#if 0` or comments are still
//!    expanded). Paths resolve with [`sb_core::normalize_pack_path`]: a leading
//!    `/` is the `shaders/` root, otherwise the including file's directory; a
//!    `..` that climbs above the root is ignored like Iris's `AbsolutePackPath`
//!    does (`pp.include-clamped`, warning).
//!    Missing files report `pp.include-missing` (error) and expand to nothing;
//!    cycles report `pp.include-cycle` (warning) and are skipped; nesting deeper
//!    than [`PreprocessOptions::max_include_depth`] reports `pp.include-depth`.
//!    Like Iris, lines are split at every Java `\R` line break (CR, LF, CRLF,
//!    vertical tab, form feed, U+0085, U+2028, U+2029).
//! 2. **C99 preprocessing** of the result: `\`-newline splicing, comments,
//!    object/function-like macros with `#`, `##`, variadics (`__VA_ARGS__`,
//!    GNU `name...` and `, ## __VA_ARGS__`), hide-set rescanning, argument
//!    pre-expansion, invocations spanning lines, `#if`/`#elif` integer
//!    expressions with `defined`, `#error`/`#warning`, `#line`, `#pragma`,
//!    `#version`, `#extension`, and the built-ins `__LINE__`, `__FILE__` (0),
//!    `__COUNTER__` (JCPP), `__VERSION__`,
//!    `GL_core_profile`/`GL_compatibility_profile`/`GL_es_profile` (+`GL_ES`)
//!    and one macro per enabled `#extension`.
//! 3. **Output.** Active code with comments retained; directive lines and
//!    inactive lines become empty lines, so output line *i* maps to
//!    [`Preprocessed::line_map`]`[i]`. `#version`, `#extension` and `#pragma`
//!    are removed and returned separately (Iris hoists active `#version` and
//!    `#extension` lines to the top).
//! 4. **Reserved-word escaping**: identifiers that lenient drivers accept but
//!    strict compilers reject are renamed to `sb_kw_<name>` (see [`escape`] for
//!    the exact, version-gated lists).
//!
//! JCPP compatibility details that are mirrored deliberately: floating-point
//! literals in `#if` are truncated to integers (hex floats shift by their
//! binary exponent), unknown identifiers (including `true`) are 0, `08`/`09`
//! are decimal, there are no binary literals, character constants hold exactly
//! one character, `defined` with a malformed operand evaluates to 0 and a
//! missing `)` after `defined(X` is tolerated, a `\` must directly precede the
//! newline to splice, NUL characters are dropped. Every corpus program and
//! `.properties` file preprocesses to the same result as on Iris; the
//! `jcpp_differential` test checks this against the real JCPP library.
//! Deviations from Iris (all more lenient or more diagnostic, never changing
//! the result of valid code): `#include <x>` and trailing comments on include
//! lines are accepted; directives inside macro arguments are processed (GCC
//! behaviour) with a warning; `GL_*` profile and extension macros and
//! `__VERSION__` are defined (JCPP leaves them undefined); `#line` is honoured;
//! only the first `#version` is kept; spaces keep macro-separated tokens from
//! merging (Iris can produce `x--1` or `x//y`); stringification and hide sets
//! follow C99 where JCPP has bugs. Inputs on which JCPP throws (so Iris
//! refuses the program) are reported as errors where they can change the
//! result: `#`/`##` outside directives, `#import`, `#if` integers that
//! overflow 64 bits.
//!
//! ## Properties mode ([`preprocess_properties`])
//!
//! Mirrors Iris's `PropertiesPreprocessor` exactly, including macro expansion
//! of ordinary lines; see the function documentation.
//!
//! ## Diagnostics
//!
//! Severity policy: **Error** when Iris would refuse the pack or our output may
//! differ from what Iris produces; **Warning** when Iris tolerates the problem
//! and our output is identical to Iris's; **Info** for harmless notes. The one
//! exception is an active `#error`, which is an error by contract (JCPP only
//! logs it). Locations are original pack files and lines.
//!
//! | code | severity | meaning |
//! |------|----------|---------|
//! | `pp.file-missing` | error | entry file not found |
//! | `pp.include-missing` | error | `#include` target not found (expands to nothing) |
//! | `pp.include-malformed` | error | `#include` without a target, resolving to the root, or `#include_next`-style lines (Iris cannot resolve them either) |
//! | `pp.include-clamped` | warning | `..` above the `shaders/` root ignored, like Iris |
//! | `pp.include-depth` | error | include nesting deeper than `max_include_depth` |
//! | `pp.include-cycle` | warning | include cycle; the offending `#include` is skipped |
//! | `pp.include-unsupported` | warning (error for `#import`) | `# include` (space), `#import`, or `#include` in `.properties`; ignored |
//! | `pp.too-large` | error | include expansion exceeds 4M lines |
//! | `pp.error` / `pp.warning` | error / warning | active `#error` / `#warning` |
//! | `pp.unmatched-conditional` | error (warning for a stray `#endif`) | `#else`/`#elif`/`#endif` structure errors |
//! | `pp.unterminated-conditional` | warning | missing `#endif` at end of input |
//! | `pp.bad-expression` | warning | malformed `#if`/`#elif` expression (false), division by zero (0), malformed `defined` operand (0, like JCPP) |
//! | `pp.bad-number` | warning (error on 64-bit overflow) | `08`-style literal read as decimal (JCPP); integer literal too large (JCPP throws) |
//! | `pp.float-in-condition` | warning | float literal truncated in `#if` (JCPP semantics) |
//! | `pp.bad-directive` | error / warning | `#ifdef`/`#ifndef` without a name (treated as true, like JCPP); `#undef` without a name |
//! | `pp.bad-define` | warning | malformed `#define` (ignored) or invalid environment define |
//! | `pp.macro-redefined` | info | macro redefined with a different body |
//! | `pp.macro-args` | error | wrong number of macro arguments |
//! | `pp.unterminated-macro-call` | error | macro arguments run to end of input |
//! | `pp.invalid-paste` | warning | `##` does not form a valid token (both tokens kept) |
//! | `pp.expansion-limit` | error | runaway macro expansion stopped (token, nesting, hide-set or output budget) |
//! | `pp.stray-hash` | error | `#`/`##` in program text outside a directive (JCPP throws; invalid GLSL); reported once |
//! | `pp.directive-in-macro-args` | warning | directive inside macro arguments (processed in place) |
//! | `pp.extra-tokens` | warning | trailing tokens after a directive |
//! | `pp.unknown-directive` | warning | unknown directive in active code (dropped) |
//! | `pp.bad-version` | error / warning | `#version` without a number / with an unknown profile |
//! | `pp.version-mismatch` | warning | several different `#version` directives (first kept) |
//! | `pp.bad-extension` | warning | malformed `#extension` (dropped) |
//! | `pp.bad-line` | warning | malformed `#line` (ignored) |
//! | `pp.unterminated-comment` | warning | `/*` without `*/` (closed in the output) |
//! | `pp.reserved-marker` | error | `.properties` file contains Iris-internal markers |

mod engine;
pub mod escape;
mod expr;
mod intern;
mod lexer;
mod macros;
mod properties;
mod source;

use indexmap::IndexMap;
use sb_core::{Diagnostic, Diagnostics, SourceLocation, SourceProvider, normalize_pack_path};
use serde::{Deserialize, Serialize};

use crate::engine::{Engine, EngineConfig, PhysLine};
use crate::intern::{FxHashMap, Interner};
use crate::source::{FileData, IncludeTarget, normalize_text};

pub use crate::escape::{ESCAPE_PREFIX, would_escape};
pub use crate::properties::{iris_idmap_fixups, preprocess_properties};

/// Hard cap on the include depth regardless of [`PreprocessOptions::max_include_depth`].
const HARD_MAX_INCLUDE_DEPTH: u32 = 512;
/// Maximum number of physical lines in one include-expanded translation unit.
const MAX_TOTAL_LINES: usize = 4_000_000;

/// Options for [`Preprocessor::preprocess`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOptions {
    /// Predefined object-like macros (environment macros such as `MC_VERSION`
    /// and option macros), in definition order. `None` defines the macro with
    /// an empty replacement list (like Iris's empty-valued environment defines).
    pub defines: IndexMap<String, Option<String>>,
    /// Keep comments in active code (default `true`; Iris keeps them, and
    /// `/* RENDERTARGETS: */` style directives live in comments).
    pub keep_comments: bool,
    /// Rename reserved words used as identifiers to `sb_kw_<name>` (default `true`).
    pub escape_reserved_words: bool,
    /// Maximum `#include` nesting depth (default 64).
    pub max_include_depth: u32,
}

impl Default for PreprocessOptions {
    fn default() -> Self {
        Self {
            defines: IndexMap::new(),
            keep_comments: true,
            escape_reserved_words: true,
            max_include_depth: 64,
        }
    }
}

impl PreprocessOptions {
    /// Default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: define `name` as `value`.
    pub fn with_define(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.defines.insert(name.into(), Some(value.into()));
        self
    }

    /// Builder: define `name` with an empty replacement list.
    pub fn with_flag(mut self, name: impl Into<String>) -> Self {
        self.defines.insert(name.into(), None);
        self
    }
}

/// GLSL profile of a `#version` directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Core,
    Compatibility,
    Es,
}

impl Profile {
    /// The profile keyword (`core`, `compatibility`, `es`).
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Core => "core",
            Profile::Compatibility => "compatibility",
            Profile::Es => "es",
        }
    }
}

/// A `#version` directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersionDirective {
    pub number: u32,
    pub profile: Option<Profile>,
}

impl std::fmt::Display for VersionDirective {
    /// Formats as directive text, e.g. `#version 330 compatibility`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#version {}", self.number)?;
        if let Some(p) = self.profile {
            write!(f, " {}", p.as_str())?;
        }
        Ok(())
    }
}

/// An `#extension name : behavior` directive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtensionDirective {
    pub name: String,
    /// `require`, `enable`, `warn` or `disable`.
    pub behavior: String,
}

impl std::fmt::Display for ExtensionDirective {
    /// Formats as directive text, e.g. `#extension GL_ARB_gpu_shader5 : enable`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#extension {} : {}", self.name, self.behavior)
    }
}

/// Result of preprocessing one shader entry file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preprocessed {
    /// Expanded GLSL without `#version`/`#extension`/`#pragma` lines; comments
    /// kept in active regions; inactive regions and directives are empty lines.
    /// Every line, including the last, ends with `\n`.
    pub code: String,
    /// The first active `#version` directive.
    pub version: Option<VersionDirective>,
    /// Active `#extension` directives in source order.
    pub extensions: Vec<ExtensionDirective>,
    /// Text after `#pragma` of every active pragma.
    pub pragmas: Vec<String>,
    /// One entry per line of `code` (`code.lines().count()`), mapping it to
    /// the original file and 1-based line (adjusted by `#line`).
    pub line_map: Vec<SourceLocation>,
    /// The entry file followed by every included file, in first-inclusion order.
    pub files: Vec<String>,
    pub diagnostics: Diagnostics,
}

impl Preprocessed {
    /// The `#version` number, or 110 (the GLSL default) without a directive.
    pub fn version_number(&self) -> u32 {
        self.version.map_or(110, |v| v.number)
    }

    /// Original location of 1-based output line `line`.
    pub fn location(&self, line: usize) -> Option<&SourceLocation> {
        line.checked_sub(1).and_then(|i| self.line_map.get(i))
    }

    /// The hoisted header: the `#version` line (if any) followed by the
    /// `#extension` lines, each terminated by `\n`.
    pub fn header(&self) -> String {
        let mut s = String::new();
        if let Some(v) = &self.version {
            s.push_str(&v.to_string());
            s.push('\n');
        }
        for e in &self.extensions {
            s.push_str(&e.to_string());
            s.push('\n');
        }
        s
    }

    /// Complete GLSL like Iris produces it: [`header`](Self::header) followed by
    /// [`code`](Self::code). Note that output line numbers then shift by the
    /// number of header lines.
    pub fn to_glsl(&self) -> String {
        let mut s = self.header();
        s.push_str(&self.code);
        s
    }
}

/// GLSL preprocessor for one shader pack. Loads files through a
/// [`SourceProvider`] and caches them (text, `#include` lines and tokens), so
/// one instance should be reused for all programs of a pack.
///
/// ```
/// use sb_core::MemorySources;
/// use sb_preprocess::{Preprocessor, PreprocessOptions};
///
/// let sources = MemorySources::new()
///     .with("lib/common.glsl", "#define SCALE 2.0\n")
///     .with("composite.fsh", "#version 120\n#include \"/lib/common.glsl\"\nfloat x = SCALE;\n");
/// let mut pp = Preprocessor::new(&sources);
/// let out = pp.preprocess("composite.fsh", &PreprocessOptions::default());
/// assert!(out.diagnostics.is_empty());
/// assert_eq!(out.version_number(), 120);
/// assert_eq!(out.code, "\n\nfloat x = 2.0;\n");
/// assert_eq!(out.line_map[1].file, "lib/common.glsl");
/// assert_eq!(out.files, ["composite.fsh", "lib/common.glsl"]);
/// ```
pub struct Preprocessor<'a> {
    sources: &'a dyn SourceProvider,
    files: Vec<FileData>,
    by_path: FxHashMap<String, Option<u32>>,
    int: Interner,
}

impl std::fmt::Debug for Preprocessor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preprocessor")
            .field("cached_files", &self.by_path.len())
            .finish_non_exhaustive()
    }
}

/// Warning for an include path whose `..` segments climb above the pack root.
fn clamp_warning(resolved: &str) -> Diagnostic {
    Diagnostic::warning(
        "pp.include-clamped",
        format!(
            "include path climbs above the shaders/ root with '..'; resolved to \"{resolved}\" by ignoring the extra '..' like Iris"
        ),
    )
}

struct FlattenCtx {
    lines: Vec<PhysLine>,
    frames: u32,
    stack: Vec<u32>,
    order: Vec<u32>,
    seen: Vec<bool>,
    max_depth: u32,
    overflow: bool,
    diags: Diagnostics,
}

impl<'a> Preprocessor<'a> {
    /// Create a preprocessor reading files from `sources`.
    pub fn new(sources: &'a dyn SourceProvider) -> Self {
        Preprocessor {
            sources,
            files: Vec::new(),
            by_path: FxHashMap::default(),
            int: Interner::new(),
        }
    }

    /// Load (or fetch from the cache) a file by normalized pack path.
    fn load(&mut self, path: &str) -> Option<u32> {
        if let Some(&r) = self.by_path.get(path) {
            return r;
        }
        let r = self.sources.read(path).map(|text| {
            let fd = FileData::new(path.to_string(), normalize_text(&text), &mut self.int, true);
            self.files.push(fd);
            (self.files.len() - 1) as u32
        });
        self.by_path.insert(path.to_string(), r);
        r
    }

    fn path_of(&self, id: u32) -> &str {
        self.files.get(id as usize).map_or("", |f| f.path.as_str())
    }

    /// Preprocess the shader file `entry_path` (a path relative to the
    /// `shaders/` root, e.g. `world0/composite.fsh`).
    pub fn preprocess(&mut self, entry_path: &str, opts: &PreprocessOptions) -> Preprocessed {
        let entry = normalize_pack_path("", entry_path).unwrap_or_else(|| entry_path.to_string());
        match self.load(&entry) {
            Some(id) => self.run_glsl(id, opts),
            None => {
                let mut diagnostics = Diagnostics::new();
                diagnostics.push(Diagnostic::error(
                    "pp.file-missing",
                    format!("shader file \"{entry}\" not found"),
                ));
                Preprocessed {
                    code: String::new(),
                    version: None,
                    extensions: Vec::new(),
                    pragmas: Vec::new(),
                    line_map: Vec::new(),
                    files: vec![entry],
                    diagnostics,
                }
            }
        }
    }

    /// Preprocess in-memory source text as if it were the file `virtual_path`
    /// (includes resolve relative to it). The text is not cached.
    pub fn preprocess_source(
        &mut self,
        virtual_path: &str,
        text: &str,
        opts: &PreprocessOptions,
    ) -> Preprocessed {
        let path =
            normalize_pack_path("", virtual_path).unwrap_or_else(|| virtual_path.to_string());
        let fd = FileData::new(path, normalize_text(text), &mut self.int, true);
        self.files.push(fd);
        let id = (self.files.len() - 1) as u32;
        let out = self.run_glsl(id, opts);
        // Free the text but keep the slot so file ids stay stable.
        if let Some(slot) = self.files.get_mut(id as usize) {
            *slot = FileData::new(slot.path.clone(), String::new(), &mut self.int, false);
        }
        out
    }

    fn flatten(&mut self, file: u32, depth: u32, ctx: &mut FlattenCtx) {
        if ctx.overflow {
            return;
        }
        let frame = ctx.frames;
        ctx.frames += 1;
        let Some(fd) = self.files.get(file as usize) else {
            return;
        };
        let includes = fd.includes.clone();
        let n = fd.line_count();
        let mut inc = includes.iter().peekable();
        for k in 0..n {
            if let Some(il) = inc.next_if(|il| il.line as usize == k) {
                let loc = SourceLocation::new(self.path_of(file), k as u32 + 1);
                if let IncludeTarget::Clamped(p) = &il.target {
                    ctx.diags.push(clamp_warning(p).at(loc.clone()));
                }
                match &il.target {
                    IncludeTarget::Invalid(msg) => {
                        ctx.diags.push(Diagnostic::error("pp.include-malformed", msg.clone()).at(loc));
                    }
                    IncludeTarget::Path(p) | IncludeTarget::Clamped(p) => match self.load(p) {
                        None => ctx.diags.push(
                            Diagnostic::error("pp.include-missing", format!("included file \"{p}\" not found")).at(loc),
                        ),
                        Some(id) if ctx.stack.contains(&id) => ctx.diags.push(
                            Diagnostic::warning(
                                "pp.include-cycle",
                                format!("#include cycle: \"{p}\" is already being included; this #include is skipped"),
                            )
                            .at(loc),
                        ),
                        Some(_) if depth + 1 > ctx.max_depth => ctx.diags.push(
                            Diagnostic::error(
                                "pp.include-depth",
                                format!("#include nested more than {} levels deep; \"{p}\" not included", ctx.max_depth),
                            )
                            .at(loc),
                        ),
                        Some(id) => {
                            let idx = id as usize;
                            if ctx.seen.len() <= idx {
                                ctx.seen.resize(idx + 1, false);
                            }
                            if !ctx.seen[idx] {
                                ctx.seen[idx] = true;
                                ctx.order.push(id);
                            }
                            ctx.stack.push(id);
                            self.flatten(id, depth + 1, ctx);
                            ctx.stack.pop();
                            if ctx.overflow {
                                return;
                            }
                        }
                    },
                }
                continue;
            }
            if ctx.lines.len() >= MAX_TOTAL_LINES {
                ctx.overflow = true;
                ctx.diags.push(
                    Diagnostic::error(
                        "pp.too-large",
                        format!(
                            "include expansion exceeds {MAX_TOTAL_LINES} lines; output truncated"
                        ),
                    )
                    .at(SourceLocation::new(self.path_of(file), k as u32 + 1)),
                );
                return;
            }
            ctx.lines.push(PhysLine {
                file,
                line: k as u32,
                frame,
            });
        }
    }

    fn run_glsl(&mut self, entry: u32, opts: &PreprocessOptions) -> Preprocessed {
        let mut ctx = FlattenCtx {
            lines: Vec::new(),
            frames: 0,
            stack: vec![entry],
            order: vec![entry],
            seen: vec![false; self.files.len()],
            max_depth: opts.max_include_depth.min(HARD_MAX_INCLUDE_DEPTH),
            overflow: false,
            diags: Diagnostics::new(),
        };
        if let Some(s) = ctx.seen.get_mut(entry as usize) {
            *s = true;
        }
        self.flatten(entry, 0, &mut ctx);
        let FlattenCtx {
            lines,
            frames,
            order,
            diags: mut diagnostics,
            ..
        } = ctx;

        let cfg = EngineConfig {
            keep_comments: opts.keep_comments,
            escape: opts.escape_reserved_words,
            properties: false,
            avoid_paste: true,
        };
        let mut engine = Engine::new(&self.files, &mut self.int, &lines, frames as usize, cfg);
        engine.define_builtins(true);
        for (name, value) in &opts.defines {
            engine.define_user(name, value.as_deref().unwrap_or(""));
        }
        let out = engine.run();
        diagnostics.extend(out.diagnostics);

        let paths: Vec<&str> = self.files.iter().map(|f| f.path.as_str()).collect();
        let line_map = out
            .line_map
            .iter()
            .map(|&(f, l)| SourceLocation::new(paths.get(f as usize).copied().unwrap_or(""), l))
            .collect();
        Preprocessed {
            code: out.code,
            version: out.version,
            extensions: out.extensions,
            pragmas: out.pragmas,
            line_map,
            files: order
                .iter()
                .map(|&id| self.path_of(id).to_string())
                .collect(),
            diagnostics,
        }
    }

    /// All files reachable from `entry_path` through `#include` lines
    /// (unconditionally, Iris semantics), entry first, in depth-first
    /// pre-order. Diagnostics report missing/malformed includes (errors) and
    /// cycles (warnings). A missing entry yields an empty list and an error.
    pub fn include_closure(&mut self, entry_path: &str) -> (Vec<String>, Diagnostics) {
        let mut diags = Diagnostics::new();
        let entry = normalize_pack_path("", entry_path).unwrap_or_else(|| entry_path.to_string());
        let Some(root) = self.load(&entry) else {
            diags.push(Diagnostic::error(
                "pp.file-missing",
                format!("shader file \"{entry}\" not found"),
            ));
            return (Vec::new(), diags);
        };
        let mut order = vec![root];
        let mut visited: Vec<bool> = Vec::new();
        let mut on_stack: Vec<bool> = Vec::new();
        let mark = |v: &mut Vec<bool>, id: u32, val: bool| {
            let i = id as usize;
            if v.len() <= i {
                v.resize(i + 1, false);
            }
            v[i] = val;
        };
        let get = |v: &Vec<bool>, id: u32| v.get(id as usize).copied().unwrap_or(false);
        mark(&mut visited, root, true);
        mark(&mut on_stack, root, true);
        // Explicit DFS stack: (file, index of next include to examine).
        let mut stack: Vec<(u32, usize)> = vec![(root, 0)];
        while let Some(&mut (file, ref mut next)) = stack.last_mut() {
            let inc = self
                .files
                .get(file as usize)
                .and_then(|f| f.includes.get(*next))
                .cloned();
            let Some(il) = inc else {
                mark(&mut on_stack, file, false);
                stack.pop();
                continue;
            };
            *next += 1;
            let loc = SourceLocation::new(self.path_of(file), il.line + 1);
            if let IncludeTarget::Clamped(p) = &il.target {
                diags.push(clamp_warning(p).at(loc.clone()));
            }
            match il.target {
                IncludeTarget::Invalid(msg) => {
                    diags.push(Diagnostic::error("pp.include-malformed", msg).at(loc))
                }
                IncludeTarget::Path(p) | IncludeTarget::Clamped(p) => match self.load(&p) {
                    None => diags.push(
                        Diagnostic::error(
                            "pp.include-missing",
                            format!("included file \"{p}\" not found"),
                        )
                        .at(loc),
                    ),
                    Some(id) if get(&on_stack, id) => diags.push(
                        Diagnostic::warning(
                            "pp.include-cycle",
                            format!("#include cycle: \"{p}\" includes itself"),
                        )
                        .at(loc),
                    ),
                    Some(id) if get(&visited, id) => {}
                    Some(id) => {
                        mark(&mut visited, id, true);
                        mark(&mut on_stack, id, true);
                        order.push(id);
                        stack.push((id, 0));
                    }
                },
            }
        }
        (
            order
                .iter()
                .map(|&id| self.path_of(id).to_string())
                .collect(),
            diags,
        )
    }
}

#[cfg(test)]
mod tests;
