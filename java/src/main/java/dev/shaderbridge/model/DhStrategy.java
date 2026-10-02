package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** How Distant Horizons LODs are shaded (ARCHITECTURE §9). */
public enum DhStrategy implements WireEnum {
    /** The pack ships {@code dh_*} programs. */
    NATIVE,
    /** {@code dh_*} programs are synthesized from {@code gbuffers_*} programs. */
    SYNTHESIZED,
    /** Distant Horizons is not present in the compile environment. */
    DISABLED
}
