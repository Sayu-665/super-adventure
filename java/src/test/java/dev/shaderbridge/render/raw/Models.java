package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.BindingTable;
import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.RenderTargets;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.SpirvReflection;
import dev.shaderbridge.render.pipeline.SpirvReflector;
import java.util.EnumMap;
import java.util.Map;

/** Copies of fixture models with one part replaced, and their reflections. */
final class Models {
    private Models() {
    }

    static DimensionPipeline withBindings(DimensionPipeline d, BindingTable bindings) {
        return new DimensionPipeline(d.folder(), d.dimensionIds(), d.targets(), d.settings(), d.uniforms(), d.customUniforms(), bindings, d.programs(),
            d.geometry(), d.passes(), d.gbufferAttachments(), d.shadowAttachments(), d.endOfFrameCopies(), d.distantHorizons());
    }

    static DimensionPipeline withTargets(DimensionPipeline d, RenderTargets targets) {
        return new DimensionPipeline(d.folder(), d.dimensionIds(), targets, d.settings(), d.uniforms(), d.customUniforms(), d.bindings(), d.programs(),
            d.geometry(), d.passes(), d.gbufferAttachments(), d.shadowAttachments(), d.endOfFrameCopies(), d.distantHorizons());
    }

    static Program with(Program p, ProgramKind kind, ComputeInfo compute) {
        return new Program(p.name(), kind, p.drawProfile(), p.requiresRawVulkan(), p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(),
            p.blend(), p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(),
            compute, p.cull(), p.synthesizedFrom());
    }

    /** @return the reflection of each stage of a fixture program */
    static Map<ShaderStage, SpirvReflection> reflect(RenderFixture fixture, Program program) {
        Map<ShaderStage, SpirvReflection> out = new EnumMap<>(ShaderStage.class);
        for (StageModule stage : program.stages()) {
            out.put(stage.stage(), SpirvReflector.reflect(fixture.blobs().spirv(stage.spirv())));
        }
        return out;
    }
}
