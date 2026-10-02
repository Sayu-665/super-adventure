package dev.shaderbridge.model;

import com.google.gson.annotations.SerializedName;
import dev.shaderbridge.model.json.OmitIfNull;
import java.util.List;

/**
 * One member of a uniform block.
 *
 * @param name          GLSL member name (the pack's uniform name, or {@code name__type} for type conflicts)
 * @param ty            declared type
 * @param offset        std140 byte offset
 * @param source        where the host gets the value from
 * @param defaultValues constant initializer components in GLSL constructor order (column-major,
 *                      unpadded), or null; hosts pad them into std140 when uploading
 */
public record BlockMember(
    String name,
    GlslType ty,
    int offset,
    UniformSource source,
    @OmitIfNull @SerializedName("default") List<Float> defaultValues
) {
    public BlockMember {
        Copies.required(name, "name");
        Copies.required(ty, "ty");
        Copies.required(source, "source");
        defaultValues = defaultValues == null ? null : Copies.list(defaultValues);
    }
}
