package dev.shaderbridge.model;

import java.util.List;

/**
 * A {@code screen.<NAME>} sub-screen ({@code sb_core::model::Screen}).
 *
 * @param entries entries in display order
 * @param columns {@code screen.<NAME>.columns}, or null for the default
 */
public record OptionScreen(List<ScreenEntry> entries, Integer columns) {
    public OptionScreen {
        entries = Copies.list(entries);
    }
}
