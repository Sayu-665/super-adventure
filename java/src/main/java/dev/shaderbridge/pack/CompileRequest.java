package dev.shaderbridge.pack;

import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.CompileEnvironment;
import java.nio.file.Path;

/**
 * Everything the compile worker needs; built on the main thread, where game state is accessible.
 *
 * @param pack         the pack to compile
 * @param environment  compile environment of the running game
 * @param optionValues the user's option values
 * @param settings     compile settings
 * @param glslDumpDir  directory to dump the translated GLSL into, or null
 */
public record CompileRequest(PackEntry pack, CompileEnvironment environment, PackOptionValues optionValues, CompileSettings settings, Path glslDumpDir) {
}
