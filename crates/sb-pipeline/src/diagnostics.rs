//! Spec step 15: aggregating diagnostics — deduplicated, collapsed across programs, sorted.

use sb_core::{Diagnostic, Diagnostics, Severity};
use std::collections::HashMap;

/// Deduplicate and sort diagnostics.
///
/// * Identical diagnostics are kept once.
/// * Warnings and notes that differ only in the program they were found in (a shared
///   include analyzed for many programs) are collapsed into one, tagged with the first
///   program and suffixed with the number of other programs. Errors stay per program.
/// * Order: errors, warnings, notes; then program, file, line, code, message.
pub fn finalize(diags: Diagnostics) -> Diagnostics {
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut exact: HashMap<String, ()> = HashMap::new();
    let mut collapsed: HashMap<String, (usize, usize)> = HashMap::new();
    for d in diags {
        let full = format!("{:?}|{}|{}|{:?}|{:?}|{:?}", d.severity, d.code, d.message, d.location, d.program, d.stage);
        if exact.insert(full, ()).is_some() {
            continue;
        }
        if d.severity != Severity::Error && d.program.is_some() && d.location.is_some() {
            let k = format!("{:?}|{}|{}|{:?}|{:?}", d.severity, d.code, d.message, d.location, d.stage);
            if let Some((idx, n)) = collapsed.get_mut(&k) {
                *n += 1;
                let _ = idx;
                continue;
            }
            collapsed.insert(k, (out.len(), 0));
        }
        out.push(d);
    }
    for (idx, n) in collapsed.into_values() {
        if n > 0 {
            let d = &mut out[idx];
            d.message = format!("{} (also in {n} other program{})", d.message, if n == 1 { "" } else { "s" });
        }
    }
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.program.cmp(&b.program))
            .then_with(|| {
                let fa = a.location.as_ref().map(|l| (l.file.as_str(), l.line));
                let fb = b.location.as_ref().map(|l| (l.file.as_str(), l.line));
                fa.cmp(&fb)
            })
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.message.cmp(&b.message))
    });
    Diagnostics(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::SourceLocation;

    #[test]
    fn dedupe_collapse_sort() {
        let w = |p: &str| Diagnostic::warning("xf.w", "shared warning").at(SourceLocation::new("lib/a.glsl", 3)).in_program(p);
        let e = |p: &str| Diagnostic::error("spv.compile", "boom").at(SourceLocation::new("lib/a.glsl", 3)).in_program(p);
        let ds = Diagnostics(vec![
            w("composite"),
            w("composite"),
            w("deferred"),
            w("final"),
            e("final"),
            e("composite"),
            Diagnostic::info("pack.x", "note"),
        ]);
        let out = finalize(ds);
        let v: Vec<String> = out.iter().map(|d| format!("{:?} {:?} {}", d.severity, d.program, d.message)).collect();
        assert_eq!(
            v,
            vec![
                "Error Some(\"composite\") boom".to_string(),
                "Error Some(\"final\") boom".to_string(),
                "Warning Some(\"composite\") shared warning (also in 2 other programs)".to_string(),
                "Info None note".to_string(),
            ]
        );
    }
}
