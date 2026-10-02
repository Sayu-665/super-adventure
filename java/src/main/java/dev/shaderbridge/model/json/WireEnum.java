package dev.shaderbridge.model.json;

/**
 * An enum serialized as a plain JSON string (a serde unit-variant enum). The default wire name is
 * the lower-cased constant name, which matches serde's {@code snake_case} and {@code lowercase}
 * renames when the Java constants are spelled in upper snake case.
 */
public interface WireEnum {
    /** @return the JSON string of this constant */
    default String wireName() {
        return SerdeNames.lowerCase(((Enum<?>) this).name());
    }
}
