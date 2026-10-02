package dev.shaderbridge.pack;

import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import java.nio.file.Path;
import java.util.List;

/**
 * The {@code settingsJson} argument of the native {@code compile}.
 *
 * @param dimensions world folders to compile, or null for all
 * @param validate   validate the generated SPIR-V
 * @param cacheDir   directory for the native compile cache, or null to disable caching
 * @param threads    native compile threads, 0 = automatic (only sent when non-zero)
 */
public record CompileSettings(List<String> dimensions, boolean validate, Path cacheDir, int threads) {
    public CompileSettings {
        dimensions = dimensions == null ? null : List.copyOf(dimensions);
    }

    /** @return the JSON text: {@code {"dimensions": [...]|null, "validate": bool, "cacheDir": String|null}} */
    public String toJson() {
        JsonObject o = new JsonObject();
        if (dimensions == null) {
            o.add("dimensions", JsonNull.INSTANCE);
        } else {
            JsonArray array = new JsonArray();
            dimensions.forEach(array::add);
            o.add("dimensions", array);
        }
        o.addProperty("validate", validate);
        if (cacheDir == null) {
            o.add("cacheDir", JsonNull.INSTANCE);
        } else {
            o.addProperty("cacheDir", cacheDir.toAbsolutePath().toString());
        }
        if (threads > 0) {
            o.addProperty("threads", threads);
        }
        return o.toString();
    }
}
