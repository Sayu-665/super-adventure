package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Composite-style pass groups and geometry pass slots, in frame execution order. */
public enum PassGroup implements WireEnum {
    /** {@code setup*}: once after load. */
    SETUP,
    /** {@code begin*}: start of every frame. */
    BEGIN,
    /** Shadow geometry pass. */
    SHADOW,
    /** {@code shadowcomp*}. */
    SHADOW_COMP,
    /** {@code prepare*}. */
    PREPARE,
    /** Opaque gbuffers geometry. */
    GBUFFERS_OPAQUE,
    /** {@code deferred*}. */
    DEFERRED,
    /** Translucent gbuffers geometry. */
    GBUFFERS_TRANSLUCENT,
    /** {@code composite*}. */
    COMPOSITE,
    /** {@code final}. */
    FINAL
}
