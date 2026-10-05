//! Word-level SPIR-V utilities: header checks, instruction walking, byte
//! conversion and debug-info stripping.
//!
//! These never panic on malformed input; every failure is a `String` error.

use spirq::spirv::Op;
use std::collections::HashMap;

/// SPIR-V magic number (first word of every module, in host word order).
pub const MAGIC: u32 = 0x0723_0203;

/// Number of words in a SPIR-V module header.
pub const HEADER_WORDS: usize = 5;

/// The five-word module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Version word, `0x00MMmm00`.
    pub version: u32,
    /// Generator magic (tool that produced the module).
    pub generator: u32,
    /// Upper bound of result ids.
    pub bound: u32,
}

impl Header {
    /// `(major, minor)` of the module's SPIR-V version.
    pub fn version_pair(&self) -> (u8, u8) {
        (((self.version >> 16) & 0xff) as u8, ((self.version >> 8) & 0xff) as u8)
    }
}

/// One instruction: opcode and operand words (without the leading word).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Instr<'a> {
    pub opcode: u32,
    pub operands: &'a [u32],
}

impl Instr<'_> {
    pub fn is(&self, op: Op) -> bool {
        self.opcode == op as u32
    }
    pub fn word(&self, i: usize) -> Option<u32> {
        self.operands.get(i).copied()
    }
}

/// Check the header and split the module into instructions.
pub(crate) fn parse(words: &[u32]) -> Result<(Header, Vec<Instr<'_>>), String> {
    let header = header(words)?;
    let mut instrs = Vec::new();
    let mut i = HEADER_WORDS;
    while i < words.len() {
        let first = words[i];
        let count = (first >> 16) as usize;
        let opcode = first & 0xffff;
        if count == 0 {
            return Err(format!("instruction at word {i} has a word count of zero"));
        }
        let end = i + count;
        if end > words.len() {
            return Err(format!("instruction at word {i} (opcode {opcode}) is truncated"));
        }
        instrs.push(Instr { opcode, operands: &words[i + 1..end] });
        i = end;
    }
    Ok((header, instrs))
}

/// Validate and decode the module header.
pub fn header(words: &[u32]) -> Result<Header, String> {
    if words.len() < HEADER_WORDS {
        return Err(format!("SPIR-V module too short: {} words (a header has {HEADER_WORDS})", words.len()));
    }
    if words[0] != MAGIC {
        if words[0] == MAGIC.swap_bytes() {
            return Err("SPIR-V module is byte-swapped (convert with `words_from_bytes`)".into());
        }
        return Err(format!("not a SPIR-V module (magic {:#010x})", words[0]));
    }
    Ok(Header { version: words[1], generator: words[2], bound: words[3] })
}

/// `OpExtInstImport` opcode.
const OP_EXT_INST_IMPORT: u32 = 11;
/// `OpExtInst` opcode.
const OP_EXT_INST: u32 = 12;
/// GLSL.std.450 `FMin`, `FMax`, `FClamp` and their NaN-tolerant counterparts `NMin`,
/// `NMax`, `NClamp`.
const NAN_TOLERANT: [(u32, u32); 3] = [(37, 79), (40, 80), (43, 81)];

/// Rewrite every GLSL.std.450 `FMin` / `FMax` / `FClamp` to `NMin` / `NMax` /
/// `NClamp`, in place, and return how many instructions changed.
///
/// GLSL leaves `min(x, NaN)` undefined and SPIR-V's `FMin` follows it, so a Vulkan
/// driver may return either operand. NVIDIA and AMD hardware returns the non-NaN
/// operand, and shader packs are tuned on those GPUs: some feed NaNs from `0/0`,
/// `normalize(vec3(0))` or `sqrt(-x)` through `clamp`/`max` and rely on them being
/// flushed. `NMin(x, NaN)` is defined as `x`, `NMax` likewise, and `NClamp(NaN, lo, hi)`
/// as `lo`, which reproduces that behaviour on every conformant driver. Every result
/// `NMin` defines is one `FMin` may return, so no defined result changes. The operand
/// types are unchanged (both forms take the same scalar or vector floating-point
/// operands), so the module stays valid.
///
/// Integer `SMin`/`UMin`/... are not touched. Malformed modules are reported as errors
/// without being modified.
pub fn nan_tolerant_min_max(spirv: &mut [u32]) -> Result<usize, String> {
    // Validate the whole module first, so that an error leaves it untouched.
    let mut glsl_sets: Vec<u32> = Vec::new();
    let mut sites: Vec<usize> = Vec::new();
    {
        let (_, instrs) = parse(spirv)?;
        let mut offset = HEADER_WORDS;
        for ins in &instrs {
            if ins.opcode == OP_EXT_INST_IMPORT
                && let Some(&id) = ins.operands.first()
                && literal_string(&ins.operands[1..]).0 == "GLSL.std.450"
            {
                glsl_sets.push(id);
            }
            // Operands: result type, result id, set, instruction, operands.
            if ins.opcode == OP_EXT_INST
                && let (Some(set), Some(inst)) = (ins.word(2), ins.word(3))
                && glsl_sets.contains(&set)
                && NAN_TOLERANT.iter().any(|(from, _)| *from == inst)
            {
                // Word index of the `instruction` operand: leading word + 3 operands.
                sites.push(offset + 4);
            }
            offset += ins.operands.len() + 1;
        }
    }
    for &i in &sites {
        if let Some((_, to)) = NAN_TOLERANT.iter().find(|(from, _)| *from == spirv[i]) {
            spirv[i] = *to;
        }
    }
    Ok(sites.len())
}

/// Decode a literal string operand (NUL-terminated, little-endian packed).
/// Returns the string and the number of words it occupies.
pub(crate) fn literal_string(words: &[u32]) -> (String, usize) {
    let mut bytes = Vec::new();
    for (i, w) in words.iter().enumerate() {
        for b in w.to_le_bytes() {
            if b == 0 {
                return (String::from_utf8_lossy(&bytes).into_owned(), i + 1);
            }
            bytes.push(b);
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), words.len())
}

/// Convert a SPIR-V binary (either byte order) to words.
pub fn words_from_bytes(bytes: &[u8]) -> Result<Vec<u32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!("SPIR-V byte length {} is not a multiple of 4", bytes.len()));
    }
    let chunks = bytes.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]);
    match bytes.get(..4) {
        Some(m) if u32::from_le_bytes([m[0], m[1], m[2], m[3]]) == MAGIC => Ok(chunks.map(u32::from_le_bytes).collect()),
        Some(m) if u32::from_be_bytes([m[0], m[1], m[2], m[3]]) == MAGIC => Ok(chunks.map(u32::from_be_bytes).collect()),
        _ => Err("not a SPIR-V binary (bad magic number)".into()),
    }
}

/// Serialize words as little-endian bytes (the on-disk `.spv` format).
pub fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Remove debug instructions: `OpSource*`, `OpString`, `OpName`, `OpMemberName`,
/// `OpModuleProcessed`, `OpLine` and `OpNoLine`.
///
/// `OpString` is kept when the module imports a `NonSemantic.*` instruction set,
/// whose instructions may reference strings.
///
/// Note that Mojang's renderpearl matches descriptors and vertex inputs by
/// *name*, so stripped modules are only usable by hosts that bind by
/// set/binding/location.
pub fn strip_debug_info(spirv: &[u32]) -> Result<Vec<u32>, String> {
    let (_, instrs) = parse(spirv)?;
    let keep_strings = instrs.iter().any(|i| {
        i.is(Op::ExtInstImport) && i.operands.len() > 1 && literal_string(&i.operands[1..]).0.starts_with("NonSemantic.")
    });
    let mut out = Vec::with_capacity(spirv.len());
    out.extend_from_slice(&spirv[..HEADER_WORDS]);
    for i in &instrs {
        let strip = [Op::SourceContinued, Op::Source, Op::SourceExtension, Op::Name, Op::MemberName, Op::ModuleProcessed, Op::Line, Op::NoLine]
            .iter()
            .any(|op| i.is(*op))
            || (i.is(Op::String) && !keep_strings);
        if !strip {
            out.push(((i.operands.len() as u32 + 1) << 16) | i.opcode);
            out.extend_from_slice(i.operands);
        }
    }
    Ok(out)
}

/// A decoration with its literal operands.
pub(crate) type Deco = (u32, Vec<u32>);

/// A type declaration, reduced to what reflection needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeDef {
    /// `OpTypeBool`.
    Bool,
    /// `OpTypeInt`.
    Int { width: u32, signed: bool },
    /// `OpTypeFloat`.
    Float { width: u32 },
    /// `OpTypeVector`.
    Vector { component: u32, count: u32 },
    /// `OpTypeMatrix`.
    Matrix { column: u32, count: u32 },
    /// `OpTypeArray`; `length` is the id of a constant.
    Array { element: u32, length: u32 },
    /// `OpTypeRuntimeArray`.
    RuntimeArray { element: u32 },
    /// `OpTypeStruct`.
    Struct { members: Vec<u32> },
    /// `OpTypeImage` (only the sampled type is kept).
    Image { sampled_type: u32 },
    /// `OpTypeSampledImage`.
    SampledImage { image: u32 },
}

/// Module-level facts gathered in one pass (what spirq does not expose).
#[derive(Debug, Default)]
pub(crate) struct ModuleInfo {
    pub capabilities: Vec<u32>,
    pub extensions: Vec<String>,
    /// `(execution model, function id, name, interface ids)`.
    pub entry_points: Vec<(u32, u32, String, Vec<u32>)>,
    /// `(function id, mode, operands, operands-are-ids)`.
    pub exec_modes: Vec<(u32, u32, Vec<u32>, bool)>,
    pub names: HashMap<u32, String>,
    /// `(struct type id, member index)` -> `OpMemberName`.
    pub member_names: HashMap<(u32, u32), String>,
    pub decorations: HashMap<u32, Vec<Deco>>,
    pub member_decorations: HashMap<(u32, u32), Vec<Deco>>,
    /// Variable id -> (pointer type id, storage class).
    pub variables: HashMap<u32, (u32, u32)>,
    /// Pointer type id -> (storage class, pointee type id).
    pub pointers: HashMap<u32, (u32, u32)>,
    /// Struct type id -> member type ids.
    pub structs: HashMap<u32, Vec<u32>>,
    /// Array type id -> element type id (sized and runtime arrays).
    pub arrays: HashMap<u32, u32>,
    /// Scalar constant id -> first literal word (`OpConstant` / `OpSpecConstant`).
    pub constants: HashMap<u32, u32>,
    /// Composite constant id -> constituent ids.
    pub composites: HashMap<u32, Vec<u32>>,
    /// Type id -> declaration (scalars, vectors, matrices, arrays, structs, images).
    pub types: HashMap<u32, TypeDef>,
    /// Opcodes spirq does not know (it would panic on them).
    pub unknown_opcodes: Vec<u32>,
}

impl ModuleInfo {
    pub fn decoration(&self, id: u32, deco: spirq::spirv::Decoration) -> Option<&[u32]> {
        self.decorations.get(&id)?.iter().find(|(d, _)| *d == deco as u32).map(|(_, ops)| ops.as_slice())
    }
    pub fn has_decoration(&self, id: u32, deco: spirq::spirv::Decoration) -> bool {
        self.decoration(id, deco).is_some()
    }
    pub fn member_decoration(&self, id: u32, member: u32, deco: spirq::spirv::Decoration) -> Option<&[u32]> {
        self.member_decorations
            .get(&(id, member))?
            .iter()
            .find(|(d, _)| *d == deco as u32)
            .map(|(_, ops)| ops.as_slice())
    }
    /// Evaluate a 32-bit integer constant id.
    pub fn const_u32(&self, id: u32) -> Option<u32> {
        self.constants.get(&id).copied()
    }

    /// The number of nodes of the largest type when expanded as a tree (every
    /// use of a struct, array or pointer type copies it, as spirq does), or
    /// `None` if some type nests deeper than `max_depth` levels.
    ///
    /// A module can declare `struct S1 { S0 a; S0 b; } ... struct S40 { S39 a; S39 b; }`
    /// in 40 instructions; expanded, that is 2^40 nodes.
    pub fn largest_type_tree(&self, max_depth: u32) -> Option<u64> {
        let mut memo: HashMap<u32, (u64, u32)> = HashMap::new();
        let mut largest = 0u64;
        for &id in self.types.keys().chain(self.pointers.keys()) {
            let (size, _) = self.type_tree(id, max_depth, &mut memo)?;
            largest = largest.max(size);
        }
        Some(largest)
    }

    /// `(tree size, depth)` of type `id`; `None` once deeper than `budget` levels.
    fn type_tree(&self, id: u32, budget: u32, memo: &mut HashMap<u32, (u64, u32)>) -> Option<(u64, u32)> {
        if let Some(&known) = memo.get(&id) {
            return (known.1 <= budget).then_some(known);
        }
        if budget == 0 {
            return None;
        }
        // Provisional entry: a (malformed) cyclic type counts as a leaf.
        memo.insert(id, (1, 1));
        let children: Vec<u32> = match (self.types.get(&id), self.pointers.get(&id)) {
            (Some(TypeDef::Struct { members }), _) => members.clone(),
            (Some(TypeDef::Array { element, .. } | TypeDef::RuntimeArray { element }), _) => vec![*element],
            (Some(TypeDef::Vector { component, .. }), _) => vec![*component],
            (Some(TypeDef::Matrix { column, .. }), _) => vec![*column],
            (Some(TypeDef::Image { sampled_type }), _) => vec![*sampled_type],
            (Some(TypeDef::SampledImage { image }), _) => vec![*image],
            (None, Some(&(_, pointee))) => vec![pointee],
            _ => Vec::new(),
        };
        let mut size = 1u64;
        let mut depth = 0u32;
        for child in children {
            let (s, d) = self.type_tree(child, budget - 1, memo)?;
            size = size.saturating_add(s);
            depth = depth.max(d);
        }
        let result = (size, depth + 1);
        memo.insert(id, result);
        Some(result)
    }
}

/// Gather [`ModuleInfo`] from a module.
pub(crate) fn scan(spirv: &[u32]) -> Result<(Header, ModuleInfo), String> {
    let (header, instrs) = parse(spirv)?;
    let mut m = ModuleInfo::default();
    for i in instrs {
        if Op::from_u32(i.opcode).is_none() {
            m.unknown_opcodes.push(i.opcode);
            continue;
        }
        let ops = i.operands;
        let op = |k| i.is(k);
        if op(Op::Capability) {
            if let Some(c) = i.word(0) {
                m.capabilities.push(c);
            }
        } else if op(Op::Extension) {
            m.extensions.push(literal_string(ops).0);
        } else if op(Op::EntryPoint) && ops.len() >= 3 {
            let (name, used) = literal_string(&ops[2..]);
            let interface = ops.get(2 + used..).unwrap_or_default().to_vec();
            m.entry_points.push((ops[0], ops[1], name, interface));
        } else if (op(Op::ExecutionMode) || op(Op::ExecutionModeId)) && ops.len() >= 2 {
            m.exec_modes.push((ops[0], ops[1], ops[2..].to_vec(), op(Op::ExecutionModeId)));
        } else if op(Op::Name) && !ops.is_empty() {
            m.names.insert(ops[0], literal_string(&ops[1..]).0);
        } else if op(Op::MemberName) && ops.len() >= 2 {
            m.member_names.insert((ops[0], ops[1]), literal_string(&ops[2..]).0);
        } else if op(Op::Decorate) && ops.len() >= 2 {
            m.decorations.entry(ops[0]).or_default().push((ops[1], ops[2..].to_vec()));
        } else if op(Op::MemberDecorate) && ops.len() >= 3 {
            m.member_decorations.entry((ops[0], ops[1])).or_default().push((ops[2], ops[3..].to_vec()));
        } else if op(Op::Variable) && ops.len() >= 3 {
            m.variables.insert(ops[1], (ops[0], ops[2]));
        } else if op(Op::TypePointer) && ops.len() >= 3 {
            m.pointers.insert(ops[0], (ops[1], ops[2]));
        } else if op(Op::TypeStruct) && !ops.is_empty() {
            m.structs.insert(ops[0], ops[1..].to_vec());
            m.types.insert(ops[0], TypeDef::Struct { members: ops[1..].to_vec() });
        } else if (op(Op::TypeArray) || op(Op::TypeRuntimeArray)) && ops.len() >= 2 {
            m.arrays.insert(ops[0], ops[1]);
            let def = match ops.get(2) {
                Some(&length) if op(Op::TypeArray) => TypeDef::Array { element: ops[1], length },
                _ => TypeDef::RuntimeArray { element: ops[1] },
            };
            m.types.insert(ops[0], def);
        } else if op(Op::TypeBool) && !ops.is_empty() {
            m.types.insert(ops[0], TypeDef::Bool);
        } else if op(Op::TypeInt) && ops.len() >= 3 {
            m.types.insert(ops[0], TypeDef::Int { width: ops[1], signed: ops[2] != 0 });
        } else if op(Op::TypeFloat) && ops.len() >= 2 {
            m.types.insert(ops[0], TypeDef::Float { width: ops[1] });
        } else if op(Op::TypeVector) && ops.len() >= 3 {
            m.types.insert(ops[0], TypeDef::Vector { component: ops[1], count: ops[2] });
        } else if op(Op::TypeMatrix) && ops.len() >= 3 {
            m.types.insert(ops[0], TypeDef::Matrix { column: ops[1], count: ops[2] });
        } else if op(Op::TypeImage) && ops.len() >= 2 {
            m.types.insert(ops[0], TypeDef::Image { sampled_type: ops[1] });
        } else if op(Op::TypeSampledImage) && ops.len() >= 2 {
            m.types.insert(ops[0], TypeDef::SampledImage { image: ops[1] });
        } else if (op(Op::Constant) || op(Op::SpecConstant)) && ops.len() >= 3 {
            m.constants.insert(ops[1], ops[2]);
        } else if (op(Op::ConstantComposite) || op(Op::SpecConstantComposite)) && ops.len() >= 2 {
            m.composites.insert(ops[1], ops[2..].to_vec());
        }
    }
    Ok((header, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `OpCapability Shader; OpMemoryModel Logical GLSL450; OpName %1 "abc"; OpSource GLSL 450`
    fn tiny() -> Vec<u32> {
        let name = u32::from_le_bytes(*b"abc\0");
        vec![
            MAGIC,
            0x0001_0500,
            0,
            10,
            0,
            (2 << 16) | Op::Capability as u32,
            1,
            (3 << 16) | Op::MemoryModel as u32,
            0,
            1,
            (3 << 16) | Op::Name as u32,
            1,
            name,
            (3 << 16) | Op::Source as u32,
            2,
            450,
        ]
    }

    #[test]
    fn header_checks() {
        assert!(header(&[]).is_err());
        assert!(header(&[1, 2, 3, 4, 5]).unwrap_err().contains("magic"));
        assert!(header(&[MAGIC.swap_bytes(), 0, 0, 0, 0]).unwrap_err().contains("byte-swapped"));
        let h = header(&tiny()).unwrap();
        assert_eq!(h.version_pair(), (1, 5));
        assert_eq!(h.bound, 10);
    }

    #[test]
    fn parse_rejects_bad_word_counts() {
        let mut m = tiny();
        m.push(0); // zero word count
        assert!(parse(&m).unwrap_err().contains("zero"));
        let mut m = tiny();
        m.push((9 << 16) | Op::Nop as u32); // claims 9 words, has 1
        assert!(parse(&m).unwrap_err().contains("truncated"));
    }

    #[test]
    fn strings() {
        let w = [u32::from_le_bytes(*b"main"), 0];
        assert_eq!(literal_string(&w), ("main".to_string(), 2));
        let w = [u32::from_le_bytes(*b"ab\0\0")];
        assert_eq!(literal_string(&w), ("ab".to_string(), 1));
        // Unterminated: consumes everything.
        let w = [u32::from_le_bytes(*b"abcd")];
        assert_eq!(literal_string(&w), ("abcd".to_string(), 1));
    }

    #[test]
    fn byte_roundtrip_both_orders() {
        let m = tiny();
        let le = words_to_bytes(&m);
        assert_eq!(words_from_bytes(&le).unwrap(), m);
        let be: Vec<u8> = m.iter().flat_map(|w| w.to_be_bytes()).collect();
        assert_eq!(words_from_bytes(&be).unwrap(), m);
        assert!(words_from_bytes(&le[..7]).is_err());
        assert!(words_from_bytes(&[0; 8]).is_err());
    }

    #[test]
    fn strip_removes_names_and_source() {
        let stripped = strip_debug_info(&tiny()).unwrap();
        let (_, instrs) = parse(&stripped).unwrap();
        assert_eq!(instrs.len(), 2);
        assert!(instrs.iter().all(|i| !i.is(Op::Name) && !i.is(Op::Source)));
    }

    /// Append one instruction.
    fn push(m: &mut Vec<u32>, op: Op, operands: &[u32]) {
        m.push(((operands.len() as u32 + 1) << 16) | op as u32);
        m.extend_from_slice(operands);
    }

    #[test]
    fn type_trees_count_shared_types_per_use() {
        // float %10; struct %11 { %10 %10 }; struct %12 { %11 %11 }; ...
        let mut m = tiny();
        push(&mut m, Op::TypeFloat, &[10, 32]);
        for i in 11..=50 {
            push(&mut m, Op::TypeStruct, &[i, i - 1, i - 1]);
        }
        let (_, info) = scan(&m).unwrap();
        // S(k) = 1 + 2 * S(k - 1): computed with memoization, not by walking 2^41 nodes.
        assert_eq!(info.largest_type_tree(64), Some((1u64 << 41) - 1));

        // Nesting depth is bounded.
        let mut m = tiny();
        push(&mut m, Op::TypeInt, &[10, 32, 0]);
        push(&mut m, Op::Constant, &[10, 11, 2]);
        for i in 12..112 {
            push(&mut m, Op::TypeArray, &[i, i - 1, 11]);
        }
        let (_, info) = scan(&m).unwrap();
        assert_eq!(info.largest_type_tree(64), None);
        assert_eq!(info.largest_type_tree(200), Some(101));

        // A (malformed) self-referencing struct does not loop.
        let mut m = tiny();
        push(&mut m, Op::TypeStruct, &[20, 20, 21]);
        push(&mut m, Op::TypeStruct, &[21, 20]);
        let (_, info) = scan(&m).unwrap();
        assert!(info.largest_type_tree(64).is_some());
    }

    #[test]
    fn scan_collects_names_and_flags_unknown_opcodes() {
        let mut m = tiny();
        m.extend([(1 << 16) | 0xfff0]);
        let (_, info) = scan(&m).unwrap();
        assert_eq!(info.names.get(&1).map(String::as_str), Some("abc"));
        assert_eq!(info.capabilities, vec![1]);
        assert_eq!(info.unknown_opcodes, vec![0xfff0]);
    }
}
