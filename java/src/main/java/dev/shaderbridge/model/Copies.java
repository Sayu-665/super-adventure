package dev.shaderbridge.model;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Defensive, order-preserving, unmodifiable copies for record components. A missing collection
 * (serde {@code #[serde(default)]}) becomes empty; nulls inside are kept because several Rust
 * collections hold {@code Option} values.
 */
final class Copies {
    private Copies() {
    }

    static <T> List<T> list(List<T> list) {
        return list == null ? List.of() : Collections.unmodifiableList(new ArrayList<>(list));
    }

    static <K, V> Map<K, V> map(Map<K, V> map) {
        return map == null ? Map.of() : Collections.unmodifiableMap(new LinkedHashMap<>(map));
    }

    static <T> T required(T value, String field) {
        return Objects.requireNonNull(value, () -> "missing field '" + field + "'");
    }
}
