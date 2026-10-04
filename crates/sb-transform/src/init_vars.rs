//! Zero-initialization of `out` parameters and of local and plain global variables
//! declared without an initializer.
//!
//! GLSL leaves all of them undefined until they are written (a global "enters main()
//! with an undefined value"). Packs that read them first
//! (`void f(out vec3 c) { c += x; }`, `vec3 sum; sum += x;`) work on GL drivers, which in
//! practice start such variables at zero, but a SPIR-V compiler may fold the undefined read
//! and the whole expression that depends on it: voyager-shader-2.0's
//! `volumetric_filter(..., out vec3 color0, out vec3 color1, out vec4 color2)` accumulates
//! into its out parameters, and lavapipe (LLVM) turned the result into zero, so every pixel
//! of the frame was black. Zero is a valid value of an undefined variable, so the
//! initialization never changes a correct program and makes the translation deterministic
//! (ANGLE initializes locals and out parameters for the same reason). Writes that follow
//! immediately are dead stores the driver removes.
//!
//! Only types with an obvious zero are initialized: scalars, vectors and matrices, structs
//! made of them, and arrays of them with a literal length up to [`MAX_ARRAY_LEN`]. Opaque
//! types, unsized arrays and anything else are left alone.

use std::collections::HashMap;

use sb_core::GlslType;

use crate::ast::*;

/// Longest array that is zero-initialized (longer constructors bloat the code).
const MAX_ARRAY_LEN: i64 = 64;

/// Struct definitions of the unit by name (global `struct S {...};` declarations).
type Structs = HashMap<String, StructDef>;

/// Zero-initialize the uninitialized plain globals, `out` parameters and uninitialized
/// locals of `unit`. Returns (out parameters, locals and globals) initialized.
pub(crate) fn apply(unit: &mut TranslationUnit) -> (usize, usize) {
    let mut structs: Structs = HashMap::new();
    let (mut params, mut locals) = (0, 0);
    for item in &mut unit.items {
        let ItemKind::Decl(d) = &mut item.kind else { continue };
        if let TypeBase::Struct(s) = &d.ty.ty.base
            && let Some(n) = &s.name
        {
            structs.insert(n.clone(), (**s).clone());
        }
        // Plain globals only: `const`, `uniform`, `in`/`out`, `buffer` and `shared`
        // (which cannot have an initializer) carry a storage qualifier.
        locals += init_declarators(d, &structs, |n| !n.starts_with("gl_"));
    }
    for f in unit.functions_mut() {
        let mut prologue = Vec::new();
        for p in &f.proto.params {
            let out = p.quals.iter().any(|q| matches!(q, Qualifier::Storage(Storage::Out)));
            let (Some(name), true) = (&p.name, out) else { continue };
            let mut dims = p.ty.array.clone();
            dims.extend(p.array.iter().cloned());
            if let Some(z) = zero_of(&p.ty.base, &dims, &structs, 0) {
                prologue.push(Stmt {
                    kind: StmtKind::Expr(Expr::Assign(Box::new(Expr::ident(name.clone())), AssignOp::Equal, Box::new(Expr::raw(z)))),
                    line: 0,
                });
                params += 1;
            }
        }
        for s in &mut f.body {
            s.walk_stmts_mut(&mut |s| {
                if let StmtKind::Decl(d) = &mut s.kind {
                    locals += init_declarators(d, &structs, |_| true);
                }
            });
        }
        if !prologue.is_empty() {
            prologue.append(&mut f.body);
            f.body = prologue;
        }
    }
    (params, locals)
}

/// Give every declarator of `d` without an initializer (and accepted by `wanted`) a zero
/// initializer, unless `d` carries a storage qualifier (`const` is always initialized;
/// the others cannot be, or are not plain variables). Returns the number initialized.
fn init_declarators(d: &mut Declaration, structs: &Structs, wanted: impl Fn(&str) -> bool) -> usize {
    if d.ty.quals.iter().any(|q| matches!(q, Qualifier::Storage(_))) {
        return 0;
    }
    let mut n = 0;
    for v in &mut d.vars {
        if v.init.is_some() || !wanted(&v.name) {
            continue;
        }
        let mut dims = d.ty.ty.array.clone();
        dims.extend(v.array.iter().cloned());
        if let Some(z) = zero_of(&d.ty.ty.base, &dims, structs, 0) {
            v.init = Some(Init::Expr(Expr::raw(z)));
            n += 1;
        }
    }
    n
}

/// GLSL text of the zero value of `base` with array dimensions `dims`.
fn zero_of(base: &TypeBase, dims: &[ArrayDim], structs: &Structs, depth: u32) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let (type_name, element) = match base {
        TypeBase::Named(n) => (n.clone(), zero_of_named(n, structs, depth)?),
        // Inline (anonymous or local) struct types are rare in locals; skip them.
        TypeBase::Struct(_) => return None,
    };
    match dims {
        [] => Some(element),
        [ArrayDim::Sized(Expr::Int(n))] if (1..=MAX_ARRAY_LEN).contains(&i64::from(*n)) => {
            let n = *n as usize;
            Some(format!("{type_name}[{n}]({})", vec![element; n].join(", ")))
        }
        _ => None,
    }
}

/// Zero value of the named (non-array) type `n`.
fn zero_of_named(n: &str, structs: &Structs, depth: u32) -> Option<String> {
    if let Some(t) = GlslType::parse(n) {
        if t.array.is_some() {
            return None;
        }
        return Some(format!("{}(0)", t.glsl_name()));
    }
    let s = structs.get(n)?;
    let mut members = Vec::new();
    for f in &s.fields {
        for (_, dims) in &f.names {
            let mut all = f.ty.array.clone();
            all.extend(dims.iter().cloned());
            members.push(zero_of(&f.ty.base, &all, structs, depth + 1)?);
        }
    }
    Some(format!("{n}({})", members.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_glsl;

    fn run(src: &str) -> (String, (usize, usize)) {
        let mut u = parse_glsl(src, 460).unwrap();
        let n = apply(&mut u);
        let mut p = crate::print::Printer::new();
        for i in &u.items {
            p.item(i);
        }
        (p.finish().0, n)
    }

    /// voyager-shader-2.0's pattern: an out parameter accumulated before it is written.
    #[test]
    fn out_parameters_start_at_zero() {
        let (out, n) = run("void f(in float w, out vec3 a, inout vec4 b, out float c[3], out mat3 m) { a += vec3(w); c[0] += w; }\nvoid main() {}\n");
        assert_eq!(n.0, 3, "{out}");
        assert!(out.contains("a = (vec3(0));"), "{out}");
        assert!(out.contains("c = (float[3](float(0), float(0), float(0)));"), "{out}");
        assert!(out.contains("m = (mat3(0));"), "{out}");
        // inout parameters keep the caller's value.
        assert!(!out.contains("b = (vec4(0))"), "{out}");
        // The initialization precedes the pack's statements.
        assert!(out.find("a = (vec3(0));").unwrap() < out.find("a += vec3(w)").unwrap(), "{out}");
    }

    #[test]
    fn uninitialized_locals_start_at_zero() {
        let (out, n) = run(
            "struct S { vec2 p; float w[2]; };\nvoid main() { vec3 sum; float x = 1.0, y; S s; const float k = 2.0; int big[100]; for (int i; i < 2; i++) { sum += vec3(x); } }\n",
        );
        assert!(out.contains("vec3 sum = (vec3(0));"), "{out}");
        assert!(out.contains("x = 1.0, y = (float(0))"), "{out}");
        assert!(out.contains("S s = (S(vec2(0), float[2](float(0), float(0))));"), "{out}");
        assert!(out.contains("int i = (int(0))"), "{out}");
        // Too long to spell out: left alone.
        assert!(out.contains("int big[100];"), "{out}");
        assert_eq!(n.1, 4, "{out}");
    }

    /// Arrays whose length is not a literal, and qualified globals, are left alone.
    #[test]
    fn non_literal_arrays_and_qualified_globals_are_left_alone() {
        let (out, n) = run("const int N = 4;\nuniform sampler2D t;\nuniform float u0;\nin vec2 uv;\nshared float s[4];\nfloat g[N];\nvoid g2(out float u[N]) {}\nvoid main() { float v[N]; }\n");
        assert_eq!(n, (0, 0), "{out}");
        assert!(out.contains("uniform float u0;") && out.contains("in vec2 uv;") && out.contains("shared float s[4];"), "{out}");
    }

    /// Plain globals are undefined at the start of `main` too: a pack accumulating into
    /// one before writing it reads zero, as on GL drivers.
    #[test]
    fn uninitialized_plain_globals_start_at_zero() {
        let (out, n) = run("struct L { vec3 c; };\nvec3 total;\nfloat w = 1.0, acc;\nL light;\nprecise vec2 p;\nvoid main() { total += vec3(w); acc += 1.0; }\n");
        assert!(out.contains("vec3 total = (vec3(0));"), "{out}");
        assert!(out.contains("float w = 1.0, acc = (float(0));"), "{out}");
        assert!(out.contains("L light = (L(vec3(0)));"), "{out}");
        assert!(out.contains("precise vec2 p = (vec2(0));"), "{out}");
        assert_eq!(n, (0, 4), "{out}");
    }
}
