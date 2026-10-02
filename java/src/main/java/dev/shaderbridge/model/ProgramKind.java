package dev.shaderbridge.model;

/** What a program is ({@code #[serde(tag = "type")]}). */
public sealed interface ProgramKind {
    /**
     * A geometry program.
     *
     * @param program the geometry program
     */
    record Geometry(GeometryProgram program) implements ProgramKind {
    }

    /**
     * A fullscreen pass.
     *
     * @param group pass group
     * @param index index within the group ({@code composite3} is 3)
     */
    record Composite(PassGroup group, int index) implements ProgramKind {
    }

    /**
     * A compute shader attached to a pass.
     *
     * @param group  pass group
     * @param index  index within the group
     * @param letter compute letter ({@code composite3_b} is {@code b}), or null for {@code composite3.csh}
     */
    record Compute(PassGroup group, int index, Character letter) implements ProgramKind {
    }

    /**
     * A compute shader of a geometry pass ({@code shadow.csh}, {@code shadow_a.csh}).
     *
     * @param program the geometry program
     * @param letter  compute letter, or null
     */
    record GeometryCompute(GeometryProgram program, Character letter) implements ProgramKind {
    }
}
