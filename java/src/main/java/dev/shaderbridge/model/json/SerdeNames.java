package dev.shaderbridge.model.json;

import java.util.Locale;

/** Name conversions that reproduce the serde attributes used by {@code sb_core::model}. */
public final class SerdeNames {
    private SerdeNames() {
    }

    /**
     * Converts a Rust variant identifier with serde's {@code rename_all = "snake_case"} rule: an
     * underscore is inserted before every upper-case character except the first, and everything is
     * lower-cased. {@code ColorTex} becomes {@code color_tex} and {@code Absolute1D} becomes
     * {@code absolute1_d}.
     *
     * @param identifier a Rust (or Java) CamelCase identifier
     * @return the serde snake_case form
     */
    public static String snakeCase(String identifier) {
        StringBuilder out = new StringBuilder(identifier.length() + 4);
        for (int i = 0; i < identifier.length(); i++) {
            char c = identifier.charAt(i);
            if (i > 0 && Character.isUpperCase(c)) {
                out.append('_');
            }
            out.append(Character.toLowerCase(c));
        }
        return out.toString();
    }

    /**
     * Converts an upper-snake Java enum constant ({@code SKY_BASIC}) to the snake_case wire name
     * ({@code sky_basic}) serde produces for the matching Rust variant ({@code SkyBasic}).
     *
     * @param constantName a Java enum constant name
     * @return the lower-cased name
     */
    public static String lowerCase(String constantName) {
        return constantName.toLowerCase(Locale.ROOT);
    }
}
