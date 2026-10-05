package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.VertexTags;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexEncoder;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Gives Sodium's pre-encoding vertices ShaderBridge's tags ({@link VertexTags}), and copies them
 * along when Sodium copies a vertex ({@code copyVertexTo}, which its translucency sorter uses to
 * keep and split quads).
 */
@Mixin(value = ChunkVertexEncoder.Vertex.class, remap = false)
abstract class ChunkVertexMixin implements VertexTags {
    @Unique
    private int shaderbridge$entityTag;

    @Unique
    private int shaderbridge$blockTag;

    @Unique
    private int shaderbridge$midTexCoordTag;

    @Override
    public int shaderbridge$entity() {
        return shaderbridge$entityTag;
    }

    @Override
    public int shaderbridge$block() {
        return shaderbridge$blockTag;
    }

    @Override
    public int shaderbridge$midTexCoord() {
        return shaderbridge$midTexCoordTag;
    }

    @Override
    public void shaderbridge$tag(int entity, int block, int midTexCoord) {
        shaderbridge$entityTag = entity;
        shaderbridge$blockTag = block;
        shaderbridge$midTexCoordTag = midTexCoord;
    }

    @Inject(method = SodiumTargets.COPY_VERTEX_TO, at = @At("TAIL"), require = 0)
    private static void shaderbridge$copyTags(ChunkVertexEncoder.Vertex from, ChunkVertexEncoder.Vertex to, CallbackInfo ci) {
        if ((Object) from instanceof VertexTags source && (Object) to instanceof VertexTags target) {
            target.shaderbridge$tag(source.shaderbridge$entity(), source.shaderbridge$block(), source.shaderbridge$midTexCoord());
        }
    }
}
