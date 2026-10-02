package dev.shaderbridge.model;

import java.util.List;
import java.util.Optional;

/**
 * Pack-global resource name to binding (ARCHITECTURE §5.2).
 *
 * @param entries every binding
 */
public record BindingTable(List<BindingEntry> entries) {
    public BindingTable {
        entries = Copies.list(entries);
    }

    /**
     * @param name canonical resource name
     * @return its binding, if present
     */
    public Optional<BindingEntry> get(String name) {
        return entries.stream().filter(e -> e.name().equals(name)).findFirst();
    }
}
