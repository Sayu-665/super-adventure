package dev.shaderbridge.model;

/** Dispatch size of a compute program ({@code #[serde(tag = "type")]}). */
public sealed interface WorkGroups {
    /**
     * {@code const ivec3 workGroups = ivec3(x, y, z);}
     *
     * @param x groups in x
     * @param y groups in y
     * @param z groups in z
     */
    record Absolute(int x, int y, int z) implements WorkGroups {
    }

    /**
     * {@code const vec2 workGroupsRender = vec2(x, y);}: the dispatch covers the scaled screen.
     *
     * @param x horizontal screen fraction
     * @param y vertical screen fraction
     */
    record Relative(float x, float y) implements WorkGroups {
    }
}
