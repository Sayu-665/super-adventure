//! One-shot boolean conditions such as `program.<name>.enabled=<expr>`.

use crate::compile::{Compiler, Resolved, Scope};
use crate::node::{Ctx, SmoothState, eval};
use crate::parse::{ExprError, ExprErrorKind, parse};
use crate::rng::{DEFAULT_SEED, Rng};
use crate::value::{Value, ValueType};

/// Evaluate a boolean condition whose identifiers are boolean option names, e.g.
/// `program.composite.enabled=(BLOOM || SSAO) && !GODRAYS`.
///
/// Identifiers are resolved through `lookup`; unknown ones are `false` (callers may
/// want to warn). Besides `!`, `&&`, `||`, parentheses, `true` and `false`, the full
/// expression language is available, so numbers and comparisons work too (`1` is
/// true, `0` is false, booleans compare as 0/1). A numeric result is true when
/// non-zero.
///
/// Note: Iris treats an unparsable condition as `true`, and an identifier that names
/// no boolean option also as `true` (`OptionValues.getBooleanValueOrDefault`, with a
/// warning). Callers that want Iris' behaviour should map `Err` to `true` and make
/// `lookup` return `Some(true)` for unknown names; the default here (unknown =
/// `false`) follows the C preprocessor, where an undefined macro is `0`.
///
/// ```
/// let opts = |n: &str| match n { "BLOOM" => Some(true), "GODRAYS" => Some(false), _ => None };
/// assert!(sb_expr::eval_bool("(BLOOM || SSAO) && !GODRAYS", &opts).unwrap());
/// assert!(!sb_expr::eval_bool("SSAO", &opts).unwrap());
/// assert!(sb_expr::eval_bool("2 > 1", &opts).unwrap());
/// assert!(sb_expr::eval_bool("BLOOM &&", &opts).is_err());
/// ```
pub fn eval_bool(src: &str, lookup: &dyn Fn(&str) -> Option<bool>) -> Result<bool, ExprError> {
    eval_bool_with(src, &|name| lookup(name).map(Value::Bool))
}

/// Like [`eval_bool`], but identifiers may resolve to any [`Value`] (for example
/// the numeric value of a slider option, enabling `SHADOW_QUALITY >= 2`). Unknown
/// identifiers are `false`.
pub fn eval_bool_with(
    src: &str,
    lookup: &dyn Fn(&str) -> Option<Value>,
) -> Result<bool, ExprError> {
    let expr = parse(src)?;
    let mut scope = OptionScope { lookup };
    let mut compiler = Compiler::new(&mut scope, 0);
    let typed = compiler.compile(&expr)?;
    let slots = compiler.smooth_slots as usize;
    if !matches!(
        typed.ty,
        ValueType::Bool | ValueType::Int | ValueType::Float
    ) {
        return Err(ExprError::new(
            ExprErrorKind::Type,
            expr.span.start,
            format!("a condition must be a boolean, found {}", typed.ty),
        ));
    }
    let mut smooth = vec![SmoothState::default(); slots];
    let mut rng = Rng::new(DEFAULT_SEED);
    let mut cx = Ctx {
        inputs: &[],
        vars: &[],
        smooth: &mut smooth,
        rng: &mut rng,
        dt: 0.0,
    };
    Ok(eval(&typed.node, &mut cx).to_bool())
}

struct OptionScope<'a> {
    lookup: &'a dyn Fn(&str) -> Option<Value>,
}

impl Scope for OptionScope<'_> {
    fn resolve(&mut self, name: &str) -> Result<Option<Resolved>, String> {
        Ok(Some(Resolved::Const(
            (self.lookup)(name).unwrap_or(Value::Bool(false)),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(name: &str) -> Option<bool> {
        match name {
            "A" => Some(true),
            "B" => Some(false),
            "C" => Some(true),
            _ => None,
        }
    }

    fn ev(s: &str) -> bool {
        eval_bool(s, &opts).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn boolean_logic() {
        assert!(ev("A"));
        assert!(!ev("B"));
        assert!(!ev("UNKNOWN"));
        assert!(ev("!B"));
        assert!(ev("A && C"));
        assert!(!ev("A && B"));
        assert!(ev("A || B"));
        assert!(ev("(B || A) && !B"));
        assert!(ev("B && A || C"), "&& binds tighter than ||");
        assert!(!ev("B && (A || C)"));
        assert!(ev("!(B)"));
        assert!(ev("!!A"));
        assert!(ev("true"));
        assert!(!ev("false"));
        assert!(ev("  A  "));
    }

    #[test]
    fn numbers_and_comparisons() {
        assert!(ev("1"));
        assert!(!ev("0"));
        assert!(ev("A == 1"));
        assert!(ev("B == 0"));
        assert!(ev("A != B"));
        assert!(ev("2 >= 1 && 1.5 < 2"));
        assert!(ev("1 ≤ 2"));
        assert!(!ev("!1"));
        let values = |n: &str| match n {
            "SHADOW_QUALITY" => Some(Value::Int(2)),
            "BLOOM" => Some(Value::Bool(true)),
            _ => None,
        };
        assert!(eval_bool_with("SHADOW_QUALITY >= 2 && BLOOM", &values).unwrap());
        assert!(!eval_bool_with("SHADOW_QUALITY > 2", &values).unwrap());
        assert!(eval_bool_with("SHADOW_QUALITY", &values).unwrap());
        assert!(!eval_bool_with("MISSING", &values).unwrap());
    }

    #[test]
    fn errors() {
        let e = eval_bool("A &&", &opts).unwrap_err();
        assert_eq!(e.offset, 4);
        assert_eq!(eval_bool("A & B", &opts).unwrap_err().offset, 2);
        assert_eq!(eval_bool("(A", &opts).unwrap_err().offset, 0);
        assert_eq!(
            eval_bool("vec2(1, 2)", &opts).unwrap_err().kind,
            ExprErrorKind::Type
        );
        assert_eq!(
            eval_bool("nosuchfn(A)", &opts).unwrap_err().kind,
            ExprErrorKind::UnknownFunction
        );
        assert_eq!(
            eval_bool("", &opts).unwrap_err().kind,
            ExprErrorKind::Syntax
        );
    }
}
