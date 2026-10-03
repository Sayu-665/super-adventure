package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.ArrayList;
import java.util.Collections;
import java.util.EnumMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;

/**
 * The reflected interface of a program's SPIR-V stages: its descriptors by name (one entry per
 * name, as Mojang's pipeline builder merges them across stages), the vertex inputs and the
 * fragment outputs.
 *
 * @param stages      reflection of every stage that has SPIR-V
 * @param descriptors descriptors by name, in first-declaration order (vertex stage first)
 * @param conflicts   names declared with different types or dimensions by different stages
 */
public record ProgramInterface(Map<ShaderStage, SpirvReflection> stages, Map<String, Descriptor> descriptors, List<String> conflicts) {
    public ProgramInterface {
        stages = Collections.unmodifiableMap(new EnumMap<>(stages));
        descriptors = Collections.unmodifiableMap(new LinkedHashMap<>(descriptors));
        conflicts = List.copyOf(conflicts);
    }

    /**
     * Reflects every stage of a program.
     *
     * @param program a program
     * @param blobs   the blob table its stage modules point into
     * @return the merged interface
     * @throws SpirvReflector.InvalidSpirvException if a module is malformed
     * @throws IllegalArgumentException             if a stage has no SPIR-V blob
     */
    public static ProgramInterface reflect(Program program, Blobs blobs) {
        Map<ShaderStage, SpirvReflection> stages = new EnumMap<>(ShaderStage.class);
        for (StageModule module : program.stages()) {
            if (module.spirv() == null) {
                throw new IllegalArgumentException(program.name() + ": the " + module.stage().wireName() + " stage has no SPIR-V");
            }
            stages.put(module.stage(), SpirvReflector.reflect(blobs.spirv(module.spirv())));
        }
        return of(stages);
    }

    /**
     * Merges stage reflections.
     *
     * @param stages reflection per stage
     * @return the merged interface
     */
    public static ProgramInterface of(Map<ShaderStage, SpirvReflection> stages) {
        Map<String, Descriptor> descriptors = new LinkedHashMap<>();
        List<String> conflicts = new ArrayList<>();
        for (SpirvReflection stage : new EnumMap<>(stages).values()) {
            for (Descriptor d : stage.descriptors()) {
                Descriptor previous = descriptors.putIfAbsent(d.name(), d);
                if (previous != null && (previous.type() != d.type() || previous.dim() != d.dim()) && !conflicts.contains(d.name())) {
                    conflicts.add(d.name());
                }
            }
        }
        return new ProgramInterface(stages, descriptors, conflicts);
    }

    /**
     * @param stage a stage
     * @return its reflection, if the program has the stage
     */
    public Optional<SpirvReflection> stage(ShaderStage stage) {
        return Optional.ofNullable(stages.get(stage));
    }

    /** @return the vertex stage's inputs (empty without a vertex stage) */
    public List<InterfaceVariable> vertexInputs() {
        return stage(ShaderStage.VERTEX).map(SpirvReflection::inputs).orElse(List.of());
    }

    /**
     * Fragment output locations with their numeric class; arrays and matrices occupy several
     * locations.
     *
     * @return location to class, sorted by location
     */
    public Map<Integer, ScalarClass> fragmentOutputs() {
        Map<Integer, ScalarClass> out = new TreeMap<>();
        for (InterfaceVariable o : stage(ShaderStage.FRAGMENT).map(SpirvReflection::outputs).orElse(List.of())) {
            if (o.location() >= 0) {
                for (int i = 0; i < o.locationCount(); i++) {
                    out.putIfAbsent(o.location() + i, o.scalar());
                }
            }
        }
        return out;
    }
}
