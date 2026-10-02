package dev.shaderbridge.gui;

import dev.shaderbridge.model.PackOption;
import java.util.Map;
import java.util.Optional;

/**
 * Text of the options GUI from the pack's lang strings, with the OptiFine key conventions:
 * {@code option.<NAME>}, {@code option.<NAME>.comment}, {@code value.<NAME>.<value>},
 * {@code prefix.<NAME>}, {@code suffix.<NAME>}, {@code screen.<NAME>}, {@code screen.<NAME>.comment},
 * {@code profile.<NAME>} and {@code profile.comment}.
 */
public final class PackLang {
    private final Map<String, String> strings;

    /** @param strings lang strings of the pack for the selected language */
    public PackLang(Map<String, String> strings) {
        this.strings = strings;
    }

    private String text(String key, String fallback) {
        return strings.getOrDefault(key, fallback);
    }

    /**
     * @param option option name
     * @return its display name
     */
    public String optionName(String option) {
        return text("option." + option, option);
    }

    /**
     * The value as shown on the option's button: the {@code value.<NAME>.<value>} string, or the
     * value with the option's suffix, preceded by its prefix.
     *
     * @param option option name
     * @param value  raw value
     * @return the display text
     */
    public String value(String option, String value) {
        String prefix = text("prefix." + option, "");
        String translated = strings.get("value." + option + "." + value);
        return prefix + (translated != null ? translated : value + text("suffix." + option, ""));
    }

    /**
     * @param option the option
     * @return the tooltip: the {@code option.<NAME>.comment} string, else the source comment
     */
    public Optional<String> comment(PackOption option) {
        String comment = strings.get("option." + option.name() + ".comment");
        if (comment == null && option.comment() != null && !option.comment().isBlank()) {
            comment = option.comment().strip();
        }
        return Optional.ofNullable(comment);
    }

    /**
     * @param screen screen name
     * @return its display name
     */
    public String screenName(String screen) {
        return text("screen." + screen, screen);
    }

    /**
     * @param screen screen name
     * @return its tooltip, if the pack has one
     */
    public Optional<String> screenComment(String screen) {
        return Optional.ofNullable(strings.get("screen." + screen + ".comment"));
    }

    /**
     * @param profile profile name
     * @return its display name
     */
    public String profileName(String profile) {
        return text("profile." + profile, profile);
    }

    /** @return the tooltip of the profile selector, if the pack has one */
    public Optional<String> profileComment() {
        return Optional.ofNullable(strings.get("profile.comment"));
    }
}
