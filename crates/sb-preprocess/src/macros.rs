//! Macro definitions, `#define` parsing and hide-sets (Prosser's algorithm).

use std::rc::Rc;

use crate::intern::{FxHashMap, Sym, known};
use crate::lexer::{Kind, Tok};

/// What kind of macro this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroKind {
    Object,
    Function {
        nparams: usize,
        variadic: bool,
    },
    /// `__LINE__`
    Line,
    /// `__FILE__` (always `0`, like glslang source-string numbers)
    File,
    /// `__VERSION__`
    Version,
    /// `__COUNTER__` (predefined by JCPP, hence by Iris): 0, 1, 2, ... per run.
    Counter,
}

/// One element of a macro replacement list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyItem {
    Tok(Tok),
    Param(u32),
    Stringify(u32),
    Paste,
}

/// A macro definition.
#[derive(Debug, Clone)]
pub(crate) struct Macro {
    pub kind: MacroKind,
    pub params: Vec<Sym>,
    pub body: Vec<BodyItem>,
}

impl Macro {
    pub(crate) fn builtin(kind: MacroKind) -> Self {
        Macro {
            kind,
            params: Vec::new(),
            body: Vec::new(),
        }
    }

    /// C's "identical redefinition" check (token kinds/spellings and whitespace placement).
    pub(crate) fn same_as(&self, other: &Macro) -> bool {
        if self.kind != other.kind
            || self.params != other.params
            || self.body.len() != other.body.len()
        {
            return false;
        }
        self.body
            .iter()
            .zip(&other.body)
            .all(|(a, b)| match (a, b) {
                (BodyItem::Tok(x), BodyItem::Tok(y)) => x.kind == y.kind && x.sym == y.sym,
                _ => a == b,
            })
    }

    pub(crate) fn variadic(&self) -> bool {
        matches!(self.kind, MacroKind::Function { variadic: true, .. })
    }
}

/// Error from [`parse_define`]: message for an error diagnostic.
pub(crate) type DefineError = String;

fn skip_white(toks: &[Tok], mut i: usize) -> usize {
    while i < toks.len() && toks[i].kind.is_white() {
        i += 1;
    }
    i
}

/// Parse the tokens following `#define` into `(name, macro)`.
///
/// `text` resolves symbols for error messages.
pub(crate) fn parse_define(
    toks: &[Tok],
    text: &dyn Fn(Sym) -> String,
) -> Result<(Sym, Macro), DefineError> {
    let mut i = skip_white(toks, 0);
    let Some(name) = toks.get(i).filter(|t| t.kind == Kind::Ident) else {
        return Err(match toks.get(i) {
            Some(t) => format!("macro name must be an identifier, found '{}'", text(t.sym)),
            None => "#define without a macro name".to_string(),
        });
    };
    if name.sym == known::DEFINED {
        return Err("'defined' cannot be used as a macro name".into());
    }
    i += 1;
    let mut params: Vec<Sym> = Vec::new();
    let mut kind = MacroKind::Object;
    if toks.get(i).is_some_and(|t| t.is_punct(known::LPAREN)) {
        i += 1;
        let mut variadic = false;
        i = skip_white(toks, i);
        if toks.get(i).is_some_and(|t| t.is_punct(known::RPAREN)) {
            i += 1;
        } else {
            loop {
                i = skip_white(toks, i);
                let Some(t) = toks.get(i) else {
                    return Err("unterminated macro parameter list".into());
                };
                if t.kind == Kind::Ident {
                    if params.contains(&t.sym) {
                        return Err(format!("duplicate macro parameter '{}'", text(t.sym)));
                    }
                    params.push(t.sym);
                    i = skip_white(toks, i + 1);
                    // GNU named variadic parameter: `name...`
                    if toks.get(i).is_some_and(|t| t.is_punct(known::ELLIPSIS)) {
                        variadic = true;
                        i = skip_white(toks, i + 1);
                        if !toks.get(i).is_some_and(|t| t.is_punct(known::RPAREN)) {
                            return Err("'...' must be the last macro parameter".into());
                        }
                        i += 1;
                        break;
                    }
                } else if t.is_punct(known::ELLIPSIS) {
                    params.push(known::VA_ARGS);
                    variadic = true;
                    i = skip_white(toks, i + 1);
                    if !toks.get(i).is_some_and(|t| t.is_punct(known::RPAREN)) {
                        return Err("'...' must be the last macro parameter".into());
                    }
                    i += 1;
                    break;
                } else {
                    return Err(format!(
                        "unexpected '{}' in macro parameter list",
                        text(t.sym)
                    ));
                }
                match toks.get(i) {
                    Some(t) if t.is_punct(known::COMMA) => i += 1,
                    Some(t) if t.is_punct(known::RPAREN) => {
                        i += 1;
                        break;
                    }
                    Some(t) => {
                        return Err(format!(
                            "unexpected '{}' in macro parameter list",
                            text(t.sym)
                        ));
                    }
                    None => return Err("unterminated macro parameter list".into()),
                }
            }
        }
        kind = MacroKind::Function {
            nparams: params.len(),
            variadic,
        };
    }
    let body = parse_body(
        &toks[i..],
        &params,
        matches!(kind, MacroKind::Function { .. }),
    )?;
    Ok((name.sym, Macro { kind, params, body }))
}

/// Convert replacement-list tokens to body items: whitespace runs collapse to
/// one space, leading/trailing whitespace is dropped, whitespace around `##`
/// is removed, parameters / stringification are resolved.
pub(crate) fn parse_body(
    toks: &[Tok],
    params: &[Sym],
    function_like: bool,
) -> Result<Vec<BodyItem>, DefineError> {
    let mut body: Vec<BodyItem> = Vec::with_capacity(toks.len());
    let mut space = false;
    let mut i = 0;
    let param_idx = |s: Sym| params.iter().position(|&p| p == s).map(|p| p as u32);
    while i < toks.len() {
        let t = toks[i];
        i += 1;
        if t.kind.is_white() {
            space = true;
            continue;
        }
        if t.is_punct(known::HASHHASH) {
            while matches!(body.last(), Some(BodyItem::Tok(s)) if s.kind == Kind::Space) {
                body.pop();
            }
            if body.is_empty() {
                return Err("'##' cannot appear at the start of a macro expansion".into());
            }
            body.push(BodyItem::Paste);
            space = false;
            continue;
        }
        let after_paste = matches!(body.last(), Some(BodyItem::Paste));
        if space && !body.is_empty() && !after_paste {
            body.push(BodyItem::Tok(Tok::space()));
        }
        space = false;
        if function_like && t.is_punct(known::HASH) {
            let j = skip_white(toks, i);
            if let Some(p) = toks
                .get(j)
                .filter(|n| n.kind == Kind::Ident)
                .and_then(|n| param_idx(n.sym))
            {
                body.push(BodyItem::Stringify(p));
                i = j + 1;
                continue;
            }
            // `#` not followed by a parameter: keep it as an ordinary token (JCPP does too).
        }
        match (t.kind, param_idx(t.sym)) {
            (Kind::Ident, Some(p)) => body.push(BodyItem::Param(p)),
            _ => body.push(BodyItem::Tok(t)),
        }
    }
    if matches!(body.last(), Some(BodyItem::Paste)) {
        return Err("'##' cannot appear at the end of a macro expansion".into());
    }
    Ok(body)
}

/// Maximum total number of symbols stored across all interned hide sets of
/// one run (64 MB). Hide sets grow with the depth of nested expansions, so a
/// long chain of macros (`#define M2 M1 + 2`, `#define M3 M2 + 3`, ...) needs
/// quadratic storage; past this budget expansion is stopped with a diagnostic
/// instead of exhausting memory.
pub(crate) const MAX_HIDESET_SYMS: usize = 1 << 24;

/// Interned, sorted hide-sets with memoized operations. Id 0 is the empty set.
pub(crate) struct HideSets {
    sets: Vec<Rc<[Sym]>>,
    lookup: FxHashMap<Rc<[Sym]>, u32>,
    add_memo: FxHashMap<(u32, Sym), u32>,
    union_memo: FxHashMap<(u32, u32), u32>,
    inter_memo: FxHashMap<(u32, u32), u32>,
    /// Total symbols stored in `sets`.
    total: usize,
    /// A set could not be stored because of [`MAX_HIDESET_SYMS`]; results are
    /// no longer reliable and the caller must stop expanding macros.
    exhausted: bool,
}

impl Default for HideSets {
    fn default() -> Self {
        Self::new()
    }
}

impl HideSets {
    pub(crate) fn new() -> Self {
        let mut h = HideSets {
            sets: Vec::new(),
            lookup: FxHashMap::default(),
            add_memo: FxHashMap::default(),
            union_memo: FxHashMap::default(),
            inter_memo: FxHashMap::default(),
            total: 0,
            exhausted: false,
        };
        h.intern(Vec::new());
        h
    }

    /// Whether the storage budget was exceeded (see [`MAX_HIDESET_SYMS`]).
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted
    }

    fn intern(&mut self, v: Vec<Sym>) -> u32 {
        if let Some(&id) = self.lookup.get(v.as_slice()) {
            return id;
        }
        if self.total + v.len() > MAX_HIDESET_SYMS {
            self.exhausted = true;
            return 0;
        }
        self.total += v.len();
        let id = self.sets.len() as u32;
        let b: Rc<[Sym]> = Rc::from(v);
        self.sets.push(Rc::clone(&b));
        self.lookup.insert(b, id);
        id
    }

    fn get(&self, id: u32) -> &[Sym] {
        self.sets.get(id as usize).map_or(&[], |s| s)
    }

    #[inline]
    pub(crate) fn contains(&self, id: u32, s: Sym) -> bool {
        id != 0 && self.get(id).binary_search(&s).is_ok()
    }

    pub(crate) fn add(&mut self, id: u32, s: Sym) -> u32 {
        if let Some(&r) = self.add_memo.get(&(id, s)) {
            return r;
        }
        let set = self.get(id);
        let r = match set.binary_search(&s) {
            Ok(_) => id,
            Err(pos) => {
                let mut v = Vec::with_capacity(set.len() + 1);
                v.extend_from_slice(&set[..pos]);
                v.push(s);
                v.extend_from_slice(&set[pos..]);
                self.intern(v)
            }
        };
        self.add_memo.insert((id, s), r);
        r
    }

    pub(crate) fn union(&mut self, a: u32, b: u32) -> u32 {
        if a == b || b == 0 {
            return a;
        }
        if a == 0 {
            return b;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&r) = self.union_memo.get(&key) {
            return r;
        }
        let (x, y) = (self.get(a), self.get(b));
        let mut v = Vec::with_capacity(x.len() + y.len());
        let (mut i, mut j) = (0, 0);
        while i < x.len() && j < y.len() {
            match x[i].cmp(&y[j]) {
                std::cmp::Ordering::Less => {
                    v.push(x[i]);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    v.push(y[j]);
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    v.push(x[i]);
                    i += 1;
                    j += 1;
                }
            }
        }
        v.extend_from_slice(&x[i..]);
        v.extend_from_slice(&y[j..]);
        let r = self.intern(v);
        self.union_memo.insert(key, r);
        r
    }

    pub(crate) fn intersect(&mut self, a: u32, b: u32) -> u32 {
        if a == b {
            return a;
        }
        if a == 0 || b == 0 {
            return 0;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&r) = self.inter_memo.get(&key) {
            return r;
        }
        let y = self.get(b);
        let v: Vec<Sym> = self
            .get(a)
            .iter()
            .copied()
            .filter(|s| y.binary_search(s).is_ok())
            .collect();
        let r = self.intern(v);
        self.inter_memo.insert(key, r);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intern::Interner;
    use crate::lexer::{LexState, lex_line};

    fn define(src: &str) -> (Interner, Result<(Sym, Macro), DefineError>) {
        let mut int = Interner::new();
        let mut toks = Vec::new();
        lex_line(src, LexState::Normal, &mut int, &mut toks);
        let names: Vec<String> = (0..int.len() as u32)
            .map(|i| int.get(Sym(i)).to_string())
            .collect();
        let r = parse_define(&toks, &|s| names.get(s.idx()).cloned().unwrap_or_default());
        (int, r)
    }

    #[test]
    fn object_like() {
        let (int, r) = define("  FOO   a   +  b  // comment");
        let (name, m) = r.expect("valid");
        assert_eq!(int.get(name), "FOO");
        assert_eq!(m.kind, MacroKind::Object);
        // a ' ' + ' ' b
        assert_eq!(m.body.len(), 5);
    }

    #[test]
    fn function_like_vs_object_with_paren() {
        let (_, r) = define("F(a, b) a ## b");
        let (_, m) = r.expect("valid");
        assert_eq!(
            m.kind,
            MacroKind::Function {
                nparams: 2,
                variadic: false
            }
        );
        assert_eq!(
            m.body,
            vec![BodyItem::Param(0), BodyItem::Paste, BodyItem::Param(1)]
        );
        let (_, r) = define("G (a) a");
        let (_, m) = r.expect("valid");
        assert_eq!(m.kind, MacroKind::Object);
    }

    #[test]
    fn variadic_and_stringify() {
        let (_, r) = define("V(fmt, ...) #fmt __VA_ARGS__");
        let (_, m) = r.expect("valid");
        assert_eq!(
            m.kind,
            MacroKind::Function {
                nparams: 2,
                variadic: true
            }
        );
        assert_eq!(m.body[0], BodyItem::Stringify(0));
        assert_eq!(m.body[2], BodyItem::Param(1));
        let (_, r) = define("N(args...) args");
        let (_, m) = r.expect("valid");
        assert!(m.variadic());
        let (_, r) = define("E() x");
        assert_eq!(
            r.expect("valid").1.kind,
            MacroKind::Function {
                nparams: 0,
                variadic: false
            }
        );
    }

    #[test]
    fn define_errors() {
        assert!(define("").1.is_err());
        assert!(define("123").1.is_err());
        assert!(define("defined 1").1.is_err());
        assert!(define("F(a, a) a").1.is_err());
        assert!(define("F(a").1.is_err());
        assert!(define("F(a,) a").1.is_err());
        assert!(define("F(..., a) a").1.is_err());
        assert!(define("F(a) ## a").1.is_err());
        assert!(define("F(a) a ##").1.is_err());
        assert!(define("X ## y").1.is_err());
    }

    #[test]
    fn hidesets() {
        let mut h = HideSets::new();
        let (a, b, c) = (Sym(10), Sym(20), Sym(30));
        let s1 = h.add(0, b);
        let s2 = h.add(s1, a);
        let sa = h.add(0, a);
        let s3 = h.add(sa, b);
        assert_eq!(s2, s3, "sets are interned independent of insertion order");
        assert!(h.contains(s2, a) && h.contains(s2, b) && !h.contains(s2, c));
        let s4 = h.add(0, c);
        let u = h.union(s2, s4);
        assert!(h.contains(u, a) && h.contains(u, c));
        let i = h.intersect(u, s4);
        assert_eq!(i, s4);
        assert_eq!(h.intersect(s1, s4), 0);
        assert_eq!(h.union(0, s4), s4);
        assert!(!h.contains(0, a));
        assert!(!h.exhausted());
    }

    /// Regression (review): a long macro chain used to need quadratic hide-set
    /// memory without bound (OOM); storage is now capped.
    #[test]
    fn hideset_storage_is_bounded() {
        let mut h = HideSets::new();
        let mut set = 0;
        let mut i = 0u32;
        while !h.exhausted() {
            set = h.add(set, Sym(i));
            i += 1;
            assert!(i < 100_000, "budget never reached");
        }
        assert!(h.total <= MAX_HIDESET_SYMS);
    }
}
