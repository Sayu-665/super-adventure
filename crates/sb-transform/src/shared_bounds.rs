//! Bounds clamping of `shared` (workgroup) array indices.
//!
//! An out-of-bounds index into a `shared` array is undefined behaviour that GL drivers
//! tend to absorb, but Vulkan's robustness features do not cover workgroup memory: an
//! out-of-bounds write can corrupt other workgroup data, fault the device, or (on
//! lavapipe, whose workgroup memory is a heap allocation) corrupt the host heap and abort
//! the process. fastpbr's `skylightPrep` compute indexes its `shared vec3
//! Skybox[4][4]` with the *global* invocation id (up to 31), which crashed the render.
//! Every index into a global `shared` array dimension is therefore clamped to the
//! dimension (`a[i]` -> `a[clamp(int(i), 0, a.length() - 1)]`); in-bounds accesses are
//! unchanged, and the length is a compile-time constant of the sized array.

use std::collections::HashMap;

use crate::ast::*;

/// Clamp the indices of every global `shared` array of `unit`. Returns the number of
/// index expressions clamped.
pub(crate) fn apply(unit: &mut TranslationUnit) -> usize {
    // Shared array name -> number of array dimensions.
    let mut shared: HashMap<String, usize> = HashMap::new();
    for item in &unit.items {
        let ItemKind::Decl(d) = &item.kind else { continue };
        if !d.ty.quals.iter().any(|q| matches!(q, Qualifier::Storage(Storage::Shared))) {
            continue;
        }
        for v in &d.vars {
            let dims = d.ty.ty.array.len() + v.array.len();
            let sized = d.ty.ty.array.iter().chain(&v.array).all(|a| matches!(a, ArrayDim::Sized(_)));
            if dims > 0 && sized {
                shared.insert(v.name.clone(), dims);
            }
        }
    }
    if shared.is_empty() {
        return 0;
    }
    let mut count = 0;
    for f in unit.functions_mut() {
        crate::scope::walk_function_exprs(f, &mut |root, is_local| clamp_indices(root, &shared, is_local, &mut count));
    }
    count
}

/// The array variable and the number of indices applied below `e` (`a` -> 0, `a[i]` -> 1,
/// `a[i][j]` -> 2), for an index chain rooted at an identifier.
fn chain_root(e: &Expr) -> Option<(&str, usize)> {
    match e {
        Expr::Ident(n) => Some((n, 0)),
        Expr::Index(b, _) => chain_root(b).map(|(n, d)| (n, d + 1)),
        _ => None,
    }
}

fn clamp_indices(e: &mut Expr, shared: &HashMap<String, usize>, is_local: &dyn Fn(&str) -> bool, count: &mut usize) {
    e.walk_children_mut(&mut |c| {
        clamp_indices(c, shared, is_local, count);
        Walk::Skip
    });
    let Expr::Index(base, idx) = e else { return };
    let Some((name, depth)) = chain_root(base) else { return };
    let Some(&dims) = shared.get(name) else { return };
    if depth >= dims || is_local(name) {
        // A vector/matrix component of an element, or a local of the same name.
        return;
    }
    // Length of dimension `depth`: `a.length()`, `a[0].length()`, ...
    let length = format!("{name}{}.length()", "[0]".repeat(depth));
    let index = std::mem::replace(idx.as_mut(), Expr::Int(0));
    **idx = Expr::call("clamp", vec![Expr::call("int", vec![index]), Expr::Int(0), Expr::raw(format!("{length} - 1"))]);
    *count += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_glsl;

    fn run(src: &str) -> (String, usize) {
        let mut u = parse_glsl(src, 460).unwrap();
        let n = apply(&mut u);
        let mut p = crate::print::Printer::new();
        for i in &u.items {
            p.item(i);
        }
        (p.finish().0, n)
    }

    /// fastpbr's pattern: a 2D shared array indexed by the global invocation id.
    #[test]
    fn shared_indices_are_clamped_per_dimension() {
        let (out, n) = run(
            "layout(local_size_x = 4, local_size_y = 4) in;\nshared vec3 Skybox[4][4];\nshared float tile[16];\nvoid main() { ivec2 i = ivec2(gl_GlobalInvocationID.xy); Skybox[i.x][i.y] = vec3(1.0); float a = Skybox[1][2].x + Skybox[0][1][2] + tile[gl_LocalInvocationIndex]; }\n",
        );
        assert_eq!(n, 7, "{out}");
        assert!(out.contains("Skybox[clamp(int(i.x), 0, Skybox.length() - 1)][clamp(int(i.y), 0, Skybox[0].length() - 1)] = vec3(1.0)"), "{out}");
        // The third index of `Skybox[0][1][2]` selects a vector component: not clamped.
        assert!(out.contains("[clamp(int(1), 0, Skybox[0].length() - 1)][2]"), "{out}");
        assert!(out.contains("tile[clamp(int(gl_LocalInvocationIndex), 0, tile.length() - 1)]"), "{out}");
    }

    /// Non-shared arrays and locals shadowing a shared name are untouched.
    #[test]
    fn other_arrays_are_untouched() {
        let (out, n) = run("shared float s[8];\nuniform float u[4];\nvoid f() { float s[2]; s[5] = 1.0; }\nvoid main() { float x = u[7]; }\n");
        assert_eq!(n, 0, "{out}");
    }
}
