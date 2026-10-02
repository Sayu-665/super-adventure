//! The preprocessing engine: reads the include-expanded physical lines,
//! handles directives and conditionals, expands macros (Prosser's hide-set
//! algorithm) and writes the output text plus its line map.
//!
//! Output invariant: every output line ends with `\n` and has exactly one
//! line-map entry. Each physical input line produces exactly one output line,
//! except that a macro invocation whose arguments span several lines is
//! emitted on its first line, followed by blank lines for the lines it consumed.

use std::rc::Rc;

use sb_core::{Diagnostic, Diagnostics, Severity, SourceLocation};

use crate::escape::{ESCAPE_PREFIX, ReservedClass, candidates, must_escape};
use crate::expr;
use crate::intern::{FxHashMap, Interner, Sym, known};
use crate::lexer::{F_MACRO, F_NOEXPAND, Kind, LexState, Tok, lex_line, lex_single, would_paste};
use crate::macros::{BodyItem, HideSets, Macro, MacroKind, parse_body, parse_define};
use crate::source::FileData;
use crate::{ExtensionDirective, Profile, VersionDirective};

/// Maximum number of tokens produced by macro substitution in one run.
const MAX_EXPANSION_TOKENS: u64 = 1 << 23;
/// Maximum nesting of argument pre-expansion / `#if` expansion.
const MAX_NESTING: u32 = 200;
/// Failed invocations after which error recovery stops re-scanning arguments.
const MAX_CALL_FAILURES: u32 = 64;
/// Output size limit (bytes) after which macro expansion stops.
const MAX_OUTPUT_BYTES: usize = 256 << 20;

/// One physical line of the include-expanded translation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysLine {
    pub file: u32,
    /// 0-based line index in the file.
    pub line: u32,
    /// Include instance (for `#line` offsets).
    pub frame: u32,
}

/// Engine configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineConfig {
    pub keep_comments: bool,
    pub escape: bool,
    /// Properties-file mode (Iris `PropertiesPreprocessor` semantics).
    pub properties: bool,
    /// Insert spaces where adjacent macro-produced tokens would otherwise merge.
    pub avoid_paste: bool,
}

/// Engine result. `line_map` holds `(file index, line)` pairs.
pub(crate) struct EngineOutput {
    pub code: String,
    pub line_map: Vec<(u32, u32)>,
    pub version: Option<VersionDirective>,
    pub extensions: Vec<ExtensionDirective>,
    pub pragmas: Vec<String>,
    pub diagnostics: Diagnostics,
}

struct LogicalLine {
    /// First physical line (index into `lines`).
    first: u32,
    /// One past the last physical line.
    end: u32,
    toks: Vec<Tok>,
    directive: bool,
}

struct Cond {
    parent_active: bool,
    active: bool,
    taken: bool,
    saw_else: bool,
    phys: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PullMode {
    /// Looking for the `(` of a function-like macro: stop at directives.
    Peek,
    /// Collecting arguments: process directives in between (GCC behaviour).
    Args,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    If,
    Ifdef,
    Ifndef,
    Elif,
    Else,
    Endif,
    Define,
    Undef,
    Error,
    Warning,
    Line,
    Pragma,
    Version,
    Extension,
    Include,
    Unknown,
}

fn classify(name: &str) -> Dir {
    match name {
        "if" => Dir::If,
        "ifdef" => Dir::Ifdef,
        "ifndef" => Dir::Ifndef,
        "elif" => Dir::Elif,
        "else" => Dir::Else,
        "endif" => Dir::Endif,
        "define" => Dir::Define,
        "undef" => Dir::Undef,
        "error" => Dir::Error,
        "warning" => Dir::Warning,
        "line" => Dir::Line,
        "pragma" => Dir::Pragma,
        "version" => Dir::Version,
        "extension" => Dir::Extension,
        "include" | "include_next" | "import" => Dir::Include,
        _ => Dir::Unknown,
    }
}

struct Args {
    args: Vec<Vec<Tok>>,
    rparen_hs: u32,
    raw: Vec<Tok>,
}

pub(crate) struct Engine<'e> {
    files: &'e [FileData],
    int: &'e mut Interner,
    lines: &'e [PhysLine],
    frame_offsets: Vec<i64>,
    cfg: EngineConfig,

    // Line reading.
    pos: usize,
    lex_state: LexState,
    bol_carry: bool,
    lookahead: Option<LogicalLine>,
    buf_pool: Vec<Vec<Tok>>,
    map_buf: Vec<(u32, u32)>,

    conds: Vec<Cond>,

    // Macros.
    macros: Vec<Option<Rc<Macro>>>,
    hs: HideSets,
    budget: u64,
    /// Next `__COUNTER__` value.
    counter: u64,
    /// A stray `#`/`##` in program text was already reported.
    stray_hash_reported: bool,
    expansion_disabled: bool,
    depth: u32,
    stack: Vec<Tok>,
    defined_warning: Option<String>,
    call_failures: u32,

    // Output.
    out: String,
    line_map: Vec<(u32, u32)>,
    cur_phys: u32,
    pending: Vec<(u32, u32)>,
    out_in_comment: bool,
    last: Option<(Kind, Sym, bool)>,
    /// A macro was expanded since the last emitted token (paste avoidance).
    boundary: bool,
    layout_after: bool,
    layout_depth: u32,
    reserved: FxHashMap<Sym, ReservedClass>,
    reserved_hits: Vec<(usize, ReservedClass)>,

    // Results.
    version: Option<VersionDirective>,
    extensions: Vec<ExtensionDirective>,
    pragmas: Vec<String>,
    diags: Diagnostics,
}

impl<'e> Engine<'e> {
    pub(crate) fn new(
        files: &'e [FileData],
        int: &'e mut Interner,
        lines: &'e [PhysLine],
        frames: usize,
        cfg: EngineConfig,
    ) -> Self {
        let mut reserved = FxHashMap::default();
        if cfg.escape {
            for (w, c) in candidates() {
                reserved.insert(int.intern(w), c);
            }
        }
        let est_out = lines.len() * 48;
        Engine {
            files,
            macros: vec![None; int.len() + 64],
            int,
            lines,
            frame_offsets: vec![0; frames.max(1)],
            cfg,
            pos: 0,
            lex_state: LexState::Normal,
            bol_carry: false,
            lookahead: None,
            buf_pool: Vec::new(),
            map_buf: Vec::new(),
            conds: Vec::new(),
            hs: HideSets::new(),
            budget: 0,
            counter: 0,
            stray_hash_reported: false,
            expansion_disabled: false,
            depth: 0,
            stack: Vec::new(),
            defined_warning: None,
            call_failures: 0,
            out: String::with_capacity(est_out),
            line_map: Vec::with_capacity(lines.len()),
            cur_phys: 0,
            pending: Vec::new(),
            out_in_comment: false,
            last: None,
            boundary: false,
            layout_after: false,
            layout_depth: 0,
            reserved,
            reserved_hits: Vec::new(),
            version: None,
            extensions: Vec::new(),
            pragmas: Vec::new(),
            diags: Diagnostics::new(),
        }
    }

    // ------------------------------------------------------------------
    // Locations and diagnostics
    // ------------------------------------------------------------------

    fn map_phys(&self, p: u32) -> (u32, u32) {
        let idx = (p as usize).min(self.lines.len().saturating_sub(1));
        let Some(pl) = self.lines.get(idx) else {
            return (0, 1);
        };
        let base = self
            .files
            .get(pl.file as usize)
            .map_or(pl.line + 1, |f| f.original_line(pl.line as usize));
        let off = self
            .frame_offsets
            .get(pl.frame as usize)
            .copied()
            .unwrap_or(0);
        let line = (i64::from(base) + off).clamp(1, i64::from(u32::MAX)) as u32;
        (pl.file, line)
    }

    fn location(&self, p: u32) -> Option<SourceLocation> {
        if self.lines.is_empty() {
            return None;
        }
        let (f, l) = self.map_phys(p);
        self.files
            .get(f as usize)
            .map(|fd| SourceLocation::new(fd.path.clone(), l))
    }

    fn diag(&mut self, severity: Severity, code: &str, message: impl Into<String>, p: u32) {
        let loc = self.location(p);
        self.diags
            .push(Diagnostic::new(severity, code, message).at_opt(loc));
    }

    fn text(&self, s: Sym) -> &str {
        self.int.get(s)
    }

    // ------------------------------------------------------------------
    // Macro table
    // ------------------------------------------------------------------

    fn macro_of(&self, s: Sym) -> Option<Rc<Macro>> {
        self.macros.get(s.idx()).and_then(Clone::clone)
    }

    fn is_defined(&self, s: Sym) -> bool {
        self.macros.get(s.idx()).is_some_and(Option::is_some)
    }

    fn set_macro(&mut self, name: Sym, m: Macro, at: Option<u32>) {
        let idx = name.idx();
        if self.macros.len() <= idx {
            self.macros.resize(idx + 64, None);
        }
        if let (Some(p), Some(old)) = (at, &self.macros[idx])
            && !old.same_as(&m)
        {
            let msg = format!(
                "macro '{}' redefined with a different definition",
                self.text(name)
            );
            self.diag(Severity::Info, "pp.macro-redefined", msg, p);
        }
        self.macros[idx] = Some(Rc::new(m));
    }

    fn undef(&mut self, name: Sym) {
        if let Some(slot) = self.macros.get_mut(name.idx()) {
            *slot = None;
        }
    }

    /// Define `__LINE__`, `__FILE__`, `__COUNTER__` (JCPP predefines it, so
    /// Iris has it) and (GLSL mode) `__VERSION__`.
    pub(crate) fn define_builtins(&mut self, glsl: bool) {
        self.set_macro(known::LINE, Macro::builtin(MacroKind::Line), None);
        self.set_macro(known::FILE, Macro::builtin(MacroKind::File), None);
        let counter = self.int.intern("__COUNTER__");
        self.set_macro(counter, Macro::builtin(MacroKind::Counter), None);
        if glsl {
            self.set_macro(known::VERSION, Macro::builtin(MacroKind::Version), None);
        }
    }

    /// Define an object-like macro from the environment (`-D name=value`).
    pub(crate) fn define_user(&mut self, name: &str, value: &str) {
        let name_tok = lex_single(name, self.int)
            .filter(|t| t.kind == Kind::Ident && self.text(t.sym) == name);
        let Some(name_tok) = name_tok.filter(|t| t.sym != known::DEFINED) else {
            self.diags.push(Diagnostic::warning(
                "pp.bad-define",
                format!("ignoring invalid macro name '{name}' in defines"),
            ));
            return;
        };
        let value = value.replace(['\n', '\r'], " ");
        let mut toks = Vec::new();
        lex_line(&value, LexState::Normal, self.int, &mut toks);
        match parse_body(&toks, &[], false) {
            Ok(body) => self.set_macro(
                name_tok.sym,
                Macro {
                    kind: MacroKind::Object,
                    params: Vec::new(),
                    body,
                },
                None,
            ),
            Err(e) => self.diags.push(Diagnostic::warning(
                "pp.bad-define",
                format!("ignoring define '{name}': {e}"),
            )),
        }
    }

    fn define_flag(&mut self, name: &str) {
        let s = self.int.intern(name);
        if !self.is_defined(s) {
            let one = Tok::new(Kind::Number, known::ONE);
            self.set_macro(
                s,
                Macro {
                    kind: MacroKind::Object,
                    params: Vec::new(),
                    body: vec![BodyItem::Tok(one)],
                },
                None,
            );
        }
    }

    // ------------------------------------------------------------------
    // Reading lines
    // ------------------------------------------------------------------

    fn take_buf(&mut self) -> Vec<Tok> {
        self.buf_pool.pop().unwrap_or_default()
    }

    fn give_buf(&mut self, mut v: Vec<Tok>) {
        if self.buf_pool.len() < 32 {
            v.clear();
            self.buf_pool.push(v);
        }
    }

    /// Lex the logical line starting at physical line `pos` in `state`.
    /// Returns the end state and the number of physical lines consumed.
    fn lex_at(&mut self, pos: usize, state: LexState, out: &mut Vec<Tok>) -> (LexState, usize) {
        let files = self.files;
        let Some(&pl) = self.lines.get(pos) else {
            return (state, 1);
        };
        let Some(f) = files.get(pl.file as usize) else {
            return (state, 1);
        };
        if let Some(ll) = f.lex.get(pl.line as usize)
            && ll.span > 0
            && ll.start == state
            && (1..ll.span).all(|d| {
                self.lines.get(pos + d as usize).is_some_and(|n| {
                    n.file == pl.file && n.line == pl.line + d && n.frame == pl.frame
                })
            })
        {
            out.extend_from_slice(&f.toks[ll.tok_start as usize..ll.tok_end as usize]);
            return (ll.end, ll.span as usize);
        }
        // Lex on the fly (state differs from the file's natural lexing, or a
        // splice crosses an include boundary).
        let first = f.line(pl.line as usize);
        if !first.ends_with('\\') {
            return (lex_line(first, state, self.int, out), 1);
        }
        let mut joined = String::new();
        let mut n = 0;
        while let Some(p) = self.lines.get(pos + n) {
            let t = files
                .get(p.file as usize)
                .map_or("", |f| f.line(p.line as usize));
            n += 1;
            match t.strip_suffix('\\') {
                Some(s) => joined.push_str(s),
                None => {
                    joined.push_str(t);
                    break;
                }
            }
        }
        (lex_line(&joined, state, self.int, out), n.max(1))
    }

    fn read_logical_line(&mut self) -> Option<LogicalLine> {
        if let Some(l) = self.lookahead.take() {
            return Some(l);
        }
        if self.pos >= self.lines.len() {
            return None;
        }
        let first = self.pos;
        let mut toks = self.take_buf();
        let bol = self.lex_state == LexState::Normal || self.bol_carry;
        let (mut st, n) = self.lex_at(first, self.lex_state, &mut toks);
        self.pos += n;
        let directive = bol
            && toks
                .iter()
                .find(|t| !t.kind.is_white())
                .is_some_and(|t| t.is_punct(known::HASH));
        if directive {
            // A block comment opened on a directive line continues the directive.
            while st == LexState::InComment && self.pos < self.lines.len() {
                let (s2, n2) = self.lex_at(self.pos, st, &mut toks);
                self.pos += n2;
                st = s2;
            }
        }
        self.bol_carry = st == LexState::InComment && bol && toks.iter().all(|t| t.kind.is_white());
        self.lex_state = st;
        Some(LogicalLine {
            first: first as u32,
            end: self.pos as u32,
            toks,
            directive,
        })
    }

    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.active)
    }

    // ------------------------------------------------------------------
    // Main loop
    // ------------------------------------------------------------------

    pub(crate) fn run(mut self) -> EngineOutput {
        while let Some(ll) = self.read_logical_line() {
            if ll.directive {
                // Map the directive's own lines before it runs (`#line` changes the mapping).
                let maps = self.map_range(&ll);
                self.directive(&ll);
                self.emit_skipped(&ll, maps);
            } else if !self.active() {
                let maps = self.map_range(&ll);
                self.emit_skipped(&ll, maps);
            } else {
                self.text_line(&ll);
            }
            self.give_buf(ll.toks);
        }
        self.finish()
    }

    fn text_line(&mut self, ll: &LogicalLine) {
        self.cur_phys = ll.first;
        for p in ll.first + 1..ll.end {
            let m = self.map_phys(p);
            self.pending.push(m);
        }
        self.stack.clear();
        self.stack.extend(ll.toks.iter().rev().copied());
        while let Some(t) = self.stack.pop() {
            if t.kind == Kind::Ident && self.try_expand(t, true) {
                self.boundary = true;
                continue;
            }
            self.emit(t);
        }
        self.end_line();
    }

    fn map_range(&mut self, ll: &LogicalLine) -> Vec<(u32, u32)> {
        let mut v = std::mem::take(&mut self.map_buf);
        v.clear();
        for p in ll.first..ll.end {
            v.push(self.map_phys(p));
        }
        v
    }

    fn emit_skipped(&mut self, ll: &LogicalLine, maps: Vec<(u32, u32)>) {
        if self.out_in_comment
            && let Some(t) = ll.toks.first().filter(|t| t.kind == Kind::CommentEnd)
        {
            // Close a comment that was opened in emitted output.
            self.out.push_str(self.int.get(t.sym));
            self.out_in_comment = false;
        }
        for &m in &maps {
            self.out.push('\n');
            self.line_map.push(m);
        }
        self.map_buf = maps;
        self.last = None;
    }

    fn end_line(&mut self) {
        self.out.push('\n');
        let m = self.map_phys(self.cur_phys);
        self.line_map.push(m);
        for m in self.pending.drain(..) {
            self.out.push('\n');
            self.line_map.push(m);
        }
        self.last = None;
        if self.out.len() > MAX_OUTPUT_BYTES && !self.expansion_disabled {
            self.expansion_disabled = true;
            let p = self.cur_phys;
            self.diag(
                Severity::Error,
                "pp.expansion-limit",
                "preprocessed output too large; macro expansion stopped",
                p,
            );
        }
    }

    fn finish(mut self) -> EngineOutput {
        let conds = std::mem::take(&mut self.conds);
        for c in conds {
            self.diag(
                Severity::Warning,
                "pp.unterminated-conditional",
                "unterminated conditional directive (missing #endif)",
                c.phys,
            );
        }
        if self.lex_state == LexState::InComment {
            let p = self.lines.len().saturating_sub(1) as u32;
            self.diag(
                Severity::Warning,
                "pp.unterminated-comment",
                "unterminated /* comment at end of file",
                p,
            );
            if self.out_in_comment && self.out.ends_with('\n') {
                self.out.pop();
                self.out.push_str(" */\n");
            }
        }
        if !self.reserved_hits.is_empty() {
            let version = self.version.as_ref().map_or(110, |v| v.number);
            let mut new = String::with_capacity(
                self.out.len() + self.reserved_hits.len() * ESCAPE_PREFIX.len(),
            );
            let mut last = 0;
            for &(off, class) in &self.reserved_hits {
                if must_escape(class, version, &self.extensions) {
                    new.push_str(&self.out[last..off]);
                    new.push_str(ESCAPE_PREFIX);
                    last = off;
                }
            }
            new.push_str(&self.out[last..]);
            self.out = new;
        }
        EngineOutput {
            code: self.out,
            line_map: self.line_map,
            version: self.version,
            extensions: self.extensions,
            pragmas: self.pragmas,
            diagnostics: self.diags,
        }
    }

    // ------------------------------------------------------------------
    // Output
    // ------------------------------------------------------------------

    fn emit(&mut self, t: Tok) {
        match t.kind {
            Kind::Newline => {
                self.end_line();
                self.cur_phys = t.sym.0;
                for p in t.sym.0 + 1..t.hs {
                    let m = self.map_phys(p);
                    self.pending.push(m);
                }
            }
            Kind::Placemarker => {}
            Kind::Space => {
                self.out.push_str(self.int.get(t.sym));
                self.last = None;
            }
            Kind::Comment | Kind::CommentStart | Kind::CommentCont | Kind::CommentEnd => {
                if self.cfg.keep_comments {
                    self.out.push_str(self.int.get(t.sym));
                    match t.kind {
                        Kind::CommentStart => self.out_in_comment = true,
                        Kind::CommentEnd => self.out_in_comment = false,
                        _ => {}
                    }
                } else if t.kind != Kind::CommentCont {
                    self.out.push(' ');
                }
                self.last = None;
            }
            _ => {
                let text = self.int.get(t.sym);
                if self.cfg.avoid_paste
                    && let Some((pk, ps, pm)) = self.last
                    && (pm || self.boundary || t.flags & F_MACRO != 0)
                    && would_paste(self.int.get(ps), pk, text, t.kind)
                {
                    self.out.push(' ');
                }
                self.boundary = false;
                if t.kind == Kind::Punct
                    && (t.sym == known::HASH || t.sym == known::HASHHASH)
                    && !self.cfg.properties
                    && !self.stray_hash_reported
                {
                    self.stray_hash_reported = true;
                    let msg = format!(
                        "stray '{}' outside a preprocessing directive in program text (Iris/JCPP rejects the program, and it is not valid GLSL); further occurrences are not reported",
                        self.int.get(t.sym)
                    );
                    let p = self.cur_phys;
                    self.diag(Severity::Error, "pp.stray-hash", msg, p);
                }
                let text = self.int.get(t.sym);
                match t.kind {
                    Kind::Ident => {
                        if self.layout_depth == 0
                            && let Some(&c) = self.reserved.get(&t.sym)
                        {
                            self.reserved_hits.push((self.out.len(), c));
                        }
                        self.layout_after = t.sym == known::LAYOUT;
                    }
                    Kind::Punct if t.sym == known::LPAREN => {
                        if self.layout_after {
                            self.layout_depth = 1;
                        } else if self.layout_depth > 0 {
                            self.layout_depth += 1;
                        }
                        self.layout_after = false;
                    }
                    Kind::Punct if t.sym == known::RPAREN => {
                        self.layout_depth = self.layout_depth.saturating_sub(1);
                        self.layout_after = false;
                    }
                    _ => self.layout_after = false,
                }
                self.out.push_str(text);
                self.last = Some((t.kind, t.sym, t.flags & F_MACRO != 0));
            }
        }
    }

    // ------------------------------------------------------------------
    // Macro expansion
    // ------------------------------------------------------------------

    /// Next token for macro-invocation scanning. At top level, pulls further
    /// source lines when the current line is exhausted.
    fn next_tok(&mut self, top: bool, mode: PullMode) -> Option<Tok> {
        loop {
            if let Some(t) = self.stack.pop() {
                return Some(t);
            }
            if !top || !self.pull_line(mode) {
                return None;
            }
        }
    }

    fn pull_line(&mut self, mode: PullMode) -> bool {
        loop {
            let Some(ll) = self.read_logical_line() else {
                return false;
            };
            if ll.directive {
                if mode == PullMode::Peek {
                    self.lookahead = Some(ll);
                    return false;
                }
                self.diag(
                    Severity::Warning,
                    "pp.directive-in-macro-args",
                    "preprocessing directive inside macro arguments (processed in place, like GCC)",
                    ll.first,
                );
                let maps = self.map_range(&ll);
                self.directive(&ll);
                self.pending.extend_from_slice(&maps);
                self.map_buf = maps;
                self.give_buf(ll.toks);
                continue;
            }
            if !self.active() {
                for p in ll.first..ll.end {
                    let m = self.map_phys(p);
                    self.pending.push(m);
                }
                self.give_buf(ll.toks);
                continue;
            }
            self.stack.extend(ll.toks.iter().rev().copied());
            self.stack.push(Tok {
                sym: Sym(ll.first),
                hs: ll.end,
                kind: Kind::Newline,
                flags: 0,
            });
            self.give_buf(ll.toks);
            return true;
        }
    }

    fn consume_newline(&mut self, t: Tok) {
        for p in t.sym.0..t.hs {
            let m = self.map_phys(p);
            self.pending.push(m);
        }
    }

    /// Push tokens back so that `toks[0]` is popped next.
    fn unread(&mut self, toks: &[Tok]) {
        self.stack.extend(toks.iter().rev().copied());
    }

    fn charge(&mut self, n: usize) -> bool {
        if self.expansion_disabled {
            return false;
        }
        if self.hs.exhausted() {
            self.expansion_disabled = true;
            let p = self.cur_phys;
            self.diag(
                Severity::Error,
                "pp.expansion-limit",
                "macro expansion nested too deeply (hide-set storage limit reached, e.g. a very long chain of macros); expansion stopped",
                p,
            );
            return false;
        }
        self.budget = self.budget.saturating_add(n as u64);
        if self.budget > MAX_EXPANSION_TOKENS {
            self.expansion_disabled = true;
            let p = self.cur_phys;
            self.diag(
                Severity::Error,
                "pp.expansion-limit",
                "macro expansion produced too many tokens (runaway or exponential macro); expansion stopped",
                p,
            );
            return false;
        }
        true
    }

    /// Try to expand identifier `t` (already popped). On success the
    /// replacement is pushed onto the stack for rescanning.
    fn try_expand(&mut self, t: Tok, top: bool) -> bool {
        if self.expansion_disabled || t.flags & F_NOEXPAND != 0 {
            return false;
        }
        let Some(m) = self.macro_of(t.sym) else {
            return false;
        };
        if self.hs.contains(t.hs, t.sym) {
            return false;
        }
        match m.kind {
            MacroKind::Line => {
                let (_, line) = self.map_phys(self.cur_phys);
                let s = self.int.intern(&line.to_string());
                self.stack.push(Tok {
                    sym: s,
                    hs: t.hs,
                    kind: Kind::Number,
                    flags: F_MACRO,
                });
                true
            }
            MacroKind::File => {
                self.stack.push(Tok {
                    sym: known::ZERO,
                    hs: t.hs,
                    kind: Kind::Number,
                    flags: F_MACRO,
                });
                true
            }
            MacroKind::Counter => {
                let s = self.int.intern(&self.counter.to_string());
                self.counter += 1;
                self.stack.push(Tok {
                    sym: s,
                    hs: t.hs,
                    kind: Kind::Number,
                    flags: F_MACRO,
                });
                true
            }
            MacroKind::Version => {
                let v = self.version.as_ref().map_or(110, |v| v.number);
                let s = self.int.intern(&v.to_string());
                self.stack.push(Tok {
                    sym: s,
                    hs: t.hs,
                    kind: Kind::Number,
                    flags: F_MACRO,
                });
                true
            }
            MacroKind::Object => {
                let hs = self.hs.add(t.hs, t.sym);
                match self.substitute(&m, &[], hs) {
                    Some(v) => {
                        self.unread(&v);
                        true
                    }
                    None => false,
                }
            }
            MacroKind::Function { .. } => {
                let mut skipped: Vec<Tok> = Vec::new();
                loop {
                    match self.next_tok(top, PullMode::Peek) {
                        None => {
                            self.unread(&skipped);
                            return false;
                        }
                        Some(n) if n.kind.is_white() => skipped.push(n),
                        Some(n) if n.is_punct(known::LPAREN) => break,
                        Some(n) => {
                            self.stack.push(n);
                            self.unread(&skipped);
                            return false;
                        }
                    }
                }
                let lparen = Tok::new(Kind::Punct, known::LPAREN);
                let restore = |this: &mut Self, raw: &[Tok], skipped: &[Tok]| {
                    this.unread(raw);
                    this.stack.push(lparen);
                    this.unread(skipped);
                };
                match self.collect_args(&m, t.sym, top) {
                    Ok(a) => {
                        let base = self.hs.intersect(t.hs, a.rparen_hs);
                        let hs = self.hs.add(base, t.sym);
                        match self.substitute(&m, &a.args, hs) {
                            Some(v) => {
                                for s in skipped.iter().chain(a.raw.iter()) {
                                    if s.kind == Kind::Newline {
                                        self.consume_newline(*s);
                                    }
                                }
                                self.unread(&v);
                                true
                            }
                            None => {
                                restore(self, &a.raw, &skipped);
                                false
                            }
                        }
                    }
                    Err((mut raw, eof)) => {
                        self.call_failures += 1;
                        if eof || self.call_failures > MAX_CALL_FAILURES {
                            // Everything up to the end of input was scanned (or many calls
                            // failed already); re-collecting arguments for each later
                            // function-like name would be quadratic.
                            for r in &mut raw {
                                if r.kind == Kind::Ident
                                    && self.macro_of(r.sym).is_some_and(|m| {
                                        matches!(m.kind, MacroKind::Function { .. })
                                    })
                                {
                                    r.flags |= F_NOEXPAND;
                                }
                            }
                        }
                        restore(self, &raw, &skipped);
                        false
                    }
                }
            }
        }
    }

    /// Collect the arguments of a function-like macro invocation (the `(` has
    /// been consumed). On error, returns the raw tokens consumed and whether
    /// the input ended before the closing parenthesis.
    fn collect_args(&mut self, m: &Macro, name: Sym, top: bool) -> Result<Args, (Vec<Tok>, bool)> {
        let (nparams, variadic) = match m.kind {
            MacroKind::Function { nparams, variadic } => (nparams, variadic),
            _ => (0, false),
        };
        let mut raw: Vec<Tok> = Vec::new();
        let mut args: Vec<Vec<Tok>> = vec![Vec::new()];
        let mut depth = 0u32;
        let mut space = false;
        let rparen_hs;
        loop {
            let Some(t) = self.next_tok(top, PullMode::Args) else {
                let msg = format!(
                    "unterminated argument list invoking macro '{}'",
                    self.text(name)
                );
                let p = self.cur_phys;
                self.diag(Severity::Error, "pp.unterminated-macro-call", msg, p);
                return Err((raw, true));
            };
            raw.push(t);
            if t.kind.is_white() || t.kind == Kind::Placemarker {
                space = true;
                continue;
            }
            if t.kind == Kind::Punct {
                if t.sym == known::LPAREN {
                    depth += 1;
                } else if t.sym == known::RPAREN {
                    if depth == 0 {
                        rparen_hs = t.hs;
                        break;
                    }
                    depth -= 1;
                } else if t.sym == known::COMMA
                    && depth == 0
                    && !(variadic && args.len() == nparams)
                {
                    args.push(Vec::new());
                    space = false;
                    continue;
                }
            }
            if let Some(cur) = args.last_mut() {
                if space && !cur.is_empty() {
                    cur.push(Tok::space());
                }
                cur.push(t);
            }
            space = false;
        }
        let given = args.len();
        let ok = if nparams == 0 {
            if given == 1 && args[0].is_empty() {
                args.clear();
                true
            } else {
                false
            }
        } else if variadic {
            if given == nparams - 1 {
                args.push(Vec::new());
            }
            args.len() == nparams
        } else {
            given == nparams
        };
        if !ok {
            let given = if nparams == 0 && given == 1 && args.first().is_some_and(Vec::is_empty) {
                0
            } else {
                given
            };
            let msg = format!(
                "macro '{}' requires {}{} argument{}, but {} given",
                self.text(name),
                if variadic { "at least " } else { "" },
                if variadic { nparams - 1 } else { nparams },
                if nparams == 1 { "" } else { "s" },
                given
            );
            let p = self.cur_phys;
            self.diag(Severity::Error, "pp.macro-args", msg, p);
            return Err((raw, false));
        }
        Ok(Args {
            args,
            rparen_hs,
            raw,
        })
    }

    /// Fully macro-expand `toks` in isolation (argument pre-expansion, `#if`).
    fn expand_isolated(&mut self, toks: Vec<Tok>) -> Vec<Tok> {
        self.expand_isolated_mode(toks, false)
    }

    /// Evaluate a `defined` that surfaced during `#if` rescanning, reading its
    /// operand unexpanded from the token stack (GCC and JCPP behave the same
    /// way). Malformed operands follow JCPP's recovery: a non-identifier
    /// operand is consumed and evaluates to 0, and a missing `)` is reported
    /// but the value is kept. Problems are recorded in `defined_warning`.
    fn defined_from_stack(&mut self) -> bool {
        fn next(stack: &mut Vec<Tok>) -> Option<Tok> {
            loop {
                match stack.pop() {
                    Some(t) if t.kind.is_white() => continue,
                    other => return other,
                }
            }
        }
        let mut t = next(&mut self.stack);
        let paren = t.is_some_and(|t| t.is_punct(known::LPAREN));
        if paren {
            t = next(&mut self.stack);
        }
        let value = match t {
            Some(n) if n.kind == Kind::Ident => self.is_defined(n.sym),
            other => {
                let found = other.map_or_else(
                    || "end of line".to_string(),
                    |o| format!("'{}'", self.text(o.sym)),
                );
                let msg = format!("operator 'defined' requires an identifier, found {found}; evaluated as 0 (Iris/JCPP)");
                self.defined_warning.get_or_insert(msg);
                false
            }
        };
        if paren {
            match next(&mut self.stack) {
                Some(c) if c.is_punct(known::RPAREN) => {}
                other => {
                    if let Some(o) = other {
                        self.stack.push(o);
                    }
                    self.defined_warning
                        .get_or_insert("missing ')' after 'defined(NAME' (accepted like Iris/JCPP)".into());
                }
            }
        }
        value
    }

    fn expand_isolated_mode(&mut self, mut toks: Vec<Tok>, if_expr: bool) -> Vec<Tok> {
        if self.expansion_disabled {
            return toks;
        }
        if self.depth >= MAX_NESTING {
            self.expansion_disabled = true;
            let p = self.cur_phys;
            self.diag(
                Severity::Error,
                "pp.expansion-limit",
                "macro invocations nested too deeply; expansion stopped",
                p,
            );
            return toks;
        }
        self.depth += 1;
        toks.reverse();
        let saved = std::mem::replace(&mut self.stack, toks);
        let mut out = Vec::with_capacity(self.stack.len());
        while let Some(t) = self.stack.pop() {
            if if_expr && t.kind == Kind::Ident && t.sym == known::DEFINED {
                let v = self.defined_from_stack();
                out.push(Tok::new(
                    Kind::Number,
                    if v { known::ONE } else { known::ZERO },
                ));
                continue;
            }
            if t.kind == Kind::Ident && self.try_expand(t, false) {
                continue;
            }
            out.push(t);
        }
        self.stack = saved;
        self.depth -= 1;
        out
    }

    fn stringify(&mut self, arg: &[Tok]) -> Tok {
        let mut s = String::from("\"");
        let mut space = false;
        for t in arg {
            if t.kind.is_white() {
                space = true;
                continue;
            }
            if t.kind == Kind::Placemarker {
                continue;
            }
            if space && s.len() > 1 {
                s.push(' ');
            }
            space = false;
            let text = self.int.get(t.sym);
            if t.kind == Kind::Str {
                for c in text.chars() {
                    if c == '"' || c == '\\' {
                        s.push('\\');
                    }
                    s.push(c);
                }
            } else {
                s.push_str(text);
            }
        }
        s.push('"');
        Tok::new(Kind::Str, self.int.intern(&s))
    }

    /// `lhs ## rhs`.
    fn paste(&mut self, lhs: Tok, rhs: Tok, out: &mut Vec<Tok>) {
        if lhs.kind == Kind::Placemarker {
            out.push(rhs);
            return;
        }
        if rhs.kind == Kind::Placemarker {
            out.push(lhs);
            return;
        }
        let text = format!("{}{}", self.int.get(lhs.sym), self.int.get(rhs.sym));
        match lex_single(&text, self.int) {
            Some(t) => out.push(t),
            None => {
                let msg = format!(
                    "pasting \"{}\" and \"{}\" does not give a valid preprocessing token",
                    self.int.get(lhs.sym),
                    self.int.get(rhs.sym)
                );
                let p = self.cur_phys;
                self.diag(Severity::Warning, "pp.invalid-paste", msg, p);
                out.push(lhs);
                out.push(rhs);
            }
        }
    }

    fn append(&mut self, out: &mut Vec<Tok>, toks: &[Tok], paste: bool) {
        let Some((&first, rest)) = toks.split_first() else {
            return;
        };
        if paste && let Some(lhs) = out.pop() {
            self.paste(lhs, first, out);
            out.extend_from_slice(rest);
        } else {
            out.extend_from_slice(toks);
        }
    }

    /// Substitute arguments into the replacement list. `None` if the
    /// expansion budget is exhausted.
    fn substitute(&mut self, m: &Macro, args: &[Vec<Tok>], hs: u32) -> Option<Vec<Tok>> {
        let body = &m.body;
        let adjacent_paste = |i: usize| {
            matches!(body.get(i + 1), Some(BodyItem::Paste))
                || (i > 0 && matches!(body.get(i - 1), Some(BodyItem::Paste)))
        };
        let mut expanded: Vec<Option<Vec<Tok>>> = vec![None; args.len()];
        for (i, item) in body.iter().enumerate() {
            if let BodyItem::Param(p) = *item {
                let p = p as usize;
                if !adjacent_paste(i) && p < args.len() && expanded[p].is_none() {
                    let e = self.expand_isolated(args[p].clone());
                    expanded[p] = Some(e);
                }
            }
        }
        let size: usize = body
            .iter()
            .enumerate()
            .map(|(i, item)| match *item {
                BodyItem::Paste => 0,
                BodyItem::Tok(_) | BodyItem::Stringify(_) => 1,
                BodyItem::Param(p) => {
                    let p = p as usize;
                    if adjacent_paste(i) {
                        args.get(p).map_or(1, |a| a.len().max(1))
                    } else {
                        expanded.get(p).and_then(Option::as_ref).map_or(0, Vec::len)
                    }
                }
            })
            .sum();
        if !self.charge(size) {
            return None;
        }
        let mut out: Vec<Tok> = Vec::with_capacity(size);
        let mut paste = false;
        let empty: &[Tok] = &[];
        for (i, item) in body.iter().enumerate() {
            match *item {
                BodyItem::Paste => {
                    paste = true;
                    continue;
                }
                BodyItem::Tok(t) => self.append(&mut out, &[t], paste),
                BodyItem::Stringify(p) => {
                    let s = self.stringify(args.get(p as usize).map_or(empty, Vec::as_slice));
                    self.append(&mut out, &[s], paste);
                }
                BodyItem::Param(p) => {
                    let p = p as usize;
                    if adjacent_paste(i) {
                        let a = args.get(p).map_or(empty, Vec::as_slice);
                        // GNU extension: `, ## __VA_ARGS__` drops the comma when the
                        // variadic argument is empty and does not paste otherwise.
                        let gnu_comma = paste
                            && m.variadic()
                            && p + 1 == args.len()
                            && out.last().is_some_and(|t| t.is_punct(known::COMMA));
                        if gnu_comma {
                            if a.is_empty() {
                                out.pop();
                            } else {
                                out.extend_from_slice(a);
                            }
                        } else if a.is_empty() {
                            self.append(
                                &mut out,
                                &[Tok::new(Kind::Placemarker, known::EMPTY)],
                                paste,
                            );
                        } else {
                            self.append(&mut out, a, paste);
                        }
                    } else {
                        let e = expanded.get(p).and_then(Option::as_deref).unwrap_or(empty);
                        self.append(&mut out, e, paste);
                    }
                }
            }
            paste = false;
        }
        out.retain(|t| t.kind != Kind::Placemarker);
        for t in &mut out {
            t.hs = self.hs.union(t.hs, hs);
            t.flags |= F_MACRO;
        }
        Some(out)
    }

    // ------------------------------------------------------------------
    // Directives
    // ------------------------------------------------------------------

    fn directive(&mut self, ll: &LogicalLine) {
        let toks = &ll.toks;
        let Some(hash) = toks.iter().position(|t| !t.kind.is_white()) else {
            return;
        };
        let mut i = hash + 1;
        while i < toks.len() && toks[i].kind.is_white() {
            i += 1;
        }
        let p = ll.first;
        let Some(&name) = toks.get(i) else { return }; // null directive
        let rest = &toks[i + 1..];
        if name.kind != Kind::Ident {
            if self.active() && !self.cfg.properties {
                let msg = format!(
                    "invalid preprocessing directive '#{}' ignored",
                    self.text(name.sym)
                );
                self.diag(Severity::Warning, "pp.unknown-directive", msg, p);
            }
            return;
        }
        let dir = classify(self.text(name.sym));
        match dir {
            Dir::If | Dir::Ifdef | Dir::Ifndef => {
                let parent = self.active();
                let v = parent
                    && match dir {
                        Dir::If => self.eval_condition(rest, p),
                        // A missing/invalid operand counts as true, like JCPP.
                        Dir::Ifdef => self.ifdef(rest, p, "#ifdef").unwrap_or(true),
                        _ => self.ifdef(rest, p, "#ifndef").is_none_or(|d| !d),
                    };
                self.conds.push(Cond {
                    parent_active: parent,
                    active: v,
                    taken: !parent || v,
                    saw_else: false,
                    phys: p,
                });
            }
            Dir::Elif => {
                let Some(top) = self.conds.last() else {
                    self.diag(
                        Severity::Error,
                        "pp.unmatched-conditional",
                        "#elif without #if",
                        p,
                    );
                    return;
                };
                if top.saw_else {
                    // JCPP reports the error and leaves the state unchanged.
                    self.diag(
                        Severity::Error,
                        "pp.unmatched-conditional",
                        "#elif after #else",
                        p,
                    );
                    return;
                }
                if !top.parent_active || top.taken {
                    if let Some(top) = self.conds.last_mut() {
                        top.active = false;
                    }
                    return;
                }
                let v = self.eval_condition(rest, p);
                if let Some(top) = self.conds.last_mut() {
                    top.active = v;
                    top.taken = v;
                }
            }
            Dir::Else => {
                let Some(top) = self.conds.last_mut() else {
                    self.diag(
                        Severity::Error,
                        "pp.unmatched-conditional",
                        "#else without #if",
                        p,
                    );
                    return;
                };
                if top.saw_else {
                    // JCPP reports the error and leaves the state unchanged.
                    self.diag(
                        Severity::Error,
                        "pp.unmatched-conditional",
                        "#else after #else",
                        p,
                    );
                    return;
                }
                top.saw_else = true;
                top.active = top.parent_active && !top.taken;
                top.taken = true;
            }
            Dir::Endif => {
                if self.conds.pop().is_none() {
                    // JCPP logs this and carries on unchanged, so Iris accepts such packs.
                    self.diag(
                        Severity::Warning,
                        "pp.unmatched-conditional",
                        "#endif without #if ignored",
                        p,
                    );
                }
            }
            _ if !self.active() => {}
            Dir::Define => self.d_define(rest, p),
            Dir::Undef => self.d_undef(rest, p),
            Dir::Error | Dir::Warning => {
                let msg = self.join_text(rest);
                let msg = if msg.is_empty() {
                    format!("#{}", self.text(name.sym))
                } else {
                    msg
                };
                if dir == Dir::Error {
                    self.diag(Severity::Error, "pp.error", msg, p);
                } else {
                    self.diag(Severity::Warning, "pp.warning", msg, p);
                }
            }
            Dir::Line => {
                if !self.cfg.properties {
                    self.d_line(ll, rest);
                }
            }
            Dir::Pragma => {
                if !self.cfg.properties {
                    let text = self.join_text(rest);
                    self.pragmas.push(text);
                }
            }
            Dir::Version if !self.cfg.properties => self.d_version(rest, p),
            Dir::Extension if !self.cfg.properties => self.d_extension(rest, p),
            Dir::Include => {
                let word = self.text(name.sym).to_string();
                let (severity, msg) = if self.cfg.properties {
                    (
                        Severity::Warning,
                        "#include is not supported in .properties files; directive ignored".to_string(),
                    )
                } else if word == "import" {
                    // JCPP throws "Unknown directive" on #import, so Iris rejects the program.
                    (
                        Severity::Error,
                        "'#import' is not supported (Iris/JCPP rejects the program); directive ignored".to_string(),
                    )
                } else {
                    (
                        Severity::Warning,
                        format!(
                            "'#{word}' is not supported here (only lines starting with '#include' are resolved, as in Iris); directive ignored"
                        ),
                    )
                };
                self.diag(severity, "pp.include-unsupported", msg, p);
            }
            _ => {
                if !self.cfg.properties {
                    let msg = format!(
                        "unknown preprocessing directive '#{}' ignored",
                        self.text(name.sym)
                    );
                    self.diag(Severity::Warning, "pp.unknown-directive", msg, p);
                }
            }
        }
    }

    /// Text of directive tokens without comments, whitespace collapsed, trimmed.
    fn join_text(&self, toks: &[Tok]) -> String {
        let mut s = String::new();
        let mut space = false;
        for t in toks {
            if t.kind.is_white() {
                space = true;
                continue;
            }
            if space && !s.is_empty() {
                s.push(' ');
            }
            space = false;
            s.push_str(self.int.get(t.sym));
        }
        s
    }

    fn non_white(toks: &[Tok]) -> impl Iterator<Item = &Tok> {
        toks.iter().filter(|t| !t.kind.is_white())
    }

    /// Operand of `#ifdef`/`#ifndef`: `Some(is_defined)` or `None` on error
    /// (JCPP then treats the group as active, and so do we).
    fn ifdef(&mut self, rest: &[Tok], p: u32, what: &str) -> Option<bool> {
        let mut it = Self::non_white(rest);
        match it.next().copied() {
            Some(t) if t.kind == Kind::Ident => {
                if it.next().is_some() {
                    self.diag(
                        Severity::Warning,
                        "pp.extra-tokens",
                        format!("extra tokens at end of {what} directive"),
                        p,
                    );
                }
                Some(self.is_defined(t.sym))
            }
            Some(t) => {
                let msg = format!(
                    "{what} requires a macro name, found '{}'; treated as true (JCPP)",
                    self.text(t.sym)
                );
                self.diag(Severity::Error, "pp.bad-directive", msg, p);
                None
            }
            None => {
                self.diag(
                    Severity::Error,
                    "pp.bad-directive",
                    format!("{what} without a macro name; treated as true (JCPP)"),
                    p,
                );
                None
            }
        }
    }

    /// Replace `defined X` / `defined(X)` by 0/1. Malformed operands follow
    /// JCPP's recovery (see [`Self::defined_from_stack`]) and are reported as warnings.
    fn replace_defined(&mut self, toks: &[Tok], p: u32) -> Vec<Tok> {
        let mut out = Vec::with_capacity(toks.len());
        let mut i = 0;
        let skip = |mut j: usize| {
            while j < toks.len() && toks[j].kind.is_white() {
                j += 1;
            }
            j
        };
        while i < toks.len() {
            let t = toks[i];
            i += 1;
            if !(t.kind == Kind::Ident && t.sym == known::DEFINED) {
                out.push(t);
                continue;
            }
            let mut j = skip(i);
            let paren = toks.get(j).is_some_and(|t| t.is_punct(known::LPAREN));
            if paren {
                j = skip(j + 1);
            }
            let value = match toks.get(j) {
                Some(n) if n.kind == Kind::Ident => {
                    j += 1;
                    self.is_defined(n.sym)
                }
                other => {
                    let found = other.map_or_else(
                        || "end of line".to_string(),
                        |o| format!("'{}'", self.text(o.sym)),
                    );
                    if other.is_some() {
                        // JCPP consumes the offending token.
                        j += 1;
                    }
                    self.diag(
                        Severity::Warning,
                        "pp.bad-expression",
                        format!("operator 'defined' requires an identifier, found {found}; evaluated as 0 (Iris/JCPP)"),
                        p,
                    );
                    false
                }
            };
            if paren {
                let k = skip(j);
                if toks.get(k).is_some_and(|t| t.is_punct(known::RPAREN)) {
                    j = k + 1;
                } else {
                    self.diag(
                        Severity::Warning,
                        "pp.bad-expression",
                        "missing ')' after 'defined(NAME' (accepted like Iris/JCPP)",
                        p,
                    );
                }
            }
            out.push(Tok::new(
                Kind::Number,
                if value { known::ONE } else { known::ZERO },
            ));
            i = j;
        }
        out
    }

    fn eval_condition(&mut self, rest: &[Tok], p: u32) -> bool {
        let saved_phys = self.cur_phys;
        self.cur_phys = p;
        let r = self.eval_condition_inner(rest, p);
        self.cur_phys = saved_phys;
        r
    }

    fn eval_condition_inner(&mut self, rest: &[Tok], p: u32) -> bool {
        // `defined` in the directive itself (also inside macro arguments, which
        // is more lenient than GCC/JCPP) ...
        let pre = self.replace_defined(rest, p);
        // ... and `defined` produced by macro expansion, whose operand is read unexpanded.
        self.defined_warning = None;
        let fin = self.expand_isolated_mode(pre, true);
        if let Some(w) = self.defined_warning.take() {
            self.diag(Severity::Warning, "pp.bad-expression", w, p);
        }
        match expr::evaluate(&fin, self.int) {
            Ok(r) => {
                for e in r.errors {
                    self.diag(
                        Severity::Warning,
                        "pp.bad-expression",
                        format!("{e}; evaluated as 0"),
                        p,
                    );
                }
                for w in r.warnings {
                    self.diag(Severity::Warning, "pp.bad-number", w, p);
                }
                for e in r.overflows {
                    self.diag(Severity::Error, "pp.bad-number", e, p);
                }
                if r.used_float {
                    self.diag(
                        Severity::Warning,
                        "pp.float-in-condition",
                        "floating-point value in #if/#elif truncated to an integer (Iris/JCPP semantics)",
                        p,
                    );
                }
                if r.trailing {
                    self.diag(
                        Severity::Warning,
                        "pp.extra-tokens",
                        "extra tokens after #if/#elif expression ignored",
                        p,
                    );
                }
                r.value != 0
            }
            Err(e) => {
                let shown = self.join_text(rest);
                self.diag(
                    Severity::Warning,
                    "pp.bad-expression",
                    format!("{e} (in '{shown}'); condition is false"),
                    p,
                );
                false
            }
        }
    }

    fn d_define(&mut self, rest: &[Tok], p: u32) {
        let int: &Interner = self.int;
        let parsed = parse_define(rest, &|s| int.get(s).to_string());
        match parsed {
            Ok((name, m)) => self.set_macro(name, m, Some(p)),
            Err(e) => self.diag(
                Severity::Warning,
                "pp.bad-define",
                format!("{e}; #define ignored"),
                p,
            ),
        }
    }

    fn d_undef(&mut self, rest: &[Tok], p: u32) {
        let mut it = Self::non_white(rest);
        match it.next().copied() {
            Some(t) if t.kind == Kind::Ident => {
                if it.next().is_some() {
                    self.diag(
                        Severity::Warning,
                        "pp.extra-tokens",
                        "extra tokens at end of #undef directive",
                        p,
                    );
                }
                self.undef(t.sym);
            }
            _ => self.diag(
                Severity::Warning,
                "pp.bad-directive",
                "#undef requires a macro name; ignored",
                p,
            ),
        }
    }

    fn d_line(&mut self, ll: &LogicalLine, rest: &[Tok]) {
        let p = ll.first;
        let toks: Vec<Tok> = Self::non_white(rest).copied().collect();
        let expanded = self.expand_isolated(toks);
        let n = Self::non_white(&expanded)
            .next()
            .filter(|t| t.kind == Kind::Number)
            .and_then(|t| self.int.get(t.sym).parse::<u32>().ok())
            .filter(|n| *n > 0);
        let Some(n) = n else {
            self.diag(
                Severity::Warning,
                "pp.bad-line",
                "#line requires a positive decimal line number; ignored",
                p,
            );
            return;
        };
        let last = (ll.end.max(1) - 1) as usize;
        let Some(pl) = self.lines.get(last).copied() else {
            return;
        };
        let orig_next = i64::from(
            self.files
                .get(pl.file as usize)
                .map_or(pl.line + 1, |f| f.original_line(pl.line as usize)),
        ) + 1;
        if let Some(off) = self.frame_offsets.get_mut(pl.frame as usize) {
            *off = i64::from(n) - orig_next;
        }
    }

    fn d_version(&mut self, rest: &[Tok], p: u32) {
        let parts: Vec<Tok> = Self::non_white(rest).copied().collect();
        let number = parts
            .first()
            .filter(|t| t.kind == Kind::Number)
            .and_then(|t| self.int.get(t.sym).parse::<u32>().ok());
        let Some(number) = number else {
            self.diag(
                Severity::Error,
                "pp.bad-version",
                "#version requires a version number",
                p,
            );
            return;
        };
        let profile = match parts.get(1) {
            None => None,
            Some(t) => match self.int.get(t.sym) {
                "core" => Some(Profile::Core),
                "compatibility" => Some(Profile::Compatibility),
                "es" => Some(Profile::Es),
                other => {
                    let msg = format!("unknown #version profile '{other}' ignored");
                    self.diag(Severity::Warning, "pp.bad-version", msg, p);
                    None
                }
            },
        };
        if parts.len() > 2 {
            self.diag(
                Severity::Warning,
                "pp.extra-tokens",
                "extra tokens at end of #version directive",
                p,
            );
        }
        let v = VersionDirective { number, profile };
        match &self.version {
            None => {
                self.version = Some(v);
                let es = profile == Some(Profile::Es) || (number == 100 && profile.is_none());
                if es {
                    self.define_flag("GL_ES");
                    self.define_flag("GL_es_profile");
                } else if profile == Some(Profile::Compatibility) {
                    self.define_flag("GL_compatibility_profile");
                } else if profile == Some(Profile::Core) || number >= 150 {
                    self.define_flag("GL_core_profile");
                }
            }
            Some(old) if *old != v => {
                let msg =
                    format!("conflicting #version directives: keeping '{old}', ignoring '{v}'");
                self.diag(Severity::Warning, "pp.version-mismatch", msg, p);
            }
            Some(_) => {}
        }
    }

    fn d_extension(&mut self, rest: &[Tok], p: u32) {
        let parts: Vec<Tok> = Self::non_white(rest).copied().collect();
        let valid = parts.len() == 3
            && parts[0].kind == Kind::Ident
            && parts[1].is_punct(known::COLON)
            && parts[2].kind == Kind::Ident;
        if !valid {
            let shown = self.join_text(rest);
            self.diag(
                Severity::Warning,
                "pp.bad-extension",
                format!("malformed #extension directive '{shown}' ignored (expected '#extension name : behavior')"),
                p,
            );
            return;
        }
        let name = self.int.get(parts[0].sym).to_string();
        let behavior = self.int.get(parts[2].sym).to_string();
        if !matches!(behavior.as_str(), "require" | "enable" | "warn" | "disable") {
            let msg =
                format!("unknown #extension behavior '{behavior}' for {name}; directive ignored");
            self.diag(Severity::Warning, "pp.bad-extension", msg, p);
            return;
        }
        if behavior != "disable" && name != "all" {
            self.define_flag(&name);
        }
        self.extensions.push(ExtensionDirective { name, behavior });
    }
}
