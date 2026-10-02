package dev.shaderbridge.model;

import com.google.gson.annotations.SerializedName;
import java.util.List;

/**
 * One pack option.
 *
 * @param name         option name ({@code #define} or {@code const} name)
 * @param kind         how it is declared
 * @param defaultValue default value as written in the pack ({@code true}/{@code false} for booleans)
 * @param value        current value after user settings
 * @param allowed      allowed values (value options); empty for booleans
 * @param comment      comment text from the source line, or null
 * @param file         file the option was found in (relative to {@code shaders/})
 * @param line         1-based line number
 */
public record PackOption(
    String name,
    OptionKind kind,
    @SerializedName("default") String defaultValue,
    String value,
    List<String> allowed,
    String comment,
    String file,
    int line
) {
    public PackOption {
        Copies.required(name, "name");
        Copies.required(kind, "kind");
        allowed = Copies.list(allowed);
    }
}
