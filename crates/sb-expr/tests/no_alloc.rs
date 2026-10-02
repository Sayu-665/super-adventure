//! `CustomUniforms::evaluate_into_block` must not allocate once its member plan is
//! built. A counting global allocator (test-only; this is the one place in the crate
//! that needs `unsafe`) tracks allocations made by the current thread.

use indexmap::IndexMap;
use sb_core::GlslType;
use sb_core::model::{BlockLayout, BlockMember, CustomUniform, UniformSource};
use sb_expr::{CustomUniforms, Value, read_value, standard_constants, write_value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn note_allocation() {
    // `try_with` cannot fail for const-initialized thread locals, except during
    // thread teardown, where nothing is tracked anyway.
    let _ = TRACKING.try_with(|t| {
        if t.get() {
            let _ = ALLOCATIONS.try_with(|a| a.set(a.get() + 1));
        }
    });
}

// SAFETY: every method forwards to the system allocator with the caller's
// arguments unchanged; the bookkeeping only touches const-initialized thread
// locals and never allocates.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: forwarded verbatim; the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded verbatim; `ptr` was allocated by `System` with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: forwarded verbatim.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation();
        // SAFETY: forwarded verbatim; `ptr`/`layout` come from this allocator (= System).
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn count_allocations(f: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|a| a.set(0));
    TRACKING.with(|t| t.set(true));
    f();
    TRACKING.with(|t| t.set(false));
    ALLOCATIONS.with(|a| a.get())
}

fn def(name: &str, ty: GlslType, expr: &str, is_variable: bool) -> CustomUniform {
    CustomUniform {
        name: name.into(),
        ty,
        expression: expr.into(),
        is_variable,
        location: None,
    }
}

fn member(name: &str, ty: GlslType, offset: u32, source: UniformSource) -> BlockMember {
    BlockMember {
        name: name.into(),
        ty,
        offset,
        source,
        default: None,
    }
}

#[test]
fn evaluate_into_block_does_not_allocate_per_frame() {
    let defs = vec![
        def(
            "difX",
            GlslType::FLOAT,
            "cameraPosition.x - previousCameraPosition.x",
            true,
        ),
        def(
            "moving",
            GlslType::FLOAT,
            "if(abs(difX) > 0.0 && abs(difX) < 1.0, 1, 0)",
            true,
        ),
        def(
            "starter",
            GlslType::FLOAT,
            "smooth(3, moving, 20, 20)",
            false,
        ),
        def(
            "isEyeInCave",
            GlslType::FLOAT,
            "if(isEyeInWater == 0, 1.0 - smooth(202, if(eyeAltitude < 5.0, eyeBrightness.y / 240.0, 1.0), 6, 12), 0.0)",
            false,
        ),
        def(
            "inRainy",
            GlslType::FLOAT,
            "smooth(102, if(in(biome_precipitation, PPT_RAIN), 1, 0), 20, 10)",
            false,
        ),
        def(
            "sunDir",
            GlslType::VEC3,
            "vec3(gbufferModelViewInverse.0.0 * sunPosition.x, gbufferModelViewInverse.1.1 * sunPosition.y, max(sunPosition.z, 0, -1))",
            false,
        ),
        def("framemod8", GlslType::INT, "frameCounter % 8", false),
        def(
            "noise",
            GlslType::FLOAT,
            "random(0, 1) + randomInt(0, 4)",
            false,
        ),
        def(
            "isDay",
            GlslType::BOOL,
            "sunPosition.y > 0 || between(worldTime, 0, 12000)",
            false,
        ),
        def(
            "taa",
            GlslType::VEC2,
            "vec2(frac(1.3247 * frameCounter + 0.5), 0.5) * 2 - 1",
            false,
        ),
    ];
    let input_type = |name: &str| {
        Some(match name {
            "cameraPosition" | "previousCameraPosition" | "sunPosition" => GlslType::VEC3,
            "eyeBrightness" => GlslType::IVEC2,
            "gbufferModelViewInverse" => GlslType::MAT4,
            "isEyeInWater" | "biome_precipitation" | "frameCounter" | "worldTime" => GlslType::INT,
            "eyeAltitude" => GlslType::FLOAT,
            _ => return None,
        })
    };
    let constants: IndexMap<String, Value> = standard_constants();
    let (mut cu, diags) = CustomUniforms::compile(&defs, &input_type, &constants);
    assert!(diags.is_empty(), "{diags:?}");

    let mut members = Vec::new();
    let mut offset = 0u32;
    for name in cu.referenced_inputs() {
        let ty = input_type(&name).unwrap();
        offset = offset.next_multiple_of(ty.std140_align());
        members.push(member(
            &name,
            ty,
            offset,
            UniformSource::Builtin(name.clone()),
        ));
        offset += ty.std140_size();
    }
    for (name, ty) in cu.outputs() {
        offset = offset.next_multiple_of(ty.std140_align());
        members.push(member(
            &name,
            ty,
            offset,
            UniformSource::Custom(name.clone()),
        ));
        offset += ty.std140_size();
    }
    let layout = BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: offset.next_multiple_of(16),
        members,
    };
    let mut block = vec![0u8; layout.size as usize];
    let set = |block: &mut [u8], name: &str, v: Value| {
        let m = layout.member(name).unwrap();
        write_value(m.ty, v, &mut block[m.offset as usize..]).unwrap();
    };
    set(
        &mut block,
        "cameraPosition",
        Value::Vec3([10.5, 64.0, -3.0]),
    );
    set(
        &mut block,
        "previousCameraPosition",
        Value::Vec3([10.0, 64.0, -3.0]),
    );
    set(&mut block, "sunPosition", Value::Vec3([0.0, 100.0, 20.0]));
    set(&mut block, "eyeBrightness", Value::Vec2([0.0, 240.0]));
    set(
        &mut block,
        "gbufferModelViewInverse",
        Value::Mat4(std::array::from_fn(|i| if i % 5 == 0 { 2.0 } else { 0.0 })),
    );
    set(&mut block, "frameCounter", Value::Int(41));
    set(&mut block, "worldTime", Value::Int(1000));

    // First call builds the member plan (allowed to allocate).
    cu.evaluate_into_block(&layout, &mut block, 1.0 / 60.0);
    let allocations = count_allocations(|| {
        for frame in 0..500 {
            set(&mut block, "frameCounter", Value::Int(42 + frame));
            cu.evaluate_into_block(&layout, &mut block, 1.0 / 60.0);
        }
    });
    assert_eq!(allocations, 0, "evaluate_into_block allocated");

    let read = |name: &str| {
        let m = layout.member(name).unwrap();
        read_value(m.ty, &block[m.offset as usize..]).unwrap()
    };
    assert_eq!(read("framemod8"), Value::Int((42 + 499) % 8));
    assert_eq!(read("sunDir"), Value::Vec3([0.0, 200.0, 20.0]));
    assert_eq!(read("isDay"), Value::Bool(true));
    assert_eq!(read("isEyeInCave"), Value::Float(0.0));
    let Value::Float(starter) = read("starter") else {
        panic!()
    };
    assert!(starter > 0.9 && starter <= 1.0, "{starter}");

    // A changed layout is detected and handled (rebuilding the plan may allocate).
    let mut shifted = layout.clone();
    for m in &mut shifted.members {
        m.offset += 16;
    }
    shifted.size += 16;
    let mut block2 = vec![0u8; shifted.size as usize];
    cu.evaluate_into_block(&shifted, &mut block2, 1.0 / 60.0);
    let allocations =
        count_allocations(|| cu.evaluate_into_block(&shifted, &mut block2, 1.0 / 60.0));
    assert_eq!(allocations, 0);
}
