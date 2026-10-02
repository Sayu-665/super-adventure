package dev.shaderbridge.pack;

import dev.shaderbridge.model.json.WireEnum;

/** Storage of a shader pack. */
public enum PackKind implements WireEnum {
    /** A directory containing {@code shaders/}. */
    DIR,
    /** A zip file containing {@code shaders/}. */
    ZIP
}
