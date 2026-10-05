package dev.shaderbridge.compat.sodium;

import dev.shaderbridge.model.IdMaps;
import java.util.Objects;

/**
 * How Sodium should mesh terrain: in its own compact vertex, or in the extended vertex
 * ({@link TerrainVertexLayout}) with the block ids of a pack's {@code block.properties}. Changing
 * the plan means meshing every section again (Sodium's renderer is reloaded), so plans compare by
 * value; {@link #sameAs} compares the id maps by identity first, which keeps the per-frame check
 * cheap.
 *
 * @param extended mesh the extended vertex
 * @param ids      the block id maps the extension carries (null when not extended)
 */
public record MeshPlan(boolean extended, IdMaps ids) {
    /** Sodium's own vertex. */
    public static final MeshPlan COMPACT = new MeshPlan(false, null);

    public MeshPlan {
        if (extended) {
            Objects.requireNonNull(ids, "ids");
        } else {
            ids = null;
        }
    }

    /**
     * @param ids the active pack's id maps
     * @return the plan of a pack
     */
    public static MeshPlan extended(IdMaps ids) {
        return new MeshPlan(true, ids);
    }

    /**
     * @param other another plan
     * @return whether meshes built for one serve the other
     */
    public boolean sameAs(MeshPlan other) {
        return extended == other.extended && (ids == other.ids || Objects.equals(ids, other.ids));
    }
}
