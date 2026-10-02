//! Protection against inputs that would overflow glslang's native stack.
//!
//! glslang parses with an explicit (bison) stack, so deeply *nested* code is
//! rejected cleanly ("memory exhausted"), but its tree passes (constant folding,
//! SPIR-V generation, ...) recurse once per level of an expression tree.
//! Left-associative chains such as `a + a + a + ...` build trees as deep as the
//! chain is long, at about 0.75 KiB of native stack per level (measured: a
//! 90,000-term sum fits in 64 MiB, 16,000 terms overflow 8 MiB) and about
//! 1.2 KiB per level of a comma-operator chain (8,000 overflow 8 MiB). A stack
//! overflow in C++ code aborts the process, which must never happen inside the
//! game.
//!
//! sb-compile therefore runs glslang on a dedicated thread with a large stack
//! and, after glslang's preprocessor has expanded all macros, rejects sources
//! whose longest statement could build a tree deeper than that stack can handle.
//!
//! The estimate counts the tokens that can add a level to glslang's tree:
//! operators, member accesses, indexing, function calls and comma *operators*
//! (which glslang chains left-deep; they count double because each level uses
//! about twice the stack of a binary operator). Call and constructor arguments and the
//! elements of `{ ... }` initializer lists are siblings in one aggregate node,
//! so they contribute the count of the *largest* argument, not the sum (a
//! 20,000-element constant array is flat). Grouping parentheses and the
//! characters of numeric literals (`1.5e-3`) do not count. The result is an
//! upper bound of the tree depth.

/// Keywords that may be followed by `(` without forming a function call.
const NON_CALL_KEYWORDS: [&[u8]; 9] = [b"if", b"while", b"for", b"switch", b"return", b"else", b"do", b"case", b"layout"];

/// What an open parenthesis, bracket or brace belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    /// Function call or constructor arguments: commas separate arguments.
    Call,
    /// Grouping parentheses, control-statement headers, `[...]`: commas are operators.
    Expression,
    /// `= { a, b, ... }` initializer lists (possibly nested): commas separate elements.
    Initializer,
}

/// An open group. For `Call` and `Initializer`, the arguments are counted one at
/// a time and only the largest is kept.
#[derive(Debug, Clone, Copy)]
struct Frame {
    kind: Group,
    /// The count of the enclosing expression when the group opened.
    outer: usize,
    /// The largest count of an argument/element closed so far.
    deepest: usize,
}

impl Frame {
    fn new(kind: Group, outer: usize) -> Self {
        Self { kind, outer, deepest: 0 }
    }
}

/// The count of the whole expression if every open group closed now.
fn fold(frames: &[Frame], count: usize) -> usize {
    frames
        .iter()
        .rev()
        .filter(|f| f.kind != Group::Expression)
        .fold(count, |inner, f| f.outer.saturating_add(f.deepest.max(inner)))
}

/// The previous significant token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prev {
    Nothing,
    /// An identifier or keyword: `bytes[start..end]`.
    Ident(usize, usize),
    Number,
    /// `)`, `]` or the `}` of an initializer list.
    Close,
    /// Any other punctuation or operator character.
    Punct(u8),
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Characters that start an operator token.
fn is_operator(b: u8) -> bool {
    matches!(b, b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^' | b'?' | b':' | b'~')
}

/// Length of the operator token at the start of `rest` (longest match, as the
/// GLSL lexer does: `<<=`, `==`, `||`, `++`, ...).
fn operator_len(rest: &[u8]) -> usize {
    const LONG: [&[u8]; 2] = [b"<<=", b">>="];
    const PAIRS: [&[u8]; 19] = [
        b"++", b"--", b"<<", b">>", b"<=", b">=", b"==", b"!=", b"&&", b"||", b"^^", b"+=", b"-=", b"*=", b"/=", b"%=",
        b"&=", b"|=", b"^=",
    ];
    if LONG.iter().any(|op| rest.starts_with(op)) {
        3
    } else if PAIRS.iter().any(|op| rest.starts_with(op)) {
        2
    } else {
        1
    }
}

/// End of the numeric literal starting at `i` (`12`, `1.5`, `.5`, `1e-3`,
/// `2.0f`, `0x1Fu`, `1.0lf`).
fn skip_number(bytes: &[u8], mut i: usize) -> usize {
    let hex = bytes.get(i) == Some(&b'0') && matches!(bytes.get(i + 1), Some(b'x' | b'X'));
    while let Some(&b) = bytes.get(i) {
        if is_ident_char(b) || b == b'.' {
            i += 1;
            if !hex && matches!(b, b'e' | b'E') && matches!(bytes.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
        } else {
            break;
        }
    }
    i
}

/// The largest estimated expression depth of one statement (text between `;`,
/// `{` and `}`), ignoring comments and preprocessor lines, together with the
/// 1-based line where that statement ends (honouring `#line N`, which glslang
/// keeps in its preprocessed output). An upper bound of the expression depth
/// glslang will recurse to.
pub(crate) fn max_statement_operators(text: &str) -> (usize, u32) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line = 1u32;
    let mut at_line_start = true;
    let mut count = 0usize;
    let mut best = (0usize, 1u32);
    let mut frames: Vec<Frame> = Vec::new();
    let mut prev = Prev::Nothing;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' => {
                line += 1;
                at_line_start = true;
                i += 1;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'#' if at_line_start => {
                // `#line N [S]`: the next line is line N.
                if let Some(n) = line_directive(&bytes[i + 1..]) {
                    line = n.saturating_sub(1);
                }
                // Preprocessor line (with `\` continuations).
                while i < bytes.len() && bytes[i] != b'\n' {
                    if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
                        line += 1;
                        i += 1;
                    }
                    i += 1;
                }
                continue;
            }
            _ if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            _ => {}
        }
        at_line_start = false;

        if is_ident_start(b) {
            let start = i;
            while i < bytes.len() && is_ident_char(bytes[i]) {
                i += 1;
            }
            prev = Prev::Ident(start, i);
            continue;
        }
        let starts_number = b.is_ascii_digit()
            || (b == b'.'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
                && !matches!(prev, Prev::Ident(..) | Prev::Close));
        if starts_number {
            i = skip_number(bytes, i);
            prev = Prev::Number;
            continue;
        }

        let top = frames.last().map(|f| f.kind);
        match b {
            b'{' if matches!(prev, Prev::Punct(b'='))
                || (top == Some(Group::Initializer) && matches!(prev, Prev::Punct(b',' | b'{'))) =>
            {
                frames.push(Frame::new(Group::Initializer, count));
                count = 0;
                prev = Prev::Punct(b);
            }
            b'}' if top == Some(Group::Initializer) => {
                count = fold(&frames[frames.len() - 1..], count);
                frames.pop();
                prev = Prev::Close;
            }
            b';' | b'{' | b'}' => {
                let total = fold(&frames, count);
                if total > best.0 {
                    best = (total, line);
                }
                count = 0;
                if b == b';' {
                    // Only `for (;;)` headers legitimately span statements.
                    frames.iter_mut().for_each(|f| *f = Frame::new(f.kind, 0));
                } else {
                    frames.clear();
                }
                prev = Prev::Punct(b);
            }
            b'(' => {
                let call = match prev {
                    Prev::Ident(s, e) => !NON_CALL_KEYWORDS.contains(&&bytes[s..e]),
                    // `float[](...)`, `S[2](...)`: array constructors.
                    Prev::Close => true,
                    _ => false,
                };
                if call {
                    // The call node itself is one level; its arguments are siblings.
                    frames.push(Frame::new(Group::Call, count + 1));
                    count = 0;
                } else {
                    frames.push(Frame::new(Group::Expression, 0));
                }
                prev = Prev::Punct(b);
            }
            b'[' => {
                count += 1;
                frames.push(Frame::new(Group::Expression, 0));
                prev = Prev::Punct(b);
            }
            b')' | b']' => {
                if let Some(f) = frames.pop()
                    && f.kind != Group::Expression
                {
                    count = fold(&[f], count);
                }
                prev = Prev::Close;
            }
            b',' => {
                match frames.last_mut() {
                    Some(f) if f.kind != Group::Expression => {
                        f.deepest = f.deepest.max(count);
                        count = 0;
                    }
                    // The comma operator chains left-deep, at about twice the
                    // stack per level of other operators.
                    _ => count += 2,
                }
                prev = Prev::Punct(b);
            }
            b'.' => {
                count += 1; // member access / swizzle
                prev = Prev::Punct(b);
            }
            _ if is_operator(b) => {
                let len = operator_len(&bytes[i..]);
                count += 1;
                // Only a plain `=` can introduce an initializer list.
                prev = Prev::Punct(if len == 1 { b } else { 0 });
                i += len;
                continue;
            }
            _ => prev = Prev::Punct(b),
        }
        i += 1;
    }
    let total = fold(&frames, count);
    if total > best.0 {
        best = (total, line);
    }
    best
}

/// Largest accepted struct or block type, in members when expanded as a tree.
///
/// glslang's SPIR-V generation and block layout take time exponential in the
/// nesting of struct types that reuse each other (`struct S1 { S0 a; S0 b; }`,
/// `struct S2 { S1 a; S1 b; }`, ...): 24 such levels take 7 s, 40 never finish.
/// Real shaders stay far below this bound.
pub(crate) const MAX_STRUCT_TREE: u64 = 100_000;

/// Simple tokens for [`struct_hazard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum STok<'a> {
    Ident(&'a str),
    Open(u8),
    Close(u8),
    Semi,
    Comma,
    Other,
}

/// Tokenize `text`, skipping comments and preprocessor lines.
fn simple_tokens(text: &str) -> Vec<(STok<'_>, u32)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let mut at_line_start = true;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            at_line_start = true;
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                if bytes[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'#' && at_line_start {
            while i < bytes.len() && bytes[i] != b'\n' {
                if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
                    line += 1;
                    i += 1;
                }
                i += 1;
            }
            continue;
        }
        at_line_start = false;
        if is_ident_start(b) {
            let start = i;
            while i < bytes.len() && is_ident_char(bytes[i]) {
                i += 1;
            }
            out.push((STok::Ident(&text[start..i]), line));
            continue;
        }
        let tok = match b {
            b'{' | b'(' | b'[' => STok::Open(b),
            b'}' | b')' | b']' => STok::Close(b),
            b';' => STok::Semi,
            b',' => STok::Comma,
            _ => STok::Other,
        };
        out.push((tok, line));
        i += 1;
    }
    out
}

/// Find a struct or block type that expands to more than [`MAX_STRUCT_TREE`]
/// members (counting every member of every nested struct, per use). Returns the
/// line of its body and a message.
pub(crate) fn struct_hazard(text: &str) -> Option<(u32, String)> {
    if !text.contains('{') {
        return None;
    }
    let toks = simple_tokens(text);
    let mut sizes: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < toks.len() {
        let (tok, line) = toks[i];
        match tok {
            STok::Open(b'{') => {
                // `struct [Name] {` anywhere, or `Name {` of a block at global scope.
                let prev = i.checked_sub(1).map(|p| toks[p].0);
                let before = i.checked_sub(2).map(|p| toks[p].0);
                let name = match (before, prev) {
                    (_, Some(STok::Ident("struct"))) => Some(""),
                    (Some(STok::Ident("struct")), Some(STok::Ident(n))) => Some(n),
                    (_, Some(STok::Ident(n))) if depth == 0 => Some(n),
                    _ => None,
                };
                if let Some(name) = name {
                    let (size, end) = body_size(&toks, i, &sizes);
                    if size > MAX_STRUCT_TREE {
                        let label = if name.is_empty() { "struct".to_string() } else { format!("'{name}'") };
                        return Some((
                            line,
                            format!(
                                "{label} : type too large to compile safely ({size} members when nested structs are expanded, limit {MAX_STRUCT_TREE})"
                            ),
                        ));
                    }
                    if !name.is_empty() {
                        sizes.insert(name, size);
                    }
                    i = end;
                    continue;
                }
                depth += 1;
            }
            STok::Close(b'}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Expanded size of the body opening at `open` (a `{`), and the index after its `}`.
fn body_size(toks: &[(STok<'_>, u32)], open: usize, sizes: &std::collections::HashMap<&str, u64>) -> (u64, usize) {
    let mut size = 1u64;
    let mut nesting = 0usize; // (), [] and inner {} inside the body
    let mut declarators = 1u64;
    let mut member_type: Option<u64> = None;
    let mut i = open + 1;
    while i < toks.len() {
        match toks[i].0 {
            STok::Open(_) => nesting += 1,
            STok::Close(b'}') if nesting == 0 => {
                if let Some(t) = member_type {
                    size = size.saturating_add(t.saturating_mul(declarators));
                }
                return (size, i + 1);
            }
            STok::Close(_) => nesting = nesting.saturating_sub(1),
            STok::Comma if nesting == 0 => declarators += 1,
            STok::Semi if nesting == 0 => {
                size = size.saturating_add(member_type.unwrap_or(1).saturating_mul(declarators));
                declarators = 1;
                member_type = None;
            }
            STok::Ident(id) if member_type.is_none() => {
                if let Some(&s) = sizes.get(id) {
                    member_type = Some(s);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (size, toks.len())
}

/// The line number of a `#line N ...` directive (`text` starts after the `#`).
fn line_directive(text: &[u8]) -> Option<u32> {
    let end = text.iter().position(|&b| b == b'\n').unwrap_or(text.len());
    let directive = std::str::from_utf8(&text[..end]).ok()?;
    let rest = directive.trim_start().strip_prefix("line")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let number = rest.split_whitespace().next()?;
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(s: &str) -> usize {
        max_statement_operators(s).0
    }

    #[test]
    fn counts_per_statement() {
        assert_eq!(max_statement_operators("a = b + c;\nd = e;"), (2, 1));
        assert_eq!(max_statement_operators("x;\ny = a+a+a+a;\n"), (4, 2));
        // Multi-character operators are one token each; runs of single ones are not.
        assert_eq!(ops("b = x == 1 || y <= 2 && z != 3;"), 6);
        assert_eq!(ops("a <<= b >>= c;"), 2);
        assert_eq!(ops("a = - - -b; c = d+-e; f = g---h; i++;"), 4);
        assert_eq!(max_statement_operators(""), (0, 1));
    }

    #[test]
    fn ignores_comments_and_directives() {
        let src = "#define X a+a+a+a+a\n// +++++++\n/* ++++\n+++ */ y = 1;\n";
        assert_eq!(max_statement_operators(src), (1, 4));
        // `#` not at line start is not a directive.
        assert_eq!(max_statement_operators("a # + + +;"), (3, 1));
        // Continued directive lines.
        assert_eq!(max_statement_operators("#define Y \\\n +++++\nz = 1;"), (1, 3));
    }

    #[test]
    fn numeric_literals_are_not_operators() {
        assert_eq!(ops("x = 1.5;"), 1);
        assert_eq!(ops("x = .5 + 2.;"), 2);
        assert_eq!(ops("x = 1e-3 + 1.5E+10;"), 2);
        assert_eq!(ops("x = 0x1Fu + 3u + 1.0lf + 2.0f;"), 4);
        // A minus after a hex digit `e` is an operator.
        assert_eq!(ops("x = 0x1e-1;"), 2);
        // But a member access or swizzle is.
        assert_eq!(ops("x = v.x + a.b.c;"), 5);
        assert_eq!(ops("x = f().y;"), 3);
    }

    #[test]
    fn argument_commas_are_flat_but_comma_operators_chain() {
        // Constructor / call arguments: one aggregate node.
        assert_eq!(ops("v = vec4(a, b, c, d);"), 2);
        let table = format!("const float t[3000] = float[3000]({});", vec!["0.25"; 3000].join(", "));
        assert_eq!(ops(&table), 4); // `[`, `=`, `[`, the constructor call
        // Comma operators chain left-deep: inside grouping parentheses, brackets,
        // control headers and at statement level they count.
        assert_eq!(ops("x = (a, b, c);"), 5);
        assert_eq!(ops("f((a, b));"), 3);
        assert_eq!(ops("return (a, b, c);"), 4);
        assert_eq!(ops("for (i = 0, j = 0; i < n; i++, j++) {}"), 4);
        assert_eq!(ops("x = a[(b, c)];"), 4);
        // Nested calls nest.
        assert_eq!(ops(&format!("x = {}y{};", "f(".repeat(50), ")".repeat(50))), 51);
        assert_eq!(ops("x = float[](1.0, 2.0)[1];"), 4);
    }

    #[test]
    fn initializer_lists_are_flat() {
        let list = format!("float t[3000] = {{ {} }};", vec!["0.25"; 3000].join(", "));
        assert_eq!(max_statement_operators(&list), (2, 1));
        assert_eq!(ops("S s[2] = { { 1, a + b }, { 3, (c, d) } };"), 4);
        // A block is not an initializer: its statements are counted separately.
        assert_eq!(max_statement_operators("void main() { a = b, c; }\n{ x = y + z + w + v; }"), (4, 2));
    }

    #[test]
    fn arguments_count_their_largest_member() {
        // Siblings: max, not sum.
        assert_eq!(ops("v = vec3(a + b + c, d * e, f);"), 4);
        assert_eq!(ops("v = f(g(a + b, c), h(d + e + f + g));"), 6);
        // Operators around a call add up with its deepest argument.
        assert_eq!(ops("x = a + f(b + c, d) * e;"), 5);
        // Nested chains inside arguments still count fully.
        let chain = format!("x = f({}y, 1.0);", "y + ".repeat(1000));
        assert_eq!(ops(&chain), 1002);
    }

    #[test]
    fn unbalanced_input_is_fine() {
        assert_eq!(ops("x = ))) , ]]] (((;"), 4);
        assert_eq!(ops("f(a, b"), 1);
        assert_eq!(ops("{ f(a, } , b"), 2);
    }

    #[test]
    fn line_directives_renumber() {
        assert_eq!(max_statement_operators("#line 100\na = b + c;\n"), (2, 100));
        assert_eq!(max_statement_operators("x;\n# line 7 2\n\ny = a+a+a;\n"), (3, 8));
        // Not a #line directive / malformed: ignored.
        assert_eq!(max_statement_operators("#lines 100\na = b + c;\n"), (2, 2));
        assert_eq!(max_statement_operators("#line x\na = b + c;\n"), (2, 2));
        assert_eq!(max_statement_operators("#line 99999999999\na = b;\n"), (1, 2));
    }

    #[test]
    fn ordinary_structs_and_blocks_are_fine() {
        let src = "struct Light { vec3 pos; float r; };\nstruct Scene { Light lights[64]; Light sun, moon; };\n\
                   layout(std140) uniform U { Scene scene; mat4 m; } u;\nvoid main() { struct L { float x; } l; }\n";
        assert_eq!(struct_hazard(src), None);
        assert_eq!(struct_hazard("void main() { x = 1; }"), None);
        // Function bodies and control blocks are not types.
        assert_eq!(struct_hazard("float f(float a) { if (a > 0.0) { return a; } return 0.0; }"), None);
    }

    #[test]
    fn exponential_struct_nesting_is_rejected() {
        let mut src = String::from("struct S0 { float v; };\n");
        for i in 1..=40 {
            src.push_str(&format!("struct S{i} {{ S{p} a; S{p} b; }};\n", p = i - 1));
        }
        let (line, msg) = struct_hazard(&src).unwrap();
        // S16 is the first to exceed 100,000 (2^17 - 1 + ... members).
        assert_eq!(line, 17, "{msg}");
        assert!(msg.starts_with("'S16' : type too large"), "{msg}");
        // Declarator lists count each declarator; arrays do not multiply.
        let mut src = String::from("struct S0 { float v; };\n");
        for i in 1..=40 {
            src.push_str(&format!("struct S{i} {{ S{p} a[1000]; }};\n", p = i - 1));
        }
        assert_eq!(struct_hazard(&src), None);
        let mut src = String::from("struct S0 { float v; };\n");
        for i in 1..=12 {
            src.push_str(&format!("struct S{i} {{ S{p} a, b, c, d; }};\n", p = i - 1));
        }
        assert!(struct_hazard(&src).is_some());
        // Blocks count too, and local struct definitions are seen.
        let block = format!("struct S0 {{ float v; }};\nuniform U {{ {} }};\n", "S0 m; ".repeat(60_000));
        assert!(struct_hazard(&block).unwrap().1.starts_with("'U' : type too large"));
        let mut local = String::from("void main() {\nstruct S0 { float v; };\n");
        for i in 1..=20 {
            local.push_str(&format!("struct S{i} {{ S{p} a; S{p} b; }};\n", p = i - 1));
        }
        assert!(struct_hazard(&local).is_some());
    }

    #[test]
    fn unterminated_input_is_fine() {
        assert_eq!(max_statement_operators("a + b"), (1, 1));
        assert_eq!(max_statement_operators("/* never closed + + +"), (0, 1));
        assert_eq!(max_statement_operators("#"), (0, 1));
        assert_eq!(max_statement_operators("x = 1e"), (1, 1));
        assert_eq!(max_statement_operators("x = 1e-"), (1, 1));
        assert_eq!(max_statement_operators("."), (1, 1));
    }
}
