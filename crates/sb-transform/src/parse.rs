//! Parsing preprocessed GLSL with glsl-lang and converting the result into
//! ShaderBridge's own syntax tree ([`crate::ast`]).
//!
//! glsl-lang 0.8 workarounds applied here:
//! * unsuffixed integer literals `>= 0x80000000` are rejected by its lexer: they are
//!   rewritten to `int(<literal>u)` before parsing ([`crate::text::wrap_large_int_literals`]);
//! * `#extension` lines are never fed to it (an unknown extension is a hard error); the
//!   preprocessed code has none, the hoisted directives are kept aside;
//! * the minimal lexer is used with an explicit `default_version`, so no `#version` line
//!   is needed and line numbers of the parsed text equal those of the preprocessed code.
//!   Type names are gated by version, so the code is parsed as GLSL 4.60 first (every
//!   type name, including extension types, is then known) and with the source version
//!   as a fallback.

use glsl_lang::ast as g;
use glsl_lang::lexer::min::str::Lexer as MinLexer;
use glsl_lang::parse::{Parse, ParseOptions};

use crate::ast::*;

/// A parse failure at a 1-based line of the parsed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseFailure {
    pub line: u32,
    pub message: String,
}

/// Byte offset -> 1-based line.
struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    fn line_of(&self, offset: usize) -> u32 {
        let idx = match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        idx as u32 + 1
    }

    fn span_line<T: glsl_lang::ast::NodeContent>(&self, node: &g::Node<T>) -> u32 {
        node.span.map_or(0, |s| self.line_of(u32::from(s.range().start()) as usize))
    }
}

/// Maximum expression nesting accepted (far above real shaders; keeps the recursive
/// passes bounded).
pub(crate) const MAX_EXPR_DEPTH: usize = 4000;

/// Parse preprocessed GLSL `code` (no directives) of source version `version`.
pub(crate) fn parse_glsl(code: &str, version: u32) -> Result<TranslationUnit, ParseFailure> {
    let (text, _wrapped) = crate::text::sanitize_for_parse(code);
    let lines = Lines::new(&text);
    let first = parse_with_version(&text, 460, &lines);
    let fallback_version = version.clamp(130, 460) as u16;
    match first {
        Ok(u) => Ok(u),
        Err(e) if fallback_version != 460 => parse_with_version(&text, fallback_version, &lines).map_err(|_| e),
        Err(e) => Err(e),
    }
}

fn parse_with_version(text: &str, version: u16, lines: &Lines) -> Result<TranslationUnit, ParseFailure> {
    let opts = ParseOptions { default_version: version, target_vulkan: false, ..ParseOptions::default() };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        <g::TranslationUnit as Parse>::parse_with_options::<MinLexer>(text, &opts)
            .map(|(tu, _, _)| tu)
            .map_err(|e| ParseFailure { line: lines.line_of(u32::from(e.pos().start()) as usize), message: e.inner().to_string() })
    }));
    let tu = match result {
        Ok(Ok(tu)) => tu,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(ParseFailure { line: 0, message: "the GLSL parser panicked".into() }),
    };
    let mut conv = Converter { lines, error: None };
    let unit = conv.unit(&tu);
    // glsl-lang drops deep trees recursively; drop on this (large-stack) thread.
    drop(tu);
    match conv.error {
        Some(e) => Err(e),
        None => Ok(unit),
    }
}

struct Converter<'a> {
    lines: &'a Lines,
    error: Option<ParseFailure>,
}

fn type_name_of(t: &g::TypeSpecifierNonArray) -> String {
    match &t.content {
        g::TypeSpecifierNonArrayData::TypeName(n) => n.0.to_string(),
        g::TypeSpecifierNonArrayData::Struct(s) => s.name.as_ref().map_or_else(String::new, |n| n.0.to_string()),
        _ => {
            let mut s = String::new();
            let mut state = glsl_lang::transpiler::glsl::FormattingState::default();
            let _ = glsl_lang::transpiler::glsl::show_type_specifier_non_array(&mut s, t, &mut state);
            s
        }
    }
}

impl Converter<'_> {
    fn fail(&mut self, line: u32, message: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(ParseFailure { line, message: message.into() });
        }
    }

    fn unit(&mut self, tu: &g::TranslationUnit) -> TranslationUnit {
        let mut items = Vec::with_capacity(tu.0.len());
        for ed in &tu.0 {
            let line = self.lines.span_line(ed);
            match &ed.content {
                g::ExternalDeclarationData::Preprocessor(_) => {}
                g::ExternalDeclarationData::FunctionDefinition(f) => {
                    let proto = self.prototype(&f.prototype);
                    let body = f.statement.statement_list.iter().map(|s| self.stmt(s)).collect();
                    items.push(Item { kind: ItemKind::Function(FunctionDef { proto, body }), line });
                }
                g::ExternalDeclarationData::Declaration(d) => {
                    if let Some(kind) = self.global_decl(d, line) {
                        items.push(Item { kind, line });
                    }
                }
            }
        }
        TranslationUnit { items }
    }

    fn global_decl(&mut self, d: &g::Declaration, line: u32) -> Option<ItemKind> {
        Some(match &d.content {
            g::DeclarationData::FunctionPrototype(p) => ItemKind::Prototype(self.prototype(p)),
            g::DeclarationData::InitDeclaratorList(l) => ItemKind::Decl(self.decl_list(l)),
            g::DeclarationData::Precision(p, t) => ItemKind::Precision(precision(p), self.type_spec(t)),
            g::DeclarationData::Block(b) => ItemKind::Block(self.block(b)),
            g::DeclarationData::Invariant(id) => ItemKind::Invariant(id.0.to_string()),
            g::DeclarationData::TypeOnly(q) => {
                let quals = self.quals(Some(q));
                if quals.is_empty() {
                    return None;
                }
                let _ = line;
                ItemKind::QualifierOnly(quals)
            }
        })
    }

    fn block(&mut self, b: &g::Block) -> InterfaceBlock {
        InterfaceBlock {
            quals: self.quals(Some(&b.qualifier)),
            name: b.name.0.to_string(),
            fields: b.fields.iter().map(|f| self.field(f)).collect(),
            instance: b.identifier.as_ref().map(|a| (a.ident.0.to_string(), self.array_opt(&a.array_spec))),
        }
    }

    fn field(&mut self, f: &g::StructFieldSpecifier) -> Field {
        Field {
            quals: self.quals(f.qualifier.as_ref()),
            ty: self.type_spec(&f.ty),
            names: f.identifiers.iter().map(|a| (a.ident.0.to_string(), self.array_opt(&a.array_spec))).collect(),
        }
    }

    fn prototype(&mut self, p: &g::FunctionPrototype) -> Prototype {
        Prototype {
            ret: self.full_type(&p.ty),
            name: p.name.0.to_string(),
            params: p
                .parameters
                .iter()
                .map(|param| match &param.content {
                    g::FunctionParameterDeclarationData::Named(q, d) => Param {
                        quals: self.quals(q.as_ref()),
                        ty: self.type_spec(&d.ty),
                        name: Some(d.ident.ident.0.to_string()),
                        array: self.array_opt(&d.ident.array_spec),
                    },
                    g::FunctionParameterDeclarationData::Unnamed(q, t) => Param {
                        quals: self.quals(q.as_ref()),
                        ty: self.type_spec(t),
                        name: None,
                        array: Vec::new(),
                    },
                })
                .collect(),
        }
    }

    fn full_type(&mut self, t: &g::FullySpecifiedType) -> FullType {
        FullType { quals: self.quals(t.qualifier.as_ref()), ty: self.type_spec(&t.ty) }
    }

    fn type_spec(&mut self, t: &g::TypeSpecifier) -> TypeSpec {
        let base = match &t.ty.content {
            g::TypeSpecifierNonArrayData::Struct(s) => TypeBase::Struct(Box::new(StructDef {
                name: s.name.as_ref().map(|n| n.0.to_string()),
                fields: s.fields.iter().map(|f| self.field(f)).collect(),
            })),
            _ => TypeBase::Named(type_name_of(&t.ty)),
        };
        TypeSpec { base, array: self.array_opt(&t.array_specifier) }
    }

    fn array_opt(&mut self, a: &Option<g::ArraySpecifier>) -> Vec<ArrayDim> {
        match a {
            None => Vec::new(),
            Some(a) => a
                .dimensions
                .iter()
                .map(|d| match &d.content {
                    g::ArraySpecifierDimensionData::Unsized => ArrayDim::Unsized,
                    g::ArraySpecifierDimensionData::ExplicitlySized(e) => ArrayDim::Sized(self.expr(e, 0)),
                })
                .collect(),
        }
    }

    fn quals(&mut self, q: Option<&g::TypeQualifier>) -> Vec<Qualifier> {
        let Some(q) = q else { return Vec::new() };
        q.qualifiers
            .iter()
            .map(|s| match &s.content {
                g::TypeQualifierSpecData::Storage(st) => Qualifier::Storage(match &st.content {
                    g::StorageQualifierData::Const => Storage::Const,
                    g::StorageQualifierData::InOut => Storage::InOut,
                    g::StorageQualifierData::In => Storage::In,
                    g::StorageQualifierData::Out => Storage::Out,
                    g::StorageQualifierData::Centroid => Storage::Centroid,
                    g::StorageQualifierData::Patch => Storage::Patch,
                    g::StorageQualifierData::Sample => Storage::Sample,
                    g::StorageQualifierData::Uniform => Storage::Uniform,
                    g::StorageQualifierData::Buffer => Storage::Buffer,
                    g::StorageQualifierData::Shared => Storage::Shared,
                    g::StorageQualifierData::Coherent => Storage::Coherent,
                    g::StorageQualifierData::Volatile => Storage::Volatile,
                    g::StorageQualifierData::Restrict => Storage::Restrict,
                    g::StorageQualifierData::ReadOnly => Storage::ReadOnly,
                    g::StorageQualifierData::WriteOnly => Storage::WriteOnly,
                    g::StorageQualifierData::Attribute => Storage::Attribute,
                    g::StorageQualifierData::Varying => Storage::Varying,
                    g::StorageQualifierData::Subroutine(ts) => {
                        Storage::Subroutine(ts.iter().map(|t| type_name_of(&t.ty)).collect())
                    }
                }),
                g::TypeQualifierSpecData::Layout(l) => Qualifier::Layout(
                    l.ids
                        .iter()
                        .map(|id| match &id.content {
                            g::LayoutQualifierSpecData::Identifier(n, v) => {
                                LayoutId { name: n.0.to_string(), value: v.as_ref().map(|e| self.expr(e, 0)) }
                            }
                            g::LayoutQualifierSpecData::Shared => LayoutId { name: "shared".into(), value: None },
                        })
                        .collect(),
                ),
                g::TypeQualifierSpecData::Precision(p) => Qualifier::Precision(precision(p)),
                g::TypeQualifierSpecData::Interpolation(i) => Qualifier::Interp(match i.content {
                    g::InterpolationQualifierData::Smooth => Interp::Smooth,
                    g::InterpolationQualifierData::Flat => Interp::Flat,
                    g::InterpolationQualifierData::NoPerspective => Interp::NoPerspective,
                }),
                g::TypeQualifierSpecData::Invariant => Qualifier::Invariant,
                g::TypeQualifierSpecData::Precise => Qualifier::Precise,
            })
            .collect()
    }

    fn decl_list(&mut self, l: &g::InitDeclaratorList) -> Declaration {
        let ty = self.full_type(&l.head.ty);
        let mut vars = Vec::new();
        if let Some(name) = &l.head.name {
            vars.push(Declarator {
                name: name.0.to_string(),
                array: self.array_opt(&l.head.array_specifier),
                init: l.head.initializer.as_ref().map(|i| self.init(i)),
            });
        }
        for t in &l.tail {
            vars.push(Declarator {
                name: t.ident.ident.0.to_string(),
                array: self.array_opt(&t.ident.array_spec),
                init: t.initializer.as_ref().map(|i| self.init(i)),
            });
        }
        Declaration { ty, vars }
    }

    fn init(&mut self, i: &g::Initializer) -> Init {
        match &i.content {
            g::InitializerData::Simple(e) => Init::Expr(self.expr(e, 0)),
            g::InitializerData::List(l) => Init::List(l.iter().map(|x| self.init(x)).collect()),
        }
    }

    fn stmt(&mut self, s: &g::Statement) -> Stmt {
        let line = self.lines.span_line(s);
        let kind = match &s.content {
            g::StatementData::Declaration(d) => match &d.content {
                g::DeclarationData::InitDeclaratorList(l) => StmtKind::Decl(self.decl_list(l)),
                // Local precision statements are stripped anyway; local prototypes,
                // blocks and invariant declarations are not valid in function bodies.
                g::DeclarationData::Precision(..) => StmtKind::Empty,
                g::DeclarationData::FunctionPrototype(_)
                | g::DeclarationData::Block(_)
                | g::DeclarationData::Invariant(_)
                | g::DeclarationData::TypeOnly(_) => {
                    self.fail(line, "declaration not allowed inside a function body");
                    StmtKind::Empty
                }
            },
            g::StatementData::Expression(e) => match &e.0 {
                Some(e) => StmtKind::Expr(self.expr(e, 0)),
                None => StmtKind::Empty,
            },
            g::StatementData::Selection(sel) => {
                let cond = self.expr(&sel.cond, 0);
                let (then, els) = match &sel.rest.content {
                    g::SelectionRestStatementData::Statement(t) => (Box::new(self.stmt(t)), None),
                    g::SelectionRestStatementData::Else(t, e) => (Box::new(self.stmt(t)), Some(Box::new(self.stmt(e)))),
                };
                StmtKind::If { cond, then, els }
            }
            g::StatementData::Switch(sw) => StmtKind::Switch {
                expr: self.expr(&sw.head, 0),
                body: sw.body.iter().map(|s| self.stmt(s)).collect(),
            },
            g::StatementData::CaseLabel(c) => match &c.content {
                g::CaseLabelData::Case(e) => StmtKind::Case(self.expr(e, 0)),
                g::CaseLabelData::Def => StmtKind::Default,
            },
            g::StatementData::Iteration(it) => match &it.content {
                g::IterationStatementData::While(c, b) => {
                    StmtKind::While { cond: self.condition(c), body: Box::new(self.stmt(b)) }
                }
                g::IterationStatementData::DoWhile(b, c) => {
                    StmtKind::DoWhile { body: Box::new(self.stmt(b)), cond: self.expr(c, 0) }
                }
                g::IterationStatementData::For(init, rest, body) => {
                    let init = match &init.content {
                        g::ForInitStatementData::Expression(None) => None,
                        g::ForInitStatementData::Expression(Some(e)) => {
                            Some(Box::new(Stmt::new(StmtKind::Expr(self.expr(e, 0)), line)))
                        }
                        g::ForInitStatementData::Declaration(d) => match &d.content {
                            g::DeclarationData::InitDeclaratorList(l) => {
                                Some(Box::new(Stmt::new(StmtKind::Decl(self.decl_list(l)), line)))
                            }
                            _ => {
                                self.fail(line, "unsupported for-loop initializer");
                                None
                            }
                        },
                    };
                    StmtKind::For {
                        init,
                        cond: rest.condition.as_ref().map(|c| self.condition(c)),
                        step: rest.post_expr.as_ref().map(|e| self.expr(e, 0)),
                        body: Box::new(self.stmt(body)),
                    }
                }
            },
            g::StatementData::Jump(j) => match &j.content {
                g::JumpStatementData::Continue => StmtKind::Continue,
                g::JumpStatementData::Break => StmtKind::Break,
                g::JumpStatementData::Return(e) => StmtKind::Return(e.as_ref().map(|e| self.expr(e, 0))),
                g::JumpStatementData::Discard => StmtKind::Discard,
            },
            g::StatementData::Compound(c) => StmtKind::Block(c.statement_list.iter().map(|s| self.stmt(s)).collect()),
        };
        Stmt { kind, line }
    }

    fn condition(&mut self, c: &g::Condition) -> Condition {
        match &c.content {
            g::ConditionData::Expr(e) => Condition::Expr(self.expr(e, 0)),
            g::ConditionData::Assignment(t, id, init) => {
                Condition::Decl { ty: self.full_type(t), name: id.0.to_string(), init: self.init(init) }
            }
        }
    }

    fn expr(&mut self, e: &g::Expr, depth: usize) -> Expr {
        if depth > MAX_EXPR_DEPTH {
            let line = self.lines.span_line(e);
            self.fail(line, format!("expression nested more than {MAX_EXPR_DEPTH} levels deep"));
            return Expr::Int(0);
        }
        let d = depth + 1;
        match &e.content {
            g::ExprData::Variable(id) => Expr::Ident(id.0.to_string()),
            g::ExprData::IntConst(v) => Expr::Int(*v),
            g::ExprData::UIntConst(v) => Expr::UInt(*v),
            g::ExprData::BoolConst(v) => Expr::Bool(*v),
            g::ExprData::FloatConst(v) => Expr::Float(*v),
            g::ExprData::DoubleConst(v) => Expr::Double(*v),
            g::ExprData::Unary(op, a) => Expr::Unary(unary(op), Box::new(self.expr(a, d))),
            g::ExprData::Binary(op, a, b) => {
                Expr::Binary(binary(op), Box::new(self.expr(a, d)), Box::new(self.expr(b, d)))
            }
            g::ExprData::Ternary(a, b, c) => {
                Expr::Ternary(Box::new(self.expr(a, d)), Box::new(self.expr(b, d)), Box::new(self.expr(c, d)))
            }
            g::ExprData::Assignment(a, op, b) => {
                Expr::Assign(Box::new(self.expr(a, d)), assign_op(op), Box::new(self.expr(b, d)))
            }
            g::ExprData::Bracket(a, b) => Expr::Index(Box::new(self.expr(a, d)), Box::new(self.expr(b, d))),
            g::ExprData::FunCall(f, args) => {
                let callee = match &f.content {
                    g::FunIdentifierData::TypeSpecifier(t) => {
                        if t.array_specifier.is_some() {
                            Callee::ArrayCtor(self.type_spec(t))
                        } else {
                            Callee::Name(type_name_of(&t.ty))
                        }
                    }
                    g::FunIdentifierData::Expr(fe) => match &fe.content {
                        g::ExprData::Variable(id) => Callee::Name(id.0.to_string()),
                        g::ExprData::Dot(recv, m) => Callee::Method(Box::new(self.expr(recv, d)), m.0.to_string()),
                        _ => {
                            let line = self.lines.span_line(e);
                            self.fail(line, "unsupported function call syntax");
                            Callee::Name("sb_invalid".into())
                        }
                    },
                };
                Expr::Call(callee, args.iter().map(|a| self.expr(a, d)).collect())
            }
            g::ExprData::Dot(a, f) => Expr::Field(Box::new(self.expr(a, d)), f.0.to_string()),
            g::ExprData::PostInc(a) => Expr::PostInc(Box::new(self.expr(a, d))),
            g::ExprData::PostDec(a) => Expr::PostDec(Box::new(self.expr(a, d))),
            g::ExprData::Comma(a, b) => Expr::Comma(Box::new(self.expr(a, d)), Box::new(self.expr(b, d))),
        }
    }
}

fn precision(p: &g::PrecisionQualifier) -> Precision {
    match p.content {
        g::PrecisionQualifierData::High => Precision::High,
        g::PrecisionQualifierData::Medium => Precision::Medium,
        g::PrecisionQualifierData::Low => Precision::Low,
    }
}

fn unary(op: &g::UnaryOp) -> UnaryOp {
    match op.content {
        g::UnaryOpData::Inc => UnaryOp::Inc,
        g::UnaryOpData::Dec => UnaryOp::Dec,
        g::UnaryOpData::Add => UnaryOp::Plus,
        g::UnaryOpData::Minus => UnaryOp::Minus,
        g::UnaryOpData::Not => UnaryOp::Not,
        g::UnaryOpData::Complement => UnaryOp::Complement,
    }
}

fn binary(op: &g::BinaryOp) -> BinaryOp {
    match op.content {
        g::BinaryOpData::Or => BinaryOp::Or,
        g::BinaryOpData::Xor => BinaryOp::Xor,
        g::BinaryOpData::And => BinaryOp::And,
        g::BinaryOpData::BitOr => BinaryOp::BitOr,
        g::BinaryOpData::BitXor => BinaryOp::BitXor,
        g::BinaryOpData::BitAnd => BinaryOp::BitAnd,
        g::BinaryOpData::Equal => BinaryOp::Equal,
        g::BinaryOpData::NonEqual => BinaryOp::NonEqual,
        g::BinaryOpData::Lt => BinaryOp::Lt,
        g::BinaryOpData::Gt => BinaryOp::Gt,
        g::BinaryOpData::Lte => BinaryOp::Lte,
        g::BinaryOpData::Gte => BinaryOp::Gte,
        g::BinaryOpData::LShift => BinaryOp::LShift,
        g::BinaryOpData::RShift => BinaryOp::RShift,
        g::BinaryOpData::Add => BinaryOp::Add,
        g::BinaryOpData::Sub => BinaryOp::Sub,
        g::BinaryOpData::Mult => BinaryOp::Mult,
        g::BinaryOpData::Div => BinaryOp::Div,
        g::BinaryOpData::Mod => BinaryOp::Mod,
    }
}

fn assign_op(op: &g::AssignmentOp) -> AssignOp {
    match op.content {
        g::AssignmentOpData::Equal => AssignOp::Equal,
        g::AssignmentOpData::Mult => AssignOp::Mult,
        g::AssignmentOpData::Div => AssignOp::Div,
        g::AssignmentOpData::Mod => AssignOp::Mod,
        g::AssignmentOpData::Add => AssignOp::Add,
        g::AssignmentOpData::Sub => AssignOp::Sub,
        g::AssignmentOpData::LShift => AssignOp::LShift,
        g::AssignmentOpData::RShift => AssignOp::RShift,
        g::AssignmentOpData::And => AssignOp::And,
        g::AssignmentOpData::Xor => AssignOp::Xor,
        g::AssignmentOpData::Or => AssignOp::Or,
    }
}

/// Parse a standalone expression (generated code, profile semantics).
pub(crate) fn parse_expr(text: &str) -> Result<Expr, ParseFailure> {
    let src = format!("void sb_parse_expr() {{ sb_parse_expr_sink({text}); }}\n");
    let unit = parse_glsl(&src, 460)?;
    for item in unit.items {
        if let ItemKind::Function(mut f) = item.kind
            && let Some(Stmt { kind: StmtKind::Expr(Expr::Call(_, mut args)), .. }) = f.body.pop()
            && args.len() == 1
        {
            return Ok(args.remove(0));
        }
    }
    Err(ParseFailure { line: 0, message: format!("`{text}` is not a single expression") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_comments() {
        for src in [
            "/***/\nvoid main() {}\n",
            "/****/\nvoid main() {}\n",
            "/*****************/\nvoid main() {}\n",
            "/* a **/\nvoid main() {}\n",
            "float x = 1e-8+y;\n",
            "float x = (1e-8+y);\n",
            "float x = 1e-8 + y;\n",
            "struct S { uint a; };\nlayout(binding=2) readonly buffer B { S m[]; };\nvoid f(const in int i) { S s = m[i]; }\n",
            "struct S { uint a; };\nvoid f(const in int i) { S s; }\n",
            "struct S { uint a; };\nlayout(binding=2) readonly buffer B { S m[]; };\nvoid f() { S s; }\n",
            "struct S { uint a; };\nuniform B { S m[2]; };\nvoid f() { S s; }\n",
            "struct S { uint a; };\nuniform B { S m[2]; } b;\nvoid f() { S s; }\n",
        ] {
            eprintln!("{src:?} -> {:?}", parse_glsl(src, 330).err());
        }
    }
}
