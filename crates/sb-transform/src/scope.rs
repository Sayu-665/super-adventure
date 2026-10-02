//! Scope-aware identifier traversal.
//!
//! [`walk_idents`] visits every identifier of a translation unit in source order and
//! tells the callback what the occurrence is (a declaration, a reference to a global or
//! a local, a function name, a type name). Callbacks may rename in place: scopes are
//! tracked with the *original* names, so renaming a declaration still classifies later
//! references to it correctly.

use std::collections::HashSet;

use crate::ast::*;

/// What an identifier occurrence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occ {
    /// Name of a global variable declaration (incl. interface/uniform declarations and
    /// instance names of blocks).
    GlobalDecl,
    /// Member of an anonymous (instance-less) global block.
    BlockMemberDecl,
    /// Name of a function definition or prototype.
    FunctionDecl,
    /// Parameter name.
    ParamDecl,
    /// Local variable declaration.
    LocalDecl,
    /// Variable reference that resolves to a global (or an undeclared name).
    GlobalRef,
    /// Variable reference that resolves to a local or parameter.
    LocalRef,
    /// Callee name of a call (function, builtin or constructor).
    Call,
    /// A type name in a type specifier (struct names, builtin types) or a struct
    /// definition's name.
    TypeName,
}

/// Visit every identifier of `unit` with its classification (see [`Occ`]).
pub(crate) fn walk_idents(unit: &mut TranslationUnit, f: &mut dyn FnMut(&mut String, Occ)) {
    for item in &mut unit.items {
        walk_item(item, f);
    }
}

/// Visit the identifiers of one item.
pub(crate) fn walk_item(item: &mut Item, f: &mut dyn FnMut(&mut String, Occ)) {
    let mut scopes = Scopes::default();
    match &mut item.kind {
        ItemKind::Function(func) => {
            proto(&mut func.proto, f, &mut scopes, true);
            for s in &mut func.body {
                stmt(s, f, &mut scopes);
            }
        }
        ItemKind::Prototype(p) => proto(p, f, &mut scopes, false),
        ItemKind::Decl(d) => {
            type_spec(&mut d.ty.ty, f, &mut scopes);
            layout_exprs(&mut d.ty.quals, f, &mut scopes);
            for v in &mut d.vars {
                dims(&mut v.array, f, &mut scopes);
                if let Some(i) = &mut v.init {
                    init(i, f, &mut scopes);
                }
                f(&mut v.name, Occ::GlobalDecl);
            }
        }
        ItemKind::Block(b) => {
            layout_exprs(&mut b.quals, f, &mut scopes);
            for field in &mut b.fields {
                type_spec(&mut field.ty, f, &mut scopes);
                for (n, d) in &mut field.names {
                    dims(d, f, &mut scopes);
                    if b.instance.is_none() {
                        f(n, Occ::BlockMemberDecl);
                    }
                }
            }
            if let Some((n, d)) = &mut b.instance {
                dims(d, f, &mut scopes);
                f(n, Occ::GlobalDecl);
            }
        }
        ItemKind::Precision(_, t) => type_spec(t, f, &mut scopes),
        ItemKind::Invariant(n) => f(n, Occ::GlobalRef),
        ItemKind::QualifierOnly(q) => layout_exprs(q, f, &mut scopes),
        ItemKind::Raw(_) => {}
    }
}

#[derive(Default)]
struct Scopes {
    stack: Vec<HashSet<String>>,
}

impl Scopes {
    fn push(&mut self) {
        self.stack.push(HashSet::new());
    }
    fn pop(&mut self) {
        self.stack.pop();
    }
    fn declare(&mut self, name: &str) {
        if let Some(top) = self.stack.last_mut() {
            top.insert(name.to_string());
        }
    }
    fn is_local(&self, name: &str) -> bool {
        self.stack.iter().any(|s| s.contains(name))
    }
}

fn layout_exprs(q: &mut [Qualifier], f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    for x in q {
        if let Qualifier::Layout(ids) = x {
            for id in ids {
                if let Some(v) = &mut id.value {
                    expr(v, f, scopes);
                }
            }
        }
    }
}

fn proto(p: &mut Prototype, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes, open_scope: bool) {
    type_spec(&mut p.ret.ty, f, scopes);
    f(&mut p.name, Occ::FunctionDecl);
    if open_scope {
        scopes.push();
    }
    for param in &mut p.params {
        type_spec(&mut param.ty, f, scopes);
        dims(&mut param.array, f, scopes);
        if let Some(n) = &mut param.name {
            let original = n.clone();
            f(n, Occ::ParamDecl);
            if open_scope {
                scopes.declare(&original);
            }
        }
    }
}

fn type_spec(t: &mut TypeSpec, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    dims(&mut t.array, f, scopes);
    match &mut t.base {
        TypeBase::Named(n) => f(n, Occ::TypeName),
        TypeBase::Struct(s) => {
            if let Some(n) = &mut s.name {
                f(n, Occ::TypeName);
            }
            for field in &mut s.fields {
                type_spec(&mut field.ty, f, scopes);
                for (_, d) in &mut field.names {
                    dims(d, f, scopes);
                }
            }
        }
    }
}

fn dims(d: &mut [ArrayDim], f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    for x in d {
        if let ArrayDim::Sized(e) = x {
            expr(e, f, scopes);
        }
    }
}

fn init(i: &mut Init, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    match i {
        Init::Expr(e) => expr(e, f, scopes),
        Init::List(l) => {
            for x in l {
                init(x, f, scopes);
            }
        }
    }
}

fn local_decl(d: &mut Declaration, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    type_spec(&mut d.ty.ty, f, scopes);
    for v in &mut d.vars {
        dims(&mut v.array, f, scopes);
        if let Some(i) = &mut v.init {
            init(i, f, scopes);
        }
        let original = v.name.clone();
        f(&mut v.name, Occ::LocalDecl);
        scopes.declare(&original);
    }
}

fn stmt(s: &mut Stmt, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    match &mut s.kind {
        StmtKind::Decl(d) => local_decl(d, f, scopes),
        StmtKind::Expr(e) | StmtKind::Case(e) => expr(e, f, scopes),
        StmtKind::Return(Some(e)) => expr(e, f, scopes),
        StmtKind::Block(b) => {
            scopes.push();
            for x in b {
                stmt(x, f, scopes);
            }
            scopes.pop();
        }
        StmtKind::If { cond, then, els } => {
            expr(cond, f, scopes);
            scopes.push();
            stmt(then, f, scopes);
            scopes.pop();
            if let Some(e) = els {
                scopes.push();
                stmt(e, f, scopes);
                scopes.pop();
            }
        }
        StmtKind::Switch { expr: e, body } => {
            expr(e, f, scopes);
            scopes.push();
            for x in body {
                stmt(x, f, scopes);
            }
            scopes.pop();
        }
        StmtKind::While { cond, body } => {
            scopes.push();
            condition(cond, f, scopes);
            stmt(body, f, scopes);
            scopes.pop();
        }
        StmtKind::DoWhile { body, cond } => {
            scopes.push();
            stmt(body, f, scopes);
            scopes.pop();
            expr(cond, f, scopes);
        }
        StmtKind::For { init, cond, step, body } => {
            scopes.push();
            if let Some(i) = init {
                stmt(i, f, scopes);
            }
            if let Some(c) = cond {
                condition(c, f, scopes);
            }
            if let Some(st) = step {
                expr(st, f, scopes);
            }
            stmt(body, f, scopes);
            scopes.pop();
        }
        StmtKind::Return(None)
        | StmtKind::Empty
        | StmtKind::Default
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Discard
        | StmtKind::Raw(_) => {}
    }
}

fn condition(c: &mut Condition, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    match c {
        Condition::Expr(e) => expr(e, f, scopes),
        Condition::Decl { ty, name, init: i } => {
            type_spec(&mut ty.ty, f, scopes);
            init(i, f, scopes);
            let original = name.clone();
            f(name, Occ::LocalDecl);
            scopes.declare(&original);
        }
    }
}

fn expr(e: &mut Expr, f: &mut dyn FnMut(&mut String, Occ), scopes: &mut Scopes) {
    match e {
        Expr::Ident(n) => {
            let occ = if scopes.is_local(n) { Occ::LocalRef } else { Occ::GlobalRef };
            f(n, occ);
        }
        Expr::Int(_) | Expr::UInt(_) | Expr::Bool(_) | Expr::Float(_) | Expr::Double(_) | Expr::Raw(_) => {}
        Expr::Unary(_, a) | Expr::PostInc(a) | Expr::PostDec(a) | Expr::Field(a, _) => expr(a, f, scopes),
        Expr::Binary(_, a, b) | Expr::Assign(a, _, b) | Expr::Index(a, b) | Expr::Comma(a, b) => {
            expr(a, f, scopes);
            expr(b, f, scopes);
        }
        Expr::Ternary(a, b, c) => {
            expr(a, f, scopes);
            expr(b, f, scopes);
            expr(c, f, scopes);
        }
        Expr::Call(callee, args) => {
            match callee {
                Callee::Name(n) => f(n, Occ::Call),
                Callee::ArrayCtor(t) => type_spec(t, f, scopes),
                Callee::Method(r, _) => expr(r, f, scopes),
            }
            for a in args {
                expr(a, f, scopes);
            }
        }
    }
}

/// Visit every expression of a function body with the set of names that are local at
/// that point (parameters and locals in scope). The callback sees each expression
/// tree root (statement expressions, initializers, conditions); it is responsible for
/// descending. Used by passes that need scope information per expression.
pub(crate) fn walk_function_exprs(func: &mut FunctionDef, f: &mut dyn FnMut(&mut Expr, &dyn Fn(&str) -> bool)) {
    let mut scopes = Scopes::default();
    scopes.push();
    for p in &func.proto.params {
        if let Some(n) = &p.name {
            scopes.declare(n);
        }
    }
    for s in &mut func.body {
        stmt_exprs(s, f, &mut scopes);
    }
}

fn stmt_exprs(s: &mut Stmt, f: &mut dyn FnMut(&mut Expr, &dyn Fn(&str) -> bool), scopes: &mut Scopes) {
    macro_rules! visit {
        ($e:expr) => {{
            let sc: &Scopes = scopes;
            f($e, &|n: &str| sc.is_local(n));
        }};
    }
    match &mut s.kind {
        StmtKind::Decl(d) => {
            for v in &mut d.vars {
                if let Some(i) = &mut v.init {
                    init_exprs(i, f, scopes);
                }
                scopes.declare(&v.name);
            }
        }
        StmtKind::Expr(e) | StmtKind::Case(e) => visit!(e),
        StmtKind::Return(Some(e)) => visit!(e),
        StmtKind::Block(b) => {
            scopes.push();
            for x in b {
                stmt_exprs(x, f, scopes);
            }
            scopes.pop();
        }
        StmtKind::If { cond, then, els } => {
            visit!(cond);
            scopes.push();
            stmt_exprs(then, f, scopes);
            scopes.pop();
            if let Some(e) = els {
                scopes.push();
                stmt_exprs(e, f, scopes);
                scopes.pop();
            }
        }
        StmtKind::Switch { expr: e, body } => {
            visit!(e);
            scopes.push();
            for x in body {
                stmt_exprs(x, f, scopes);
            }
            scopes.pop();
        }
        StmtKind::While { cond, body } => {
            scopes.push();
            match cond {
                Condition::Expr(e) => visit!(e),
                Condition::Decl { name, init: i, .. } => {
                    init_exprs(i, f, scopes);
                    scopes.declare(name);
                }
            }
            stmt_exprs(body, f, scopes);
            scopes.pop();
        }
        StmtKind::DoWhile { body, cond } => {
            scopes.push();
            stmt_exprs(body, f, scopes);
            scopes.pop();
            visit!(cond);
        }
        StmtKind::For { init, cond, step, body } => {
            scopes.push();
            if let Some(i) = init {
                stmt_exprs(i, f, scopes);
            }
            match cond {
                Some(Condition::Expr(e)) => visit!(e),
                Some(Condition::Decl { name, init: i, .. }) => {
                    init_exprs(i, f, scopes);
                    scopes.declare(name);
                }
                None => {}
            }
            if let Some(st) = step {
                visit!(st);
            }
            stmt_exprs(body, f, scopes);
            scopes.pop();
        }
        _ => {}
    }
}

fn init_exprs(i: &mut Init, f: &mut dyn FnMut(&mut Expr, &dyn Fn(&str) -> bool), scopes: &mut Scopes) {
    match i {
        Init::Expr(e) => {
            let sc: &Scopes = scopes;
            f(e, &|n: &str| sc.is_local(n));
        }
        Init::List(l) => {
            for x in l {
                init_exprs(x, f, scopes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_glsl;

    #[test]
    fn classifies_and_renames() {
        let mut u = parse_glsl(
            "uniform float a;\nfloat g = a;\nfloat f(float a, float b) { float c = a + b + g; { float g = 1.0; c += g; } return c + g; }\n",
            330,
        )
        .unwrap();
        let mut seen = Vec::new();
        walk_idents(&mut u, &mut |n, occ| {
            seen.push(format!("{n}:{occ:?}"));
            if n == "g" && occ == Occ::GlobalRef {
                *n = "G".into();
            }
        });
        let s: Vec<&str> = seen.iter().map(String::as_str).collect();
        assert!(s.contains(&"a:GlobalRef"));
        assert!(s.contains(&"a:LocalRef"));
        assert!(s.contains(&"g:LocalDecl"));
        assert!(s.contains(&"f:FunctionDecl"));
        assert!(s.contains(&"float:TypeName"));
        let mut p = crate::print::Printer::new();
        for it in &u.items {
            p.item(it);
        }
        let text = p.finish().0;
        assert!(text.contains("float c = a + b + G;"), "{text}");
        assert!(text.contains("c += g;"), "{text}");
        assert!(text.contains("return c + G;"), "{text}");
    }
}
