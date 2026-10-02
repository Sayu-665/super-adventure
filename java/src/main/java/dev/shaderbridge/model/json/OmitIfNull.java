package dev.shaderbridge.model.json;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Marks a record component that mirrors a Rust field with
 * {@code #[serde(default, skip_serializing_if = "Option::is_none")]}: the field is left out of the
 * JSON when it is null, exactly as serde leaves it out, instead of being written as {@code null}.
 * Applied by {@link OmitIfNullAdapterFactory}.
 */
@Retention(RetentionPolicy.RUNTIME)
@Target({ElementType.RECORD_COMPONENT, ElementType.FIELD})
public @interface OmitIfNull {
}
