package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.IdMaps;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** {@link MeshPlan}: when Sodium's meshes serve another plan and when they must be rebuilt. */
class MeshPlanTest {
    private static IdMaps ids(String... water) {
        return new IdMaps(Map.of(8, List.of(water)), Map.of(), Map.of(), Map.of(), Map.of());
    }

    @Test
    void compactPlansAreAlike() {
        assertTrue(MeshPlan.COMPACT.sameAs(new MeshPlan(false, ids("water"))));
        assertNull(new MeshPlan(false, ids("water")).ids(), "a compact plan carries no ids");
    }

    @Test
    void switchingBetweenCompactAndExtendedRebuilds() {
        MeshPlan pack = MeshPlan.extended(ids("water"));
        assertFalse(MeshPlan.COMPACT.sameAs(pack));
        assertFalse(pack.sameAs(MeshPlan.COMPACT));
    }

    @Test
    void equalIdMapsOfAnotherCompileServe() {
        MeshPlan first = MeshPlan.extended(ids("water", "flowing_water"));
        assertTrue(first.sameAs(first));
        assertTrue(first.sameAs(MeshPlan.extended(ids("water", "flowing_water"))), "a recompile with the same block.properties");
        assertEquals(first, MeshPlan.extended(ids("water", "flowing_water")));
    }

    @Test
    void otherBlockIdsRebuild() {
        assertFalse(MeshPlan.extended(ids("water")).sameAs(MeshPlan.extended(ids("water", "flowing_water"))));
    }

    @Test
    void anExtendedPlanNeedsIds() {
        assertThrows(NullPointerException.class, () -> new MeshPlan(true, null));
    }
}
