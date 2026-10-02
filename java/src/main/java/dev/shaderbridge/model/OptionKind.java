package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** How a pack option is declared in the shader sources. */
public enum OptionKind implements WireEnum {
    /** {@code #define NAME}, toggled by commenting it out. */
    BOOLEAN_DEFINE,
    /** {@code #define NAME VALUE // [a b c]}. */
    VALUE_DEFINE,
    /** {@code const <type> NAME = VALUE; // [a b c]} or a whitelisted {@code const bool}. */
    CONST;

    /** @return true for options shown as ON/OFF toggles */
    public boolean isBoolean() {
        return this == BOOLEAN_DEFINE;
    }
}
