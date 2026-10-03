package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The vertex buffer layouts of draw profiles, as Mojang {@link VertexFormat}s whose element names
 * equal the profile's input names (the pipeline builder matches shader inputs to elements by
 * name). A profile whose inputs are all Mojang standard elements ({@code Position}, {@code UV0},
 * ...) gets them in input order with Mojang's formats, per-instance inputs in slot 1; the other
 * built-in profiles have explicit byte layouts; mods register theirs. Thread-safe.
 */
public final class ProfileVertexFormats {
    private static final ProfileVertexFormats SHARED = new ProfileVertexFormats(DrawProfiles.get());

    private final DrawProfiles profiles;
    private final Map<String, List<VertexFormat>> registered = new ConcurrentHashMap<>();

    /** @param profiles where profile definitions come from */
    public ProfileVertexFormats(DrawProfiles profiles) {
        this.profiles = profiles;
    }

    /** @return the registry of the built-in profiles (and those registered at runtime) */
    public static ProfileVertexFormats get() {
        return SHARED;
    }

    /**
     * Registers the layout of a profile drawn from another mod's vertex buffers.
     *
     * @param profile  profile id
     * @param bindings vertex format per vertex buffer slot
     */
    public void register(String profile, List<VertexFormat> bindings) {
        registered.put(profile, List.copyOf(bindings));
    }

    /**
     * @param profile a profile id
     * @return its vertex format per vertex buffer slot (empty list: no vertex buffers), or empty
     *     if the profile is unknown or its inputs have no known layout
     */
    public Optional<List<VertexFormat>> bindings(String profile) {
        List<VertexFormat> own = registered.get(profile);
        if (own != null) {
            return Optional.of(own);
        }
        List<VertexFormat> explicit = HostVertexLayouts.BY_PROFILE.get(profile);
        if (explicit != null) {
            return Optional.of(explicit);
        }
        return profiles.profile(profile).flatMap(ProfileVertexFormats::derive);
    }

    /**
     * Builds the layout of a profile made of Mojang standard elements.
     *
     * @param info a profile
     * @return its bindings, or empty if an input is not a Mojang element
     */
    static Optional<List<VertexFormat>> derive(DrawProfileInfo info) {
        List<DrawProfileInfo.Input> inputs = new ArrayList<>(info.inputs());
        inputs.sort(Comparator.comparingInt(DrawProfileInfo.Input::location));
        if (inputs.stream().anyMatch(i -> !HostVertexLayouts.MOJANG_ELEMENTS.containsKey(i.name()))) {
            return Optional.empty();
        }
        List<DrawProfileInfo.Input> perVertex = inputs.stream().filter(i -> !i.instanced()).toList();
        List<DrawProfileInfo.Input> perInstance = inputs.stream().filter(DrawProfileInfo.Input::instanced).toList();
        if (perVertex.isEmpty() && !perInstance.isEmpty()) {
            return Optional.empty();
        }
        List<VertexFormat> bindings = new ArrayList<>();
        if (!perVertex.isEmpty()) {
            bindings.add(format(0, perVertex));
        }
        if (!perInstance.isEmpty()) {
            bindings.add(format(1, perInstance));
        }
        return Optional.of(List.copyOf(bindings));
    }

    private static VertexFormat format(int stepRate, List<DrawProfileInfo.Input> inputs) {
        VertexFormat.Builder builder = VertexFormat.builder(stepRate);
        for (DrawProfileInfo.Input input : inputs) {
            builder.addAttribute(input.name(), HostVertexLayouts.MOJANG_ELEMENTS.get(input.name()));
        }
        return builder.build();
    }

    /**
     * Whether vertex buffers laid out as {@code host} feed a program translated for a profile laid
     * out as {@code profile}: every profile element must exist in the same slot of the host
     * layout, with the same name, format and step rate. The host may have more elements (only
     * the program's inputs are read) and other offsets (they come from the host layout).
     *
     * @param profile the profile's layout
     * @param host    the layout of the vertex buffers the host binds
     * @return the differences, empty if compatible
     */
    public static List<String> compatibility(List<VertexFormat> profile, List<VertexFormat> host) {
        List<String> problems = new ArrayList<>();
        for (int slot = 0; slot < profile.size(); slot++) {
            VertexFormat want = profile.get(slot);
            if (want == null) {
                continue;
            }
            VertexFormat have = slot < host.size() ? host.get(slot) : null;
            if (have == null) {
                problems.add("vertex buffer slot " + slot + " (" + want + ") is not bound by the host");
                continue;
            }
            if (have.getStepRate() != want.getStepRate()) {
                problems.add("vertex buffer slot " + slot + " has step rate " + have.getStepRate() + ", the profile expects " + want.getStepRate());
            }
            for (VertexFormatElement element : want.getElements()) {
                VertexFormatElement other = have.getElement(element.name());
                GpuFormat format = other == null ? null : other.format();
                if (format == null) {
                    problems.add("vertex element " + element.name() + " is missing from slot " + slot + " (" + have + ")");
                } else if (format != element.format()) {
                    problems.add("vertex element " + element.name() + " is " + format + ", the profile expects " + element.format());
                }
            }
        }
        return problems;
    }
}
