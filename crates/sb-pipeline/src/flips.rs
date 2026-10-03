//! The static pass list and main/alt ("flip") schedule of a frame, mirroring Iris's
//! `CompositeRenderer` / `FinalPassRenderer` construction:
//!
//! * one `BufferFlipper` is shared by `begin`, `prepare`, `deferred`, `composite` and
//!   `final` (in that order); `shadowcomp` flips shadowcolor buffers, which hosts track
//!   themselves (the model has no shadowcolor schedule);
//! * before a group, its virtual `<group>_pre` program applies its `flip.<group>_pre.<buf>=true`
//!   keys (`false` has no effect);
//! * a composite-style pass reads the current state and writes the other image of every
//!   draw buffer; afterwards each draw buffer is flipped unless `flip.<prog>.<buf>=false`,
//!   and then every `flip.<prog>.<buf>=true` flips its buffer once more (Iris applies the
//!   explicit `true` flips after the draw-buffer loop, so `true` on a written buffer flips
//!   it twice, i.e. not at all; on an unwritten buffer it forces a flip);
//! * passes with computes only, `setup`, the geometry passes and `final` do not flip;
//! * at the end of the frame every buffer that is flipped (an odd number of flips) and
//!   not cleared every frame is copied alt → main.

use indexmap::IndexMap;
use sb_core::PassGroup;
use sb_core::model::Pass;
use std::collections::{BTreeMap, BTreeSet};

/// Pass groups in execution order.
pub const GROUP_ORDER: [PassGroup; 10] = [
    PassGroup::Setup,
    PassGroup::Begin,
    PassGroup::Shadow,
    PassGroup::ShadowComp,
    PassGroup::Prepare,
    PassGroup::GbuffersOpaque,
    PassGroup::Deferred,
    PassGroup::GbuffersTranslucent,
    PassGroup::Composite,
    PassGroup::Final,
];

/// Composite-style groups whose programs flip colortex buffers.
pub fn flips_buffers(group: PassGroup) -> bool {
    matches!(group, PassGroup::Begin | PassGroup::Prepare | PassGroup::Deferred | PassGroup::Composite)
}

/// One pass to schedule.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PassInput {
    /// Index within the group (`composite3` → 3).
    pub index: u8,
    /// Fullscreen program (model index), if any.
    pub program: Option<u32>,
    /// Compute programs (model indices) dispatched before the program.
    pub computes: Vec<u32>,
    /// The fullscreen program's draw buffers.
    pub draw_buffers: Vec<u32>,
    /// `flip.<prog>.<buf>` of the fullscreen program.
    pub explicit_flips: IndexMap<u32, bool>,
}

/// Input of [`schedule`].
#[derive(Debug, Clone, Default)]
pub struct ScheduleInput {
    /// Composite-style passes (and setup / final) per group, in index order. Geometry
    /// groups get a pass automatically; their computes (e.g. `shadow.csh`) go in
    /// [`ScheduleInput::geometry_computes`].
    pub passes: BTreeMap<PassGroup, Vec<PassInput>>,
    /// Computes dispatched at the start of a geometry pass (`shadow.csh`, `shadow_a.csh`).
    pub geometry_computes: BTreeMap<PassGroup, Vec<u32>>,
    /// `flip.<group>_pre.<buf>` keys.
    pub pre_flips: BTreeMap<PassGroup, IndexMap<u32, bool>>,
    /// colortex buffers cleared every frame.
    pub cleared: BTreeSet<u32>,
    /// Minimum length of [`Pass::flip_state`] (number of colortex buffers in use).
    pub colortex_count: u32,
}

/// Output of [`schedule`].
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Passes in execution order, with `flip_state` and `flips_after`.
    pub passes: Vec<Pass>,
    /// Buffers flipped an odd number of times and not cleared: copied alt → main.
    pub end_of_frame_copies: Vec<u32>,
    /// Flip state when each group starts (after its `_pre` flips).
    pub group_state: BTreeMap<PassGroup, BTreeSet<u32>>,
    /// Buffers flipped at least once by a program before each pass starts (Iris
    /// `flippedAtLeastOnce`, which decides whether `texture.<stage>.colortexN` overrides
    /// still apply); same order as [`Schedule::passes`].
    pub flipped_at_least_once: Vec<BTreeSet<u32>>,
}

impl Schedule {
    /// Flip state at the start of the pass running program `program` (or `None`).
    pub fn state_of_program(&self, program: u32) -> Option<BTreeSet<u32>> {
        self.passes.iter().find(|p| p.program == Some(program) || p.computes.contains(&program)).map(|p| {
            p.flip_state.iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i as u32).collect()
        })
    }
}

fn toggle(set: &mut BTreeSet<u32>, b: u32) {
    if !set.remove(&b) {
        set.insert(b);
    }
}

/// Build the pass list and flip schedule.
pub fn schedule(input: &ScheduleInput) -> Schedule {
    let mut count = input.colortex_count.max(1);
    let mut bump = |b: u32| count = count.max(b.saturating_add(1));
    for (group, passes) in &input.passes {
        // Only colortex-flipping groups name colortex buffers (shadowcomp writes shadowcolor).
        if !flips_buffers(*group) {
            continue;
        }
        for p in passes {
            p.draw_buffers.iter().copied().for_each(&mut bump);
            p.explicit_flips.keys().copied().for_each(&mut bump);
        }
    }
    for f in input.pre_flips.values() {
        f.keys().copied().for_each(&mut bump);
    }
    let count = count.min(sb_uniforms::MAX_COLOR_TEX);
    let state_vec = |s: &BTreeSet<u32>| -> Vec<bool> { (0..count).map(|i| s.contains(&i)).collect() };

    let mut flipped: BTreeSet<u32> = BTreeSet::new();
    let mut at_least_once: BTreeSet<u32> = BTreeSet::new();
    let mut out = Schedule {
        passes: Vec::new(),
        end_of_frame_copies: Vec::new(),
        group_state: BTreeMap::new(),
        flipped_at_least_once: Vec::new(),
    };
    for group in GROUP_ORDER {
        if group.pre_flip_name().is_some()
            && let Some(pre) = input.pre_flips.get(&group)
        {
            // NB (Iris): `_pre` flips do not count as "flipped at least once".
            for (b, flip) in pre {
                if *flip {
                    toggle(&mut flipped, *b);
                }
            }
        }
        out.group_state.insert(group, flipped.clone());
        if matches!(group, PassGroup::Shadow | PassGroup::GbuffersOpaque | PassGroup::GbuffersTranslucent) {
            out.passes.push(Pass {
                group,
                index: 0,
                computes: input.geometry_computes.get(&group).cloned().unwrap_or_default(),
                program: None,
                flips_after: Vec::new(),
                flip_state: state_vec(&flipped),
            });
            out.flipped_at_least_once.push(at_least_once.clone());
            continue;
        }
        let Some(passes) = input.passes.get(&group) else { continue };
        let mut passes: Vec<&PassInput> = passes.iter().collect();
        passes.sort_by_key(|p| p.index);
        for p in passes {
            if p.program.is_none() && p.computes.is_empty() {
                continue;
            }
            let state = state_vec(&flipped);
            out.flipped_at_least_once.push(at_least_once.clone());
            let mut flips_after = Vec::new();
            if flips_buffers(group) && p.program.is_some() {
                let mut toggled: BTreeSet<u32> = BTreeSet::new();
                for &b in &p.draw_buffers {
                    if p.explicit_flips.get(&b) == Some(&false) {
                        continue;
                    }
                    toggle(&mut flipped, b);
                    toggle(&mut toggled, b);
                    at_least_once.insert(b);
                }
                for (&b, &flip) in &p.explicit_flips {
                    if flip {
                        toggle(&mut flipped, b);
                        toggle(&mut toggled, b);
                        at_least_once.insert(b);
                    }
                }
                flips_after = toggled.into_iter().collect();
            }
            out.passes.push(Pass {
                group,
                index: p.index,
                computes: p.computes.clone(),
                program: p.program,
                flips_after,
                flip_state: state,
            });
        }
    }
    out.end_of_frame_copies = flipped.difference(&input.cleared).copied().collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pass(index: u8, program: u32, draw: &[u32]) -> PassInput {
        PassInput { index, program: Some(program), draw_buffers: draw.to_vec(), ..Default::default() }
    }

    fn states(s: &Schedule) -> Vec<(PassGroup, u8, Vec<u32>, Vec<bool>)> {
        s.passes.iter().map(|p| (p.group, p.index, p.flips_after.clone(), p.flip_state.clone())).collect()
    }

    #[test]
    fn composite_ping_pong() {
        // deferred (writes 0,1), composite (writes 0), composite1 (writes 0, 2), final.
        let mut input = ScheduleInput { colortex_count: 3, cleared: [0, 2].into(), ..Default::default() };
        input.passes.insert(PassGroup::Deferred, vec![pass(0, 1, &[0, 1])]);
        input.passes.insert(PassGroup::Composite, vec![pass(1, 3, &[0, 2]), pass(0, 2, &[0])]);
        input.passes.insert(PassGroup::Final, vec![pass(0, 4, &[0])]);
        let s = schedule(&input);
        use PassGroup as G;
        let f = false;
        let t = true;
        assert_eq!(
            states(&s),
            vec![
                (G::Shadow, 0, vec![], vec![f, f, f]),
                (G::GbuffersOpaque, 0, vec![], vec![f, f, f]),
                (G::Deferred, 0, vec![0, 1], vec![f, f, f]),
                (G::GbuffersTranslucent, 0, vec![], vec![t, t, f]),
                (G::Composite, 0, vec![0], vec![t, t, f]),
                (G::Composite, 1, vec![0, 2], vec![f, t, f]),
                (G::Final, 0, vec![], vec![t, t, t]),
            ]
        );
        // colortex1 flipped once and not cleared → copied; 0 and 2 are cleared.
        assert_eq!(s.end_of_frame_copies, vec![1]);
        assert_eq!(s.group_state[&G::GbuffersTranslucent], [0, 1].into());
        assert_eq!(s.state_of_program(3), Some([1].into()));
    }

    #[test]
    fn explicit_flips_and_pre() {
        let mut input = ScheduleInput { colortex_count: 4, ..Default::default() };
        let mut c0 = pass(0, 10, &[0, 1]);
        c0.explicit_flips.insert(1, false); // written but not flipped
        c0.explicit_flips.insert(3, true); // not written, forced flip
        let mut c1 = pass(1, 11, &[2]);
        c1.explicit_flips.insert(2, true); // written AND true: flipped twice (Iris) = no flip
        input.passes.insert(PassGroup::Composite, vec![c0, c1]);
        input.pre_flips.insert(PassGroup::Composite, [(2, true), (0, false)].into_iter().collect());
        let s = schedule(&input);
        let comp: Vec<_> = s.passes.iter().filter(|p| p.group == PassGroup::Composite).collect();
        // composite_pre flipped colortex2 before composite.
        assert_eq!(comp[0].flip_state, vec![false, false, true, false]);
        assert_eq!(comp[0].flips_after, vec![0, 3]);
        assert_eq!(comp[1].flip_state, vec![true, false, true, true]);
        assert_eq!(comp[1].flips_after, Vec::<u32>::new());
        // colortex0 and colortex3 flipped once (pre flip of 2 + nothing net from composite1).
        assert_eq!(s.end_of_frame_copies, vec![0, 2, 3]);
        // `_pre` flips do not count as flipped at least once; program flips do.
        assert_eq!(s.flipped_at_least_once[s.passes.iter().position(|p| p.program == Some(11)).unwrap()], [0, 3].into());
    }

    #[test]
    fn computes_do_not_flip_and_order_is_iris() {
        let mut input = ScheduleInput { colortex_count: 1, ..Default::default() };
        input.passes.insert(PassGroup::Setup, vec![PassInput { index: 0, computes: vec![0], ..Default::default() }]);
        input.passes.insert(
            PassGroup::Begin,
            vec![PassInput { index: 0, computes: vec![1], draw_buffers: vec![0], ..Default::default() }],
        );
        input.passes.insert(PassGroup::ShadowComp, vec![pass(0, 2, &[0, 1])]);
        input.passes.insert(PassGroup::Prepare, vec![pass(0, 3, &[0])]);
        input.geometry_computes.insert(PassGroup::Shadow, vec![7]);
        let s = schedule(&input);
        let order: Vec<PassGroup> = s.passes.iter().map(|p| p.group).collect();
        use PassGroup as G;
        assert_eq!(order, vec![G::Setup, G::Begin, G::Shadow, G::ShadowComp, G::Prepare, G::GbuffersOpaque, G::GbuffersTranslucent]);
        // Compute-only begin pass and shadowcomp do not flip colortex buffers.
        assert!(s.passes[1].flips_after.is_empty());
        assert!(s.passes[3].flips_after.is_empty());
        assert_eq!(s.passes[2].computes, vec![7]);
        assert_eq!(s.passes[4].flips_after, vec![0]);
        assert_eq!(s.passes[5].flip_state, vec![true]);
        assert_eq!(s.end_of_frame_copies, vec![0]);
    }

    #[test]
    fn begin_pre_and_empty_passes() {
        let mut input = ScheduleInput::default();
        input.pre_flips.insert(PassGroup::Begin, [(5, true)].into_iter().collect());
        input.passes.insert(PassGroup::Begin, vec![PassInput { index: 3, ..Default::default() }]);
        let s = schedule(&input);
        // Empty passes are dropped; the pre flip still applies and widens flip_state.
        assert!(s.passes.iter().all(|p| p.group != PassGroup::Begin));
        assert_eq!(s.passes[0].flip_state.len(), 6);
        assert!(s.passes[0].flip_state[5]);
        assert_eq!(s.end_of_frame_copies, vec![5]);
    }
}
