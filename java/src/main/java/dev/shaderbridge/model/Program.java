package dev.shaderbridge.model;

import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * One translated program.
 *
 * @param name              e.g. {@code world0/gbuffers_terrain}, {@code composite3_b}
 * @param kind              what the program is
 * @param drawProfile       draw profile id, or null for compute programs
 * @param requiresRawVulkan needs features Mojang's pipeline API cannot express
 * @param stages            stage modules
 * @param drawBuffers       target of each logical fragment output
 * @param outputSlots       physical output location of each logical output
 * @param outputTypes       output base type per location ({@code float}, {@code int}, {@code uint})
 * @param blend             blend mode, or null for no blending
 * @param blendPerBuffer    per-target blend overrides (null value = blending off)
 * @param alphaTest         alpha test, or null
 * @param viewport          viewport scale and offset
 * @param mipmapTargets     targets whose mipmaps must be generated first
 * @param bindingsUsed      resources the program uses
 * @param vertexInputs      vertex attributes
 * @param pushConstantSize  push constant block size in bytes
 * @param compute           compute dispatch information, or null
 * @param cull              back-face culling override, or null for the host default (ShaderBridge
 *                          follows Iris 26.3, which ignores {@code backFace.*}: always null)
 * @param synthesizedFrom   source program of a synthesized program, or null
 * @param inheritBlend      a geometry program without {@code blend.<program>} or a program-file
 *                          override: like Iris, the host keeps the blend of the draw the program
 *                          replaces ({@code blend} is only a stand-in), or uses the slot's
 *                          {@link GeometrySlot#blend()} when it draws the geometry itself. serde
 *                          {@code #[serde(default)]}: false when absent.
 */
public record Program(
    String name,
    ProgramKind kind,
    String drawProfile,
    boolean requiresRawVulkan,
    List<StageModule> stages,
    List<Integer> drawBuffers,
    List<Integer> outputSlots,
    List<String> outputTypes,
    BlendMode blend,
    Map<Integer, BlendMode> blendPerBuffer,
    AlphaTest alphaTest,
    ViewportScale viewport,
    List<Integer> mipmapTargets,
    List<BindingUse> bindingsUsed,
    List<VertexInput> vertexInputs,
    int pushConstantSize,
    ComputeInfo compute,
    Boolean cull,
    String synthesizedFrom,
    boolean inheritBlend
) {
    public Program {
        Copies.required(name, "name");
        Copies.required(kind, "kind");
        stages = Copies.list(stages);
        drawBuffers = Copies.list(drawBuffers);
        outputSlots = Copies.list(outputSlots);
        outputTypes = Copies.list(outputTypes);
        blendPerBuffer = Copies.map(blendPerBuffer);
        Copies.required(viewport, "viewport");
        mipmapTargets = Copies.list(mipmapTargets);
        bindingsUsed = Copies.list(bindingsUsed);
        vertexInputs = Copies.list(vertexInputs);
    }

    /**
     * A program whose blend applies as it is ({@code inheritBlend} false).
     *
     * @param name              program name
     * @param kind              what the program is
     * @param drawProfile       draw profile id, or null for compute programs
     * @param requiresRawVulkan needs features Mojang's pipeline API cannot express
     * @param stages            stage modules
     * @param drawBuffers       target of each logical fragment output
     * @param outputSlots       physical output location of each logical output
     * @param outputTypes       output base type per location
     * @param blend             blend mode, or null for no blending
     * @param blendPerBuffer    per-target blend overrides
     * @param alphaTest         alpha test, or null
     * @param viewport          viewport scale and offset
     * @param mipmapTargets     targets whose mipmaps must be generated first
     * @param bindingsUsed      resources the program uses
     * @param vertexInputs      vertex attributes
     * @param pushConstantSize  push constant block size in bytes
     * @param compute           compute dispatch information, or null
     * @param cull              back-face culling override, or null
     * @param synthesizedFrom   source program of a synthesized program, or null
     */
    public Program(String name, ProgramKind kind, String drawProfile, boolean requiresRawVulkan, List<StageModule> stages,
                   List<Integer> drawBuffers, List<Integer> outputSlots, List<String> outputTypes, BlendMode blend,
                   Map<Integer, BlendMode> blendPerBuffer, AlphaTest alphaTest, ViewportScale viewport, List<Integer> mipmapTargets,
                   List<BindingUse> bindingsUsed, List<VertexInput> vertexInputs, int pushConstantSize, ComputeInfo compute, Boolean cull,
                   String synthesizedFrom) {
        this(name, kind, drawProfile, requiresRawVulkan, stages, drawBuffers, outputSlots, outputTypes, blend, blendPerBuffer, alphaTest, viewport,
            mipmapTargets, bindingsUsed, vertexInputs, pushConstantSize, compute, cull, synthesizedFrom, false);
    }

    /**
     * @param blend        the blend to draw with (null for none)
     * @param inheritBlend whether it still stands for the replaced draw's blend
     * @param alphaTest    the alpha test to draw with (null for none)
     * @return this program with another blend and alpha test
     */
    public Program withDrawState(BlendMode blend, boolean inheritBlend, AlphaTest alphaTest) {
        return new Program(name, kind, drawProfile, requiresRawVulkan, stages, drawBuffers, outputSlots, outputTypes, blend, blendPerBuffer, alphaTest,
            viewport, mipmapTargets, bindingsUsed, vertexInputs, pushConstantSize, compute, cull, synthesizedFrom, inheritBlend);
    }

    /**
     * @param stage a shader stage
     * @return the module of that stage, if the program has it
     */
    public Optional<StageModule> stage(ShaderStage stage) {
        return stages.stream().filter(s -> s.stage() == stage).findFirst();
    }
}
