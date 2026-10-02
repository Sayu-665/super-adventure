package dev.shaderbridge.model.json;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Overrides the wire tag of one variant of a tagged union. Without it,
 * {@link TaggedUnionAdapterFactory} derives the tag from the variant's simple class name with
 * {@link SerdeNames#snakeCase(String)}, exactly as serde does for the Rust variant of the same name.
 */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface Tag {
    /** @return the tag written in the {@code "type"} field */
    String value();
}
