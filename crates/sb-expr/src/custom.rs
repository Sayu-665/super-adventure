//! Custom uniforms and variables from `shaders.properties`
//! (`uniform.<type>.<name>=<expr>`, `variable.<type>.<name>=<expr>`).

use crate::compile::{Compiler, Resolved, Scope};
use crate::node::{Ctx, Node, SmoothState, eval};
use crate::parse::{ExprError, parse};
use crate::rng::{DEFAULT_SEED, Rng};
use crate::std140::{read_value, write_value};
use crate::value::{Value, ValueType};
use indexmap::IndexMap;
use sb_core::model::{BlockLayout, CustomUniform, UniformSource};
use sb_core::{Diagnostic, Diagnostics, GlslType};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::hash::BuildHasher;

/// Source of builtin uniform values for [`CustomUniforms::evaluate`].
///
/// Implemented for `HashMap<String, Value>`, `BTreeMap<String, Value>`,
/// `IndexMap<String, Value>`, closures `Fn(&str) -> Option<Value>` and
/// [`BlockInputs`]. A missing value reads as zero; a value of a different type than
/// the uniform's is converted (see [`Value::convert`]).
pub trait UniformInputs {
    /// The current value of the builtin uniform `name`.
    fn get(&self, name: &str) -> Option<Value>;
}

impl<S: BuildHasher> UniformInputs for HashMap<String, Value, S> {
    fn get(&self, name: &str) -> Option<Value> {
        HashMap::get(self, name).copied()
    }
}

impl UniformInputs for BTreeMap<String, Value> {
    fn get(&self, name: &str) -> Option<Value> {
        BTreeMap::get(self, name).copied()
    }
}

impl<S: BuildHasher> UniformInputs for IndexMap<String, Value, S> {
    fn get(&self, name: &str) -> Option<Value> {
        IndexMap::get(self, name).copied()
    }
}

impl<F: Fn(&str) -> Option<Value>> UniformInputs for F {
    fn get(&self, name: &str) -> Option<Value> {
        self(name)
    }
}

/// [`UniformInputs`] reading `Builtin`-sourced members of a std140 block.
///
/// Each lookup scans the layout; for per-frame use prefer
/// [`CustomUniforms::evaluate_into_block`], which caches the member offsets.
#[derive(Debug, Clone, Copy)]
pub struct BlockInputs<'a> {
    layout: &'a BlockLayout,
    bytes: &'a [u8],
}

impl<'a> BlockInputs<'a> {
    /// Read inputs from `bytes`, laid out as described by `layout`.
    pub fn new(layout: &'a BlockLayout, bytes: &'a [u8]) -> Self {
        Self { layout, bytes }
    }
}

impl UniformInputs for BlockInputs<'_> {
    fn get(&self, name: &str) -> Option<Value> {
        let m = self
            .layout
            .members
            .iter()
            .find(|m| matches!(&m.source, UniformSource::Builtin(n) if n == name))?;
        read_value(m.ty, self.bytes.get(m.offset as usize..)?).ok()
    }
}

#[derive(Debug, Clone)]
struct Def {
    name: String,
    ty: ValueType,
    is_variable: bool,
    node: Node,
}

#[derive(Debug, Clone)]
struct InputSlot {
    name: String,
    ty: ValueType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MemberRef {
    index: usize,
    offset: usize,
    ty: GlslType,
}

/// Cached member offsets for [`CustomUniforms::evaluate_into_block`].
#[derive(Debug, Clone)]
struct BlockPlan {
    block_name: String,
    members_len: usize,
    size: u32,
    /// Per input slot: the `Builtin` member it is read from.
    inputs: Vec<Option<MemberRef>>,
    /// (definition index, `Custom` member written).
    outputs: Vec<(u32, MemberRef)>,
}

impl BlockPlan {
    fn build(layout: &BlockLayout, inputs: &[InputSlot], defs: &[Def], outputs: &[u32]) -> Self {
        let mref = |index: usize| {
            let m = &layout.members[index];
            MemberRef {
                index,
                offset: m.offset as usize,
                ty: m.ty,
            }
        };
        // Member indices by source name, so building is linear in the layout size.
        let mut builtin: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut custom: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, m) in layout.members.iter().enumerate() {
            match &m.source {
                UniformSource::Builtin(n) => builtin.entry(n.as_str()).or_default().push(index),
                UniformSource::Custom(n) => custom.entry(n.as_str()).or_default().push(index),
                UniformSource::Unset => {}
            }
        }
        let inputs = inputs
            .iter()
            .map(|slot| {
                let candidates = builtin.get(slot.name.as_str())?;
                // Prefer the member whose type matches the builtin's own type (others
                // are `name__<type>` re-declarations), else the first one.
                let index = candidates
                    .iter()
                    .copied()
                    .find(|&i| ValueType::from_glsl(layout.members[i].ty) == Some(slot.ty))
                    .or_else(|| candidates.first().copied())?;
                Some(mref(index))
            })
            .collect();
        let mut outs = Vec::new();
        for &d in outputs {
            let name = defs[d as usize].name.as_str();
            for &index in custom.get(name).map_or(&[][..], Vec::as_slice) {
                outs.push((d, mref(index)));
            }
        }
        Self {
            block_name: layout.name.clone(),
            members_len: layout.members.len(),
            size: layout.size,
            inputs,
            outputs: outs,
        }
    }

    /// Cheap, allocation-free check that `layout` is still the layout this plan was
    /// built for.
    fn matches(&self, layout: &BlockLayout, inputs: &[InputSlot], defs: &[Def]) -> bool {
        let member_is = |r: &MemberRef, builtin: bool, name: &str| {
            layout.members.get(r.index).is_some_and(|m| {
                m.offset as usize == r.offset
                    && m.ty == r.ty
                    && match &m.source {
                        UniformSource::Builtin(n) => builtin && n == name,
                        UniformSource::Custom(n) => !builtin && n == name,
                        UniformSource::Unset => false,
                    }
            })
        };
        layout.members.len() == self.members_len
            && layout.size == self.size
            && layout.name == self.block_name
            && self
                .inputs
                .iter()
                .zip(inputs)
                .all(|(r, slot)| r.as_ref().is_none_or(|r| member_is(r, true, &slot.name)))
            && self.outputs.iter().all(|(d, r)| {
                defs.get(*d as usize)
                    .is_some_and(|def| member_is(r, false, &def.name))
            })
    }
}

/// A compiled set of custom uniforms and variables, evaluated once per frame.
///
/// * Definitions may reference builtin uniforms, constants, `pi` and other custom
///   uniforms/variables in any order (they are evaluated in dependency order, as in
///   Iris); cycles are errors.
/// * `uniform.*` definitions are outputs; `variable.*` definitions are only usable by
///   other expressions.
/// * A custom definition with the same name as a builtin uniform shadows it (with a
///   warning).
/// * `smooth()` keeps per-call-site state; `random()`/`randomInt()` draw from a
///   seedable deterministic generator.
///
/// ```
/// use indexmap::IndexMap;
/// use sb_core::{GlslType, model::CustomUniform};
/// use sb_expr::{CustomUniforms, Value};
///
/// let defs = vec![
///     CustomUniform { name: "half".into(), ty: GlslType::FLOAT, expression: "frameTimeCounter / 2".into(), is_variable: true, location: None },
///     CustomUniform { name: "pos".into(), ty: GlslType::VEC2, expression: "vec2(half, sunPosition.y)".into(), is_variable: false, location: None },
/// ];
/// let input_type = |name: &str| match name {
///     "frameTimeCounter" => Some(GlslType::FLOAT),
///     "sunPosition" => Some(GlslType::VEC3),
///     _ => None,
/// };
/// let (mut cu, diags) = CustomUniforms::compile(&defs, &input_type, &IndexMap::new());
/// assert!(diags.is_empty());
/// assert_eq!(cu.referenced_inputs(), vec!["frameTimeCounter", "sunPosition"]);
/// let inputs = |name: &str| match name {
///     "frameTimeCounter" => Some(Value::Float(3.0)),
///     "sunPosition" => Some(Value::Vec3([0.0, 100.0, 0.0])),
///     _ => None,
/// };
/// let out = cu.evaluate(&inputs, 1.0 / 60.0);
/// assert_eq!(out, vec![("pos".to_string(), Value::Vec2([1.5, 100.0]))]);
/// ```
#[derive(Debug, Clone)]
pub struct CustomUniforms {
    /// Kept definitions, in definition order. Variable slots index this list.
    defs: Vec<Def>,
    /// Evaluation order (indices into `defs`).
    order: Vec<u32>,
    /// `uniform.*` definitions (indices into `defs`), in definition order.
    outputs: Vec<u32>,
    inputs: Vec<InputSlot>,
    input_values: Vec<Value>,
    values: Vec<Value>,
    smooth: Vec<SmoothState>,
    rng: Rng,
    seed: u64,
    plan: Option<BlockPlan>,
}

impl Default for CustomUniforms {
    fn default() -> Self {
        Self {
            defs: Vec::new(),
            order: Vec::new(),
            outputs: Vec::new(),
            inputs: Vec::new(),
            input_values: Vec::new(),
            values: Vec::new(),
            smooth: Vec::new(),
            rng: Rng::new(DEFAULT_SEED),
            seed: DEFAULT_SEED,
            plan: None,
        }
    }
}

fn key_of(d: &CustomUniform) -> String {
    let kind = if d.is_variable { "variable" } else { "uniform" };
    format!("{kind}.{}.{}", d.ty, d.name)
}

fn expr_diag(d: &CustomUniform, e: &ExprError) -> Diagnostic {
    Diagnostic::error(
        e.kind.code(),
        format!(
            "{}: {} at column {} of `{}`; the definition is ignored",
            key_of(d),
            e.message,
            e.column(&d.expression),
            d.expression.trim()
        ),
    )
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Name resolution for one definition: custom names, then constants, then `pi`,
/// then builtin inputs.
struct DefScope<'a> {
    by_name: &'a HashMap<&'a str, usize>,
    declared: &'a [Option<ValueType>],
    constants: &'a IndexMap<String, Value>,
    input_type: &'a dyn Fn(&str) -> Option<GlslType>,
    /// Builtin inputs referenced by this definition (local slot = index).
    inputs: IndexMap<String, ValueType>,
    /// Custom definitions referenced (indices into the original list).
    deps: Vec<usize>,
}

impl Scope for DefScope<'_> {
    fn resolve(&mut self, name: &str) -> Result<Option<Resolved>, String> {
        if let Some(&i) = self.by_name.get(name)
            && let Some(ty) = self.declared.get(i).copied().flatten()
        {
            self.deps.push(i);
            return Ok(Some(Resolved::Var(i as u32, ty)));
        }
        if let Some(v) = self.constants.get(name) {
            return Ok(Some(Resolved::Const(*v)));
        }
        if name == "pi" {
            return Ok(Some(Resolved::Const(Value::Float(std::f32::consts::PI))));
        }
        let Some(gt) = (self.input_type)(name) else {
            return Ok(None);
        };
        let ty = ValueType::from_glsl(gt).ok_or_else(|| {
            format!("builtin uniform `{name}` has type {gt}, which expressions cannot use")
        })?;
        let slot = match self.inputs.get_index_of(name) {
            Some(p) => p,
            None => self.inputs.insert_full(name.to_string(), ty).0,
        };
        Ok(Some(Resolved::Input(slot as u32, ty)))
    }
}

struct Compiled {
    node: Node,
    deps: Vec<usize>,
    inputs: IndexMap<String, ValueType>,
}

/// Tarjan's strongly connected components of the graph restricted to the nodes with
/// `in_set[i]`, following the edges `succ[i]`. Returns a component id for every node
/// (`usize::MAX` outside the set). Iterative, so long dependency chains cannot
/// overflow the stack.
fn strongly_connected(roots: &[usize], succ: &[&[usize]], in_set: &[bool]) -> Vec<usize> {
    const NONE: usize = usize::MAX;
    let n = succ.len();
    let mut index = vec![NONE; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![NONE; n];
    let mut stack: Vec<usize> = Vec::new();
    // (node, position of the next successor to visit)
    let mut call: Vec<(usize, usize)> = Vec::new();
    let mut next_index = 0;
    let mut next_comp = 0;
    let inside = |w: usize| in_set.get(w).copied().unwrap_or(false);
    for &root in roots {
        if !inside(root) || index[root] != NONE {
            continue;
        }
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        call.push((root, 0));
        while let Some(top) = call.last_mut() {
            let v = top.0;
            if let Some(&w) = succ[v].get(top.1) {
                top.1 += 1;
                if !inside(w) {
                    continue;
                }
                if index[w] == NONE {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(u, _)) = call.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp[w] = next_comp;
                    if w == v {
                        break;
                    }
                }
                next_comp += 1;
            }
        }
    }
    comp
}

impl CustomUniforms {
    /// Compile custom uniform/variable definitions.
    ///
    /// * `input_type` gives the GLSL type of each builtin uniform usable as an input
    ///   (`None` = not a builtin). Dynamic per-draw uniforms (`entityId`,
    ///   `entityColor`, ...) should not be offered.
    /// * `constants` are extra named constants (e.g. `BIOME_*`, `CAT_*`, `PPT_*`;
    ///   see [`standard_constants`](crate::standard_constants)).
    ///
    /// Definitions that do not parse, are ill-typed, reference unknown identifiers,
    /// have an unsupported declared type, are part of a reference cycle or depend on
    /// a dropped definition are dropped with an error diagnostic. Duplicate names
    /// keep the first definition (a repeated identical key keeps the last one, like
    /// `java.util.Properties`).
    pub fn compile(
        defs: &[CustomUniform],
        input_type: &dyn Fn(&str) -> Option<GlslType>,
        constants: &IndexMap<String, Value>,
    ) -> (CustomUniforms, Diagnostics) {
        let mut diags = Diagnostics::new();
        let n = defs.len();

        // 1. Validate names/types and resolve duplicates.
        let mut declared: Vec<Option<ValueType>> = vec![None; n];
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        for (i, d) in defs.iter().enumerate() {
            let key = key_of(d);
            if !is_identifier(&d.name) {
                diags.push(Diagnostic::error(
                    "expr.invalid-name",
                    format!("{key}: `{}` is not a valid identifier", d.name),
                ));
                continue;
            }
            let Some(ty) = ValueType::from_declared(d.ty) else {
                diags.push(Diagnostic::error(
                    "expr.invalid-type",
                    format!("{key}: custom uniforms must be float, int, bool, vec2, vec3 or vec4; the definition is ignored"),
                ));
                continue;
            };
            match by_name.get(d.name.as_str()).copied() {
                None => {
                    by_name.insert(&d.name, i);
                    declared[i] = Some(ty);
                }
                Some(j) if defs[j].is_variable == d.is_variable && defs[j].ty == d.ty => {
                    declared[j] = None;
                    declared[i] = Some(ty);
                    by_name.insert(&d.name, i);
                    diags.push(Diagnostic::info(
                        "expr.redefined",
                        format!("{key} is defined more than once; the last definition is used"),
                    ));
                }
                Some(j) => diags.push(Diagnostic::warning(
                    "expr.duplicate",
                    format!(
                        "{key} is ignored: the name is already defined by {}",
                        key_of(&defs[j])
                    ),
                )),
            }
        }
        for (i, d) in defs.iter().enumerate() {
            if declared[i].is_some() && input_type(&d.name).is_some() {
                diags.push(Diagnostic::warning(
                    "expr.shadows-builtin",
                    format!(
                        "{} shadows the builtin uniform `{}`; the custom definition is used",
                        key_of(d),
                        d.name
                    ),
                ));
            }
        }

        // 2. Parse and type-check.
        let mut compiled: Vec<Option<Compiled>> = (0..n).map(|_| None).collect();
        let mut failed = vec![false; n];
        let mut smooth_slots = 0u32;
        for (i, d) in defs.iter().enumerate() {
            let Some(ty) = declared[i] else { continue };
            let expr = match parse(&d.expression) {
                Ok(e) => e,
                Err(e) => {
                    diags.push(expr_diag(d, &e));
                    failed[i] = true;
                    continue;
                }
            };
            let mut scope = DefScope {
                by_name: &by_name,
                declared: &declared,
                constants,
                input_type,
                inputs: IndexMap::new(),
                deps: Vec::new(),
            };
            let mut compiler = Compiler::new(&mut scope, smooth_slots);
            let result = compiler.compile_as(&expr, ty);
            smooth_slots = compiler.smooth_slots;
            match result {
                Ok(node) => {
                    let mut deps = std::mem::take(&mut scope.deps);
                    deps.sort_unstable();
                    deps.dedup();
                    compiled[i] = Some(Compiled {
                        node,
                        deps,
                        inputs: scope.inputs,
                    });
                }
                Err(e) => {
                    diags.push(expr_diag(d, &e));
                    failed[i] = true;
                }
            }
        }

        // 3. Dependency order (Kahn, lowest definition index first), propagating
        //    failures to dependents.
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indegree = vec![0usize; n];
        for (i, c) in compiled.iter().enumerate() {
            if let Some(c) = c {
                for &dep in &c.deps {
                    dependents[dep].push(i);
                }
                indegree[i] = c.deps.len();
            }
        }
        let mut ready: BinaryHeap<Reverse<usize>> = (0..n)
            .filter(|&i| declared[i].is_some() && indegree[i] == 0)
            .map(Reverse)
            .collect();
        let mut visited = vec![false; n];
        let mut order = Vec::new();
        while let Some(Reverse(i)) = ready.pop() {
            visited[i] = true;
            if failed[i] {
                for &d in &dependents[i] {
                    if !failed[d] {
                        failed[d] = true;
                        diags.push(Diagnostic::error(
                            "expr.dropped-dependency",
                            format!(
                                "{}: uses `{}`, which could not be compiled; the definition is ignored",
                                key_of(&defs[d]),
                                defs[i].name
                            ),
                        ));
                    }
                }
            } else {
                order.push(i);
            }
            for &d in &dependents[i] {
                indegree[d] -= 1;
                if indegree[d] == 0 {
                    ready.push(Reverse(d));
                }
            }
        }
        // Whatever Kahn could not order is on a reference cycle or depends on one.
        // Strongly connected components tell the two apart, so that only the members
        // of a cycle are reported as such.
        let stuck: Vec<usize> = (0..n)
            .filter(|&i| declared[i].is_some() && !visited[i])
            .collect();
        if !stuck.is_empty() {
            let succ: Vec<&[usize]> = compiled
                .iter()
                .map(|c| c.as_ref().map_or(&[][..], |c| c.deps.as_slice()))
                .collect();
            let in_set: Vec<bool> = (0..n)
                .map(|i| declared[i].is_some() && !visited[i])
                .collect();
            let comp = strongly_connected(&stuck, &succ, &in_set);
            let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
            for &i in &stuck {
                members.entry(comp[i]).or_default().push(i);
            }
            // The member list of each cycle, printed once (at most a few names).
            const LISTED: usize = 8;
            let cycle_names: HashMap<usize, String> = members
                .iter()
                .filter(|(_, m)| m.len() > 1 || m.iter().any(|&i| succ[i].contains(&i)))
                .map(|(&c, m)| {
                    let mut names = m
                        .iter()
                        .take(LISTED)
                        .map(|&j| format!("`{}`", defs[j].name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if m.len() > LISTED {
                        names.push_str(&format!(" and {} more", m.len() - LISTED));
                    }
                    (c, names)
                })
                .collect();
            for &i in &stuck {
                failed[i] = true;
                if let Some(names) = cycle_names.get(&comp[i]) {
                    diags.push(Diagnostic::error(
                        "expr.cycle",
                        format!(
                            "{}: circular reference between custom uniforms ({names}); the definition is ignored",
                            key_of(&defs[i])
                        ),
                    ));
                } else {
                    let via = succ[i]
                        .iter()
                        .find(|&&d| in_set[d])
                        .map_or("?", |&d| defs[d].name.as_str());
                    diags.push(Diagnostic::error(
                        "expr.dropped-dependency",
                        format!(
                            "{}: uses `{via}`, which is part of (or depends on) a circular reference; the definition is ignored",
                            key_of(&defs[i])
                        ),
                    ));
                }
            }
        }

        // 4. Assign final slots: definitions in definition order, inputs in order of
        //    first reference.
        let mut new_index = vec![u32::MAX; n];
        let kept: Vec<usize> = (0..n)
            .filter(|&i| declared[i].is_some() && !failed[i])
            .collect();
        for (k, &i) in kept.iter().enumerate() {
            new_index[i] = k as u32;
        }
        let mut global_inputs: IndexMap<String, ValueType> = IndexMap::new();
        let mut out_defs = Vec::with_capacity(kept.len());
        for &i in &kept {
            let (Some(c), Some(ty)) = (compiled[i].take(), declared[i]) else {
                continue;
            };
            let local: Vec<u32> = c
                .inputs
                .into_iter()
                .map(|(name, ty)| global_inputs.insert_full(name, ty).0 as u32)
                .collect();
            let mut node = c.node;
            node.remap(
                &|l| local.get(l as usize).copied().unwrap_or(u32::MAX),
                &|v| new_index.get(v as usize).copied().unwrap_or(u32::MAX),
            );
            out_defs.push(Def {
                name: defs[i].name.clone(),
                ty,
                is_variable: defs[i].is_variable,
                node,
            });
        }
        let order: Vec<u32> = order
            .iter()
            .map(|&i| new_index[i])
            .filter(|&k| k != u32::MAX)
            .collect();
        let outputs: Vec<u32> = (0..out_defs.len())
            .filter(|&k| !out_defs[k].is_variable)
            .map(|k| k as u32)
            .collect();
        let inputs: Vec<InputSlot> = global_inputs
            .into_iter()
            .map(|(name, ty)| InputSlot { name, ty })
            .collect();

        let cu = CustomUniforms {
            input_values: inputs.iter().map(|s| s.ty.zero()).collect(),
            values: out_defs.iter().map(|d| d.ty.zero()).collect(),
            smooth: vec![SmoothState::default(); smooth_slots as usize],
            defs: out_defs,
            order,
            outputs,
            inputs,
            ..CustomUniforms::default()
        };
        (cu, diags)
    }

    /// Names of the builtin uniforms read by the compiled expressions, in order of
    /// first reference. These must be present in the frame block (ARCHITECTURE §5.1).
    pub fn referenced_inputs(&self) -> Vec<String> {
        self.inputs.iter().map(|s| s.name.clone()).collect()
    }

    /// The `uniform.*` outputs (not `variable.*`), in definition order, with their
    /// declared GLSL types.
    pub fn outputs(&self) -> Vec<(String, GlslType)> {
        self.outputs
            .iter()
            .map(|&d| {
                let def = &self.defs[d as usize];
                (def.name.clone(), def.ty.glsl())
            })
            .collect()
    }

    /// Evaluate all definitions for one frame and return the outputs in the order of
    /// [`outputs`](Self::outputs). `frame_delta_seconds` drives `smooth()`.
    pub fn evaluate(
        &mut self,
        inputs: &dyn UniformInputs,
        frame_delta_seconds: f32,
    ) -> Vec<(String, Value)> {
        for (value, slot) in self.input_values.iter_mut().zip(&self.inputs) {
            *value = match inputs.get(&slot.name) {
                Some(v) => v.convert(slot.ty),
                None => slot.ty.zero(),
            };
        }
        self.run(frame_delta_seconds);
        self.outputs
            .iter()
            .map(|&d| (self.defs[d as usize].name.clone(), self.values[d as usize]))
            .collect()
    }

    /// Evaluate all definitions for one frame directly on a std140 block (normally
    /// `sb_Frame`): `Builtin`-sourced members are read as inputs and every
    /// `Custom`-sourced member whose source names a `uniform.*` output is written
    /// (converted to the member's type; booleans as `u32` 0/1).
    ///
    /// Inputs without a member read as zero; members outside `block` are skipped.
    /// The member offsets are cached and re-validated each call without
    /// allocating; after the first call with a given layout this function does not
    /// allocate. Use [`check_block`](Self::check_block) to diagnose a layout.
    ///
    /// The re-validation compares the layout's name, size and member count, and the
    /// offset, type and source of every member the cached plan reads or writes. It
    /// does not rescan the other members (that would cost about as much as the
    /// evaluation itself), so a layout edited *in place* without changing any of
    /// these (e.g. an `Unset` member turned into the `Builtin` an input was missing)
    /// keeps the old plan. Layouts are normally immutable once a pack is compiled.
    pub fn evaluate_into_block(
        &mut self,
        layout: &BlockLayout,
        block: &mut [u8],
        frame_delta_seconds: f32,
    ) {
        if !self
            .plan
            .as_ref()
            .is_some_and(|p| p.matches(layout, &self.inputs, &self.defs))
        {
            self.plan = Some(BlockPlan::build(
                layout,
                &self.inputs,
                &self.defs,
                &self.outputs,
            ));
        }
        let Some(plan) = self.plan.take() else { return };
        for ((value, slot), member) in self
            .input_values
            .iter_mut()
            .zip(&self.inputs)
            .zip(&plan.inputs)
        {
            *value = member
                .as_ref()
                .and_then(|m| read_value(m.ty, block.get(m.offset..)?).ok())
                .map_or(slot.ty.zero(), |v| v.convert(slot.ty));
        }
        self.run(frame_delta_seconds);
        for (d, m) in &plan.outputs {
            if let (Some(v), Some(bytes)) =
                (self.values.get(*d as usize), block.get_mut(m.offset..))
            {
                // Errors (member does not fit / unsupported type) are reported by
                // `check_block`; the member is left untouched.
                let _ = write_value(m.ty, *v, bytes);
            }
        }
        self.plan = Some(plan);
    }

    /// Check how [`evaluate_into_block`](Self::evaluate_into_block) would use
    /// `layout`: inputs without a `Builtin` member, outputs without a `Custom`
    /// member, `Custom` members naming no output, and members that do not fit.
    pub fn check_block(&self, layout: &BlockLayout) -> Diagnostics {
        let mut diags = Diagnostics::new();
        let plan = BlockPlan::build(layout, &self.inputs, &self.defs, &self.outputs);
        let fits = |m: &MemberRef| {
            m.ty.array.is_none()
                && m.offset
                    .checked_add(m.ty.std140_size() as usize)
                    .is_some_and(|end| end <= layout.size as usize)
        };
        for (slot, m) in self.inputs.iter().zip(&plan.inputs) {
            match m {
                None => diags.push(Diagnostic::warning(
                    "expr.block-missing-input",
                    format!(
                        "builtin `{}` is used by custom uniforms but has no member in `{}`; it reads as zero",
                        slot.name, layout.name
                    ),
                )),
                Some(m) if !fits(m) => diags.push(Diagnostic::warning(
                    "expr.block-member",
                    format!("member `{}` of `{}` cannot be read as {}", layout.members[m.index].name, layout.name, m.ty),
                )),
                Some(_) => {}
            }
        }
        let written: std::collections::HashSet<u32> =
            plan.outputs.iter().map(|(d, _)| *d).collect();
        for &d in &self.outputs {
            if !written.contains(&d) {
                diags.push(Diagnostic::info(
                    "expr.block-unused-output",
                    format!(
                        "custom uniform `{}` has no member in `{}`",
                        self.defs[d as usize].name, layout.name
                    ),
                ));
            }
        }
        for (_, m) in &plan.outputs {
            if !fits(m) {
                diags.push(Diagnostic::warning(
                    "expr.block-member",
                    format!(
                        "member `{}` of `{}` cannot be written as {}",
                        layout.members[m.index].name, layout.name, m.ty
                    ),
                ));
            }
        }
        let output_names: std::collections::HashSet<&str> = self
            .outputs
            .iter()
            .map(|&d| self.defs[d as usize].name.as_str())
            .collect();
        for m in &layout.members {
            if let UniformSource::Custom(name) = &m.source
                && !output_names.contains(name.as_str())
            {
                diags.push(Diagnostic::warning(
                    "expr.block-unknown-custom",
                    format!(
                        "member `{}` of `{}` expects custom uniform `{name}`, which is not defined (or was dropped); it is left unchanged",
                        m.name, layout.name
                    ),
                ));
            }
        }
        diags
    }

    /// Re-seed the generator behind `random()` and `randomInt()`.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
        self.rng = Rng::new(seed);
    }

    /// Forget all per-frame state: smoothing history (the next evaluation snaps to
    /// the targets), last values, and the random generator (re-seeded with the last
    /// seed). Call this when the world or the pack is reloaded.
    pub fn reset(&mut self) {
        self.smooth
            .iter_mut()
            .for_each(|s| *s = SmoothState::default());
        for (v, d) in self.values.iter_mut().zip(&self.defs) {
            *v = d.ty.zero();
        }
        self.rng = Rng::new(self.seed);
    }

    /// The value computed by the last evaluation for the uniform or variable `name`
    /// (zero before the first evaluation).
    pub fn value(&self, name: &str) -> Option<Value> {
        let k = self.defs.iter().position(|d| d.name == name)?;
        self.values.get(k).copied()
    }

    /// Number of compiled definitions (uniforms and variables).
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// `true` when no definition survived compilation.
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    fn run(&mut self, frame_delta_seconds: f32) {
        let dt = if frame_delta_seconds.is_finite() && frame_delta_seconds > 0.0 {
            frame_delta_seconds
        } else {
            0.0
        };
        for &k in &self.order {
            let k = k as usize;
            let Some(def) = self.defs.get(k) else {
                continue;
            };
            let v = {
                let mut cx = Ctx {
                    inputs: &self.input_values,
                    vars: &self.values,
                    smooth: &mut self.smooth,
                    rng: &mut self.rng,
                    dt,
                };
                eval(&def.node, &mut cx)
            };
            if let Some(slot) = self.values.get_mut(k) {
                *slot = v;
            }
        }
    }
}
