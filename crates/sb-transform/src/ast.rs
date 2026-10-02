//! ShaderBridge's own compact GLSL syntax tree.
//!
//! glsl-lang's AST is converted into this tree after parsing (see `parse.rs`). It is
//! owned, has no span wrappers, records the source line of every top-level item and
//! statement (for the line map), and is printed by `print.rs`.

/// A 1-based line of the parsed text (the preprocessed code). 0 = generated code.
pub type Line = u32;

/// A whole shader.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranslationUnit {
    /// External declarations in source order.
    pub items: Vec<Item>,
}

/// An external declaration with its source line.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// What it is.
    pub kind: ItemKind,
    /// First source line (0 = generated).
    pub line: Line,
}

/// External declaration kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// Function definition.
    Function(FunctionDef),
    /// Function prototype (`T f(...);`).
    Prototype(Prototype),
    /// Variable / struct declaration (`uniform float x;`, `struct S {...};`, `in vec2 uv;`).
    Decl(Declaration),
    /// Interface block (`uniform B {...} b;`, `buffer B {...};`, `out V {...} v;`).
    Block(InterfaceBlock),
    /// `precision highp float;`
    Precision(Precision, TypeSpec),
    /// `invariant gl_Position;`
    Invariant(String),
    /// Qualifier-only declaration (`layout(local_size_x = 8) in;`).
    QualifierOnly(Vec<Qualifier>),
    /// Generated text, printed verbatim.
    Raw(String),
}

/// A function definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    /// Signature.
    pub proto: Prototype,
    /// Body statements.
    pub body: Vec<Stmt>,
}

/// A function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct Prototype {
    /// Return type (qualifiers allowed, e.g. `highp`).
    pub ret: FullType,
    /// Function name.
    pub name: String,
    /// Parameters.
    pub params: Vec<Param>,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Qualifiers (`in`, `out`, `inout`, `const`, precision).
    pub quals: Vec<Qualifier>,
    /// Type.
    pub ty: TypeSpec,
    /// Name (may be absent in prototypes).
    pub name: Option<String>,
    /// Array dimensions written after the name.
    pub array: Vec<ArrayDim>,
}

/// Qualifiers plus a type.
#[derive(Debug, Clone, PartialEq)]
pub struct FullType {
    /// Qualifiers in source order.
    pub quals: Vec<Qualifier>,
    /// The type.
    pub ty: TypeSpec,
}

/// A type: a named type (builtin or struct name) or an inline struct, with array
/// dimensions written on the type (`float[3]`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeSpec {
    /// Base type.
    pub base: TypeBase,
    /// Array dimensions on the type itself.
    pub array: Vec<ArrayDim>,
}

/// Base of a [`TypeSpec`].
#[derive(Debug, Clone, PartialEq)]
pub enum TypeBase {
    /// `vec4`, `sampler2D`, a struct name, ...
    Named(String),
    /// Inline struct definition.
    Struct(Box<StructDef>),
}

/// A struct definition.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    /// Struct name (anonymous structs have none).
    pub name: Option<String>,
    /// Members.
    pub fields: Vec<Field>,
}

/// A struct or block member declaration (`T a, b[2];`).
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Member qualifiers (layout, precision, interpolation).
    pub quals: Vec<Qualifier>,
    /// Member type.
    pub ty: TypeSpec,
    /// Names with their own array dimensions.
    pub names: Vec<(String, Vec<ArrayDim>)>,
}

/// One array dimension.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrayDim {
    /// `[]`
    Unsized,
    /// `[expr]`
    Sized(Expr),
}

/// A declaration of zero or more variables of one type.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// Qualified type.
    pub ty: FullType,
    /// Declared variables (empty for `struct S {...};`).
    pub vars: Vec<Declarator>,
}

/// One declared variable.
#[derive(Debug, Clone, PartialEq)]
pub struct Declarator {
    /// Name.
    pub name: String,
    /// Array dimensions after the name.
    pub array: Vec<ArrayDim>,
    /// Initializer.
    pub init: Option<Init>,
}

/// An initializer.
#[derive(Debug, Clone, PartialEq)]
pub enum Init {
    /// `= expr`
    Expr(Expr),
    /// `= { ... }`
    List(Vec<Init>),
}

/// An interface block.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceBlock {
    /// Qualifiers (`uniform`, `buffer`, `in`, `out`, layout, memory qualifiers).
    pub quals: Vec<Qualifier>,
    /// Block name.
    pub name: String,
    /// Members.
    pub fields: Vec<Field>,
    /// Instance name and array dimensions.
    pub instance: Option<(String, Vec<ArrayDim>)>,
}

/// A type qualifier.
#[derive(Debug, Clone, PartialEq)]
pub enum Qualifier {
    /// Storage / memory / auxiliary qualifier.
    Storage(Storage),
    /// `layout(...)`
    Layout(Vec<LayoutId>),
    /// `lowp`/`mediump`/`highp`
    Precision(Precision),
    /// `smooth`/`flat`/`noperspective`
    Interp(Interp),
    /// `invariant`
    Invariant,
    /// `precise`
    Precise,
}

/// Storage qualifiers (including memory and auxiliary qualifiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Storage {
    /// `const`
    Const,
    /// `inout`
    InOut,
    /// `in`
    In,
    /// `out`
    Out,
    /// `centroid`
    Centroid,
    /// `patch`
    Patch,
    /// `sample`
    Sample,
    /// `uniform`
    Uniform,
    /// `buffer`
    Buffer,
    /// `shared`
    Shared,
    /// `coherent`
    Coherent,
    /// `volatile`
    Volatile,
    /// `restrict`
    Restrict,
    /// `readonly`
    ReadOnly,
    /// `writeonly`
    WriteOnly,
    /// `attribute`
    Attribute,
    /// `varying`
    Varying,
    /// `subroutine(...)`
    Subroutine(Vec<String>),
}

/// A `layout(...)` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutId {
    /// Identifier (`location`, `std140`, `rgba16f`, `shared`, ...).
    pub name: String,
    /// Value (`= expr`).
    pub value: Option<Expr>,
}

/// Precision qualifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// `highp`
    High,
    /// `mediump`
    Medium,
    /// `lowp`
    Low,
}

/// Interpolation qualifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interp {
    /// `smooth`
    Smooth,
    /// `flat`
    Flat,
    /// `noperspective`
    NoPerspective,
}

/// A statement with its source line.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// What it is.
    pub kind: StmtKind,
    /// Source line (0 = generated).
    pub line: Line,
}

/// Statement kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// Local declaration.
    Decl(Declaration),
    /// Expression statement.
    Expr(Expr),
    /// `;`
    Empty,
    /// `{ ... }`
    Block(Vec<Stmt>),
    /// `if (cond) then else els`
    If {
        /// Condition.
        cond: Expr,
        /// Then branch.
        then: Box<Stmt>,
        /// Else branch.
        els: Option<Box<Stmt>>,
    },
    /// `switch (expr) { body }`
    Switch {
        /// Selector.
        expr: Expr,
        /// Body (case labels are statements).
        body: Vec<Stmt>,
    },
    /// `case expr:`
    Case(Expr),
    /// `default:`
    Default,
    /// `while (cond) body`
    While {
        /// Condition.
        cond: Condition,
        /// Body.
        body: Box<Stmt>,
    },
    /// `do body while (cond);`
    DoWhile {
        /// Body.
        body: Box<Stmt>,
        /// Condition.
        cond: Expr,
    },
    /// `for (init; cond; step) body`
    For {
        /// Init statement (declaration or expression).
        init: Option<Box<Stmt>>,
        /// Condition.
        cond: Option<Condition>,
        /// Step expression.
        step: Option<Expr>,
        /// Body.
        body: Box<Stmt>,
    },
    /// `return [expr];`
    Return(Option<Expr>),
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `discard;`
    Discard,
    /// Generated statement text, printed verbatim.
    Raw(String),
}

/// A loop condition.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// Plain expression.
    Expr(Expr),
    /// `T name = init` (declaration condition).
    Decl {
        /// Declared type.
        ty: FullType,
        /// Variable name.
        name: String,
        /// Initializer.
        init: Init,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `++x`
    Inc,
    /// `--x`
    Dec,
    /// `+x`
    Plus,
    /// `-x`
    Minus,
    /// `!x`
    Not,
    /// `~x`
    Complement,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `||`
    Or,
    /// `^^`
    Xor,
    /// `&&`
    And,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `&`
    BitAnd,
    /// `==`
    Equal,
    /// `!=`
    NonEqual,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Lte,
    /// `>=`
    Gte,
    /// `<<`
    LShift,
    /// `>>`
    RShift,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mult,
    /// `/`
    Div,
    /// `%`
    Mod,
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Equal,
    /// `*=`
    Mult,
    /// `/=`
    Div,
    /// `%=`
    Mod,
    /// `+=`
    Add,
    /// `-=`
    Sub,
    /// `<<=`
    LShift,
    /// `>>=`
    RShift,
    /// `&=`
    And,
    /// `^=`
    Xor,
    /// `|=`
    Or,
}

impl AssignOp {
    /// The binary operator of a compound assignment (`+=` -> `+`).
    pub fn binary(self) -> Option<BinaryOp> {
        Some(match self {
            Self::Equal => return None,
            Self::Mult => BinaryOp::Mult,
            Self::Div => BinaryOp::Div,
            Self::Mod => BinaryOp::Mod,
            Self::Add => BinaryOp::Add,
            Self::Sub => BinaryOp::Sub,
            Self::LShift => BinaryOp::LShift,
            Self::RShift => BinaryOp::RShift,
            Self::And => BinaryOp::BitAnd,
            Self::Xor => BinaryOp::BitXor,
            Self::Or => BinaryOp::BitOr,
        })
    }
}

/// What a call expression calls.
#[derive(Debug, Clone, PartialEq)]
pub enum Callee {
    /// A function, builtin or constructor by name (`texture`, `vec4`, `MyStruct`).
    Name(String),
    /// An array constructor (`float[3](...)`).
    ArrayCtor(TypeSpec),
    /// A method call (`arr.length()`).
    Method(Box<Expr>, String),
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Identifier reference.
    Ident(String),
    /// `int` literal.
    Int(i32),
    /// `uint` literal.
    UInt(u32),
    /// `bool` literal.
    Bool(bool),
    /// `float` literal.
    Float(f32),
    /// `double` literal.
    Double(f64),
    /// Prefix unary operation.
    Unary(UnaryOp, Box<Expr>),
    /// Binary operation.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `c ? a : b`
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Assignment.
    Assign(Box<Expr>, AssignOp, Box<Expr>),
    /// `a[i]`
    Index(Box<Expr>, Box<Expr>),
    /// Function call / constructor.
    Call(Callee, Vec<Expr>),
    /// `a.b` (member or swizzle)
    Field(Box<Expr>, String),
    /// `a++`
    PostInc(Box<Expr>),
    /// `a--`
    PostDec(Box<Expr>),
    /// `a, b`
    Comma(Box<Expr>, Box<Expr>),
    /// Generated expression text, printed verbatim inside parentheses.
    Raw(String),
}

/// What a pre-order expression visitor wants next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    /// Visit the children.
    Children,
    /// Skip the children.
    Skip,
}

impl Expr {
    /// `Ident(name)`.
    pub fn ident(name: impl Into<String>) -> Self {
        Self::Ident(name.into())
    }

    /// A call by name.
    pub fn call(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self::Call(Callee::Name(name.into()), args)
    }

    /// Generated expression text.
    pub fn raw(text: impl Into<String>) -> Self {
        Self::Raw(text.into())
    }

    /// The identifier name if this is an identifier.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            Self::Ident(s) => Some(s),
            _ => None,
        }
    }

    /// The callee name if this is a call by name.
    pub fn call_name(&self) -> Option<&str> {
        match self {
            Self::Call(Callee::Name(n), _) => Some(n),
            _ => None,
        }
    }

    /// Pre-order mutable walk over this expression and its sub-expressions (array
    /// constructor sizes and method receivers included).
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        if f(self) == Walk::Skip {
            return;
        }
        self.walk_children_mut(f);
    }

    /// Walk only the children of this expression (see [`Expr::walk_mut`]).
    pub fn walk_children_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        match self {
            Expr::Ident(_) | Expr::Int(_) | Expr::UInt(_) | Expr::Bool(_) | Expr::Float(_) | Expr::Double(_) | Expr::Raw(_) => {}
            Expr::Unary(_, a) | Expr::PostInc(a) | Expr::PostDec(a) | Expr::Field(a, _) => a.walk_mut(f),
            Expr::Binary(_, a, b) | Expr::Assign(a, _, b) | Expr::Index(a, b) | Expr::Comma(a, b) => {
                a.walk_mut(f);
                b.walk_mut(f);
            }
            Expr::Ternary(a, b, c) => {
                a.walk_mut(f);
                b.walk_mut(f);
                c.walk_mut(f);
            }
            Expr::Call(callee, args) => {
                match callee {
                    Callee::Name(_) => {}
                    Callee::ArrayCtor(t) => t.walk_exprs_mut(f),
                    Callee::Method(r, _) => r.walk_mut(f),
                }
                for a in args {
                    a.walk_mut(f);
                }
            }
        }
    }

    /// Pre-order immutable walk.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        if f(self) == Walk::Skip {
            return;
        }
        match self {
            Expr::Ident(_) | Expr::Int(_) | Expr::UInt(_) | Expr::Bool(_) | Expr::Float(_) | Expr::Double(_) | Expr::Raw(_) => {}
            Expr::Unary(_, a) | Expr::PostInc(a) | Expr::PostDec(a) | Expr::Field(a, _) => a.walk(f),
            Expr::Binary(_, a, b) | Expr::Assign(a, _, b) | Expr::Index(a, b) | Expr::Comma(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::Ternary(a, b, c) => {
                a.walk(f);
                b.walk(f);
                c.walk(f);
            }
            Expr::Call(callee, args) => {
                match callee {
                    Callee::Name(_) => {}
                    Callee::ArrayCtor(t) => t.walk_exprs(f),
                    Callee::Method(r, _) => r.walk(f),
                }
                for a in args {
                    a.walk(f);
                }
            }
        }
    }
}

impl TypeSpec {
    /// A named type without array dimensions.
    pub fn named(name: impl Into<String>) -> Self {
        Self { base: TypeBase::Named(name.into()), array: Vec::new() }
    }

    /// The type name if this is a named type.
    pub fn name(&self) -> Option<&str> {
        match &self.base {
            TypeBase::Named(n) => Some(n),
            TypeBase::Struct(_) => None,
        }
    }

    /// Walk the expressions inside this type (array sizes, inline struct members).
    pub fn walk_exprs_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        for d in &mut self.array {
            d.walk_mut(f);
        }
        if let TypeBase::Struct(s) = &mut self.base {
            for field in &mut s.fields {
                field.ty.walk_exprs_mut(f);
                for (_, dims) in &mut field.names {
                    for d in dims {
                        d.walk_mut(f);
                    }
                }
            }
        }
    }

    /// Immutable variant of [`TypeSpec::walk_exprs_mut`].
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        for d in &self.array {
            if let ArrayDim::Sized(e) = d {
                e.walk(f);
            }
        }
        if let TypeBase::Struct(s) = &self.base {
            for field in &s.fields {
                field.ty.walk_exprs(f);
                for (_, dims) in &field.names {
                    for d in dims {
                        if let ArrayDim::Sized(e) = d {
                            e.walk(f);
                        }
                    }
                }
            }
        }
    }
}

impl ArrayDim {
    fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        if let ArrayDim::Sized(e) = self {
            e.walk_mut(f);
        }
    }
}

impl Init {
    /// Walk the expressions of the initializer.
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        match self {
            Init::Expr(e) => e.walk_mut(f),
            Init::List(l) => {
                for i in l {
                    i.walk_mut(f);
                }
            }
        }
    }

    /// Immutable walk.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        match self {
            Init::Expr(e) => e.walk(f),
            Init::List(l) => {
                for i in l {
                    i.walk(f);
                }
            }
        }
    }
}

impl FullType {
    /// Unqualified type.
    pub fn plain(ty: TypeSpec) -> Self {
        Self { quals: Vec::new(), ty }
    }

    /// Whether a storage qualifier is present.
    pub fn has_storage(&self, s: &Storage) -> bool {
        has_storage(&self.quals, s)
    }
}

/// Whether `quals` contain the storage qualifier `s`.
pub fn has_storage(quals: &[Qualifier], s: &Storage) -> bool {
    quals.iter().any(|q| matches!(q, Qualifier::Storage(x) if x == s))
}

/// The value of layout identifier `name` (case-insensitive key) in `quals`, if present.
pub fn layout_value<'a>(quals: &'a [Qualifier], name: &str) -> Option<Option<&'a Expr>> {
    quals.iter().rev().find_map(|q| match q {
        Qualifier::Layout(ids) => ids.iter().rev().find(|l| l.name.eq_ignore_ascii_case(name)).map(|l| l.value.as_ref()),
        _ => None,
    })
}

/// The interpolation qualifier in `quals`.
pub fn interpolation(quals: &[Qualifier]) -> Option<Interp> {
    quals.iter().find_map(|q| match q {
        Qualifier::Interp(i) => Some(*i),
        _ => None,
    })
}

impl Declaration {
    /// Walk every expression (type array sizes, declarator sizes, initializers).
    pub fn walk_exprs_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        self.ty.ty.walk_exprs_mut(f);
        for v in &mut self.vars {
            for d in &mut v.array {
                d.walk_mut(f);
            }
            if let Some(i) = &mut v.init {
                i.walk_mut(f);
            }
        }
    }
}

impl Stmt {
    /// A statement at `line`.
    pub fn new(kind: StmtKind, line: Line) -> Self {
        Self { kind, line }
    }

    /// Generated statement text.
    pub fn raw(text: impl Into<String>) -> Self {
        Self { kind: StmtKind::Raw(text.into()), line: 0 }
    }

    /// Walk every expression in this statement and nested statements (pre-order per
    /// expression tree).
    pub fn walk_exprs_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        match &mut self.kind {
            StmtKind::Decl(d) => d.walk_exprs_mut(f),
            StmtKind::Expr(e) | StmtKind::Case(e) => e.walk_mut(f),
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    e.walk_mut(f);
                }
            }
            StmtKind::Switch { expr, body } => {
                expr.walk_mut(f);
                for s in body {
                    s.walk_exprs_mut(f);
                }
            }
            StmtKind::Block(b) => {
                for s in b {
                    s.walk_exprs_mut(f);
                }
            }
            StmtKind::If { cond, then, els } => {
                cond.walk_mut(f);
                then.walk_exprs_mut(f);
                if let Some(e) = els {
                    e.walk_exprs_mut(f);
                }
            }
            StmtKind::While { cond, body } => {
                cond.walk_mut(f);
                body.walk_exprs_mut(f);
            }
            StmtKind::DoWhile { body, cond } => {
                body.walk_exprs_mut(f);
                cond.walk_mut(f);
            }
            StmtKind::For { init, cond, step, body } => {
                if let Some(i) = init {
                    i.walk_exprs_mut(f);
                }
                if let Some(c) = cond {
                    c.walk_mut(f);
                }
                if let Some(s) = step {
                    s.walk_mut(f);
                }
                body.walk_exprs_mut(f);
            }
            StmtKind::Empty | StmtKind::Break | StmtKind::Continue | StmtKind::Discard | StmtKind::Raw(_) => {}
        }
    }

    /// Visit this statement and every nested statement (pre-order).
    pub fn walk_stmts_mut(&mut self, f: &mut dyn FnMut(&mut Stmt)) {
        f(self);
        match &mut self.kind {
            StmtKind::Block(b) | StmtKind::Switch { body: b, .. } => {
                for s in b {
                    s.walk_stmts_mut(f);
                }
            }
            StmtKind::If { then, els, .. } => {
                then.walk_stmts_mut(f);
                if let Some(e) = els {
                    e.walk_stmts_mut(f);
                }
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => body.walk_stmts_mut(f),
            StmtKind::For { init, body, .. } => {
                if let Some(i) = init {
                    i.walk_stmts_mut(f);
                }
                body.walk_stmts_mut(f);
            }
            _ => {}
        }
    }

    /// Immutable statement walk (pre-order).
    pub fn walk_stmts(&self, f: &mut dyn FnMut(&Stmt)) {
        f(self);
        match &self.kind {
            StmtKind::Block(b) | StmtKind::Switch { body: b, .. } => {
                for s in b {
                    s.walk_stmts(f);
                }
            }
            StmtKind::If { then, els, .. } => {
                then.walk_stmts(f);
                if let Some(e) = els {
                    e.walk_stmts(f);
                }
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => body.walk_stmts(f),
            StmtKind::For { init, body, .. } => {
                if let Some(i) = init {
                    i.walk_stmts(f);
                }
                body.walk_stmts(f);
            }
            _ => {}
        }
    }

    /// Immutable walk over every expression of this statement tree.
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        self.walk_stmts(&mut |s| match &s.kind {
            StmtKind::Decl(d) => {
                d.ty.ty.walk_exprs(f);
                for v in &d.vars {
                    for dim in &v.array {
                        if let ArrayDim::Sized(e) = dim {
                            e.walk(f);
                        }
                    }
                    if let Some(i) = &v.init {
                        i.walk(f);
                    }
                }
            }
            StmtKind::Expr(e) | StmtKind::Case(e) | StmtKind::Switch { expr: e, .. } | StmtKind::If { cond: e, .. } => e.walk(f),
            StmtKind::DoWhile { cond, .. } => cond.walk(f),
            StmtKind::Return(Some(e)) => e.walk(f),
            StmtKind::While { cond, .. } => cond.walk(f),
            StmtKind::For { cond, step, .. } => {
                if let Some(c) = cond {
                    c.walk(f);
                }
                if let Some(s) = step {
                    s.walk(f);
                }
            }
            _ => {}
        });
    }
}

impl Condition {
    fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        match self {
            Condition::Expr(e) => e.walk_mut(f),
            Condition::Decl { init, .. } => init.walk_mut(f),
        }
    }

    fn walk(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        match self {
            Condition::Expr(e) => e.walk(f),
            Condition::Decl { init, .. } => init.walk(f),
        }
    }
}

impl TranslationUnit {
    /// Walk every expression of every item (global initializers, array sizes, layout
    /// values excluded, function bodies).
    pub fn walk_exprs_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        for item in &mut self.items {
            item.walk_exprs_mut(f);
        }
    }

    /// Immutable variant of [`TranslationUnit::walk_exprs_mut`].
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        for item in &self.items {
            item.walk_exprs(f);
        }
    }

    /// The function definitions.
    pub fn functions(&self) -> impl Iterator<Item = &FunctionDef> {
        self.items.iter().filter_map(|i| match &i.kind {
            ItemKind::Function(f) => Some(f),
            _ => None,
        })
    }

    /// Mutable function definitions.
    pub fn functions_mut(&mut self) -> impl Iterator<Item = &mut FunctionDef> {
        self.items.iter_mut().filter_map(|i| match &mut i.kind {
            ItemKind::Function(f) => Some(f),
            _ => None,
        })
    }
}

impl Item {
    /// Walk the expressions of this item.
    pub fn walk_exprs_mut(&mut self, f: &mut dyn FnMut(&mut Expr) -> Walk) {
        match &mut self.kind {
            ItemKind::Function(func) => {
                walk_proto_mut(&mut func.proto, f);
                for s in &mut func.body {
                    s.walk_exprs_mut(f);
                }
            }
            ItemKind::Prototype(p) => walk_proto_mut(p, f),
            ItemKind::Decl(d) => d.walk_exprs_mut(f),
            ItemKind::Block(b) => {
                for field in &mut b.fields {
                    field.ty.walk_exprs_mut(f);
                    for (_, dims) in &mut field.names {
                        for d in dims {
                            d.walk_mut(f);
                        }
                    }
                }
                if let Some((_, dims)) = &mut b.instance {
                    for d in dims {
                        d.walk_mut(f);
                    }
                }
            }
            ItemKind::Precision(..) | ItemKind::Invariant(_) | ItemKind::QualifierOnly(_) | ItemKind::Raw(_) => {}
        }
    }

    /// Immutable walk.
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr) -> Walk) {
        match &self.kind {
            ItemKind::Function(func) => {
                walk_proto(&func.proto, f);
                for s in &func.body {
                    s.walk_exprs(f);
                }
            }
            ItemKind::Prototype(p) => walk_proto(p, f),
            ItemKind::Decl(d) => {
                d.ty.ty.walk_exprs(f);
                for v in &d.vars {
                    for dim in &v.array {
                        if let ArrayDim::Sized(e) = dim {
                            e.walk(f);
                        }
                    }
                    if let Some(i) = &v.init {
                        i.walk(f);
                    }
                }
            }
            ItemKind::Block(b) => {
                for field in &b.fields {
                    field.ty.walk_exprs(f);
                    for (_, dims) in &field.names {
                        for d in dims {
                            if let ArrayDim::Sized(e) = d {
                                e.walk(f);
                            }
                        }
                    }
                }
                if let Some((_, dims)) = &b.instance {
                    for d in dims {
                        if let ArrayDim::Sized(e) = d {
                            e.walk(f);
                        }
                    }
                }
            }
            ItemKind::Precision(..) | ItemKind::Invariant(_) | ItemKind::QualifierOnly(_) | ItemKind::Raw(_) => {}
        }
    }
}

fn walk_proto_mut(p: &mut Prototype, f: &mut dyn FnMut(&mut Expr) -> Walk) {
    p.ret.ty.walk_exprs_mut(f);
    for param in &mut p.params {
        param.ty.walk_exprs_mut(f);
        for d in &mut param.array {
            d.walk_mut(f);
        }
    }
}

fn walk_proto(p: &Prototype, f: &mut dyn FnMut(&Expr) -> Walk) {
    p.ret.ty.walk_exprs(f);
    for param in &p.params {
        param.ty.walk_exprs(f);
        for d in &param.array {
            if let ArrayDim::Sized(e) = d {
                e.walk(f);
            }
        }
    }
}
