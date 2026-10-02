package dev.shaderbridge.model;

import dev.shaderbridge.model.json.Tag;

/** One entry of an options screen ({@code #[serde(tag = "type", content = "value")]}). */
public sealed interface ScreenEntry {
    /**
     * An option.
     *
     * @param name the option shown here
     */
    @Tag("option")
    record OptionEntry(String name) implements ScreenEntry {
    }

    /**
     * {@code [NAME]}: a link to a sub-screen.
     *
     * @param screen the target screen name
     */
    @Tag("screen")
    record ScreenLink(String screen) implements ScreenEntry {
    }

    /** {@code <profile>}: the profile selector. */
    @Tag("profile")
    record ProfileSelector() implements ScreenEntry {
    }

    /** {@code <empty>}: a spacer. */
    record Empty() implements ScreenEntry {
    }

    /** {@code *}: every option not placed on any screen. */
    record Rest() implements ScreenEntry {
    }
}
