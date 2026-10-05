package dev.shaderbridge.render.mapping;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.render.chunk.ExtendedTerrainPipelines;
import dev.shaderbridge.render.pipeline.DrawProfileInfo;
import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Collectors;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.resources.Identifier;
import org.junit.jupiter.api.Test;

/**
 * Checks the vanilla pipeline table against Minecraft 26.3's {@code RenderPipelines} (the game jar
 * on the test class path): every pipeline is decided explicitly, every entry names a real
 * pipeline, and every mapped pipeline's vertex format and host blocks match its draw profile.
 */
class VanillaPipelineTableTest {
    /** Every vanilla pipeline: the registry (one per location) plus every static field (several share a location). */
    private static List<RenderPipeline> allPipelines() throws IllegalAccessException {
        List<RenderPipeline> out = new ArrayList<>(RenderPipelines.requiredPipelines());
        out.addAll(RenderPipelines.optionalPipelines());
        for (Field f : RenderPipelines.class.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && f.getType() == RenderPipeline.class) {
                f.setAccessible(true);
                out.add((RenderPipeline) f.get(null));
            }
        }
        return out;
    }

    private static Set<String> paths(List<RenderPipeline> pipelines) {
        return pipelines.stream().map(p -> {
            assertEquals(VanillaPipelineTable.NAMESPACE, p.getLocation().getNamespace(), p.getLocation().toString());
            return p.getLocation().getPath();
        }).collect(Collectors.toCollection(TreeSet::new));
    }

    @Test
    void everyVanillaPipelineIsDecidedExplicitly() throws IllegalAccessException {
        Set<String> undecided = new TreeSet<>();
        for (String path : paths(allPipelines())) {
            if (!VanillaPipelineTable.covers(path)) {
                undecided.add(path);
            }
        }
        assertEquals(Set.of(), undecided, "pipelines missing from the table");
    }

    @Test
    void everyEntryNamesAVanillaPipeline() throws IllegalAccessException {
        Set<String> vanilla = paths(allPipelines());
        Set<String> stale = new TreeSet<>(VanillaPipelineTable.entries().keySet());
        stale.removeAll(vanilla);
        assertEquals(Set.of(), stale, "table entries without a 26.3 pipeline");
    }

    @Test
    void mappedPipelinesDrawTheirProfilesVertexFormatAndBindTheirHostBlocks() throws IllegalAccessException {
        ProfileVertexFormats formats = ProfileVertexFormats.get();
        PipelineRouter router = new PipelineRouter(formats);
        Map<String, String> mismatches = new LinkedHashMap<>();
        Set<String> supersets = new TreeSet<>();
        for (RenderPipeline pipeline : allPipelines()) {
            if (!(VanillaPipelineTable.lookup(pipeline.getLocation()) instanceof PipelineMapping.Mapped mapped)) {
                continue;
            }
            String where = pipeline.getLocation().getPath() + " " + pipeline.getVertexFormatBindings();
            assertEquals(mapped, router.route(pipeline), where);
            List<VertexFormat> profile = formats.bindings(mapped.profile()).orElseThrow();
            List<VertexFormat> actual = trimmed(pipeline.getVertexFormatBindings());
            if (!profile.equals(actual)) {
                supersets.add(pipeline.getLocation().getPath() + " " + actual);
            }
            DrawProfileInfo info = DrawProfiles.get().profile(mapped.profile()).orElseThrow();
            Set<String> declared = BindGroupLayout.flattenUniforms(pipeline.getBindGroupLayouts()).stream()
                .map(BindGroupLayout.UniformDescription::name).collect(Collectors.toSet());
            for (String block : info.blocks()) {
                if (!declared.contains(block)) {
                    mismatches.put(where, "does not bind host block " + block);
                }
            }
        }
        assertEquals(Map.of(), mismatches);
        // Only the GLINT_SPECIAL item pipelines carry more than their profile (UV3), which the
        // programs do not read.
        assertEquals(Set.of("pipeline/item_cutout [VertexFormat[Position, Color, UV0, UV1, UV2, UV3, Normal]]",
            "pipeline/item_translucent_glint [VertexFormat[Position, Color, UV0, UV1, UV2, UV3, Normal]]"), supersets);
    }

    private static List<VertexFormat> trimmed(List<VertexFormat> bindings) {
        List<VertexFormat> out = new ArrayList<>(bindings);
        while (!out.isEmpty() && out.getLast() == null) {
            out.removeLast();
        }
        return out;
    }

    @Test
    void mappedEntriesUseGbuffersProgramsKnownProfilesAndShadowPrograms() {
        for (Map.Entry<String, PipelineMapping> e : VanillaPipelineTable.entries().entrySet()) {
            if (e.getValue() instanceof PipelineMapping.Mapped m) {
                assertTrue(m.gbuffers().fileName().startsWith("gbuffers_"), e.getKey());
                assertTrue(m.shadow().map(s -> s.fileName().startsWith("shadow")).orElse(true), e.getKey());
                assertTrue(DrawProfiles.get().profile(m.profile()).isPresent(), e.getKey() + ": " + m.profile());
                assertEquals(Optional.of(m.gbuffers()), m.program(false));
                assertEquals(m.shadow(), m.program(true));
            }
        }
    }

    @Test
    void specifiedRoutes() {
        assertRoute("solid_terrain_multidraw", GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_basic", GeometryProgram.SHADOW_SOLID);
        assertRoute("solid_terrain", GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section", GeometryProgram.SHADOW_SOLID);
        assertRoute("cutout_terrain_multidraw", GeometryProgram.TERRAIN_CUTOUT, "vanilla_terrain_basic", GeometryProgram.SHADOW_CUTOUT);
        assertRoute("translucent_terrain_multidraw", GeometryProgram.WATER, "vanilla_terrain_basic", GeometryProgram.SHADOW_WATER);
        // Iris 26.3 (IrisPipelines): blocks outside chunk meshes use the terrain programs, translucent ones gbuffers_block.
        assertRoute("solid_block", GeometryProgram.TERRAIN_SOLID, "vanilla_block", GeometryProgram.SHADOW_CUTOUT);
        assertRoute("cutout_block", GeometryProgram.TERRAIN_CUTOUT, "vanilla_block", GeometryProgram.SHADOW_CUTOUT);
        assertRoute("translucent_block", GeometryProgram.BLOCK, "vanilla_block", GeometryProgram.SHADOW_WATER);
        for (String entity : List.of("entity_solid", "entity_cutout", "entity_cutout_cull", "entity_cutout_dissolve", "entity_cutout_z_offset",
            "entity_solid_offset_forward", "armor_cutout_no_cull", "armor_decal_cutout_no_cull", "armor_translucent", "energy_swirl",
            "item_cutout", "armor_cutout_no_cull_glint", "entity_solid_glint")) {
            assertRoute(entity, GeometryProgram.ENTITIES, "vanilla_entity", GeometryProgram.SHADOW_ENTITIES);
        }
        for (String translucent : List.of("entity_translucent", "entity_translucent_cull", "item_translucent", "item_translucent_glint",
            "breeze_wind", "banner_pattern")) {
            assertRoute(translucent, GeometryProgram.ENTITIES_TRANSLUCENT, "vanilla_entity", GeometryProgram.SHADOW_ENTITIES);
        }
        assertRoute("entity_shadow", GeometryProgram.ENTITIES_TRANSLUCENT, "vanilla_entity", null);
        assertRoute("eyes", GeometryProgram.SPIDER_EYES, "vanilla_entity", GeometryProgram.SHADOW_ENTITIES);
        assertRoute("entity_translucent_emissive", GeometryProgram.SPIDER_EYES, "vanilla_entity", GeometryProgram.SHADOW_ENTITIES);
        assertRoute("glint", GeometryProgram.ARMOR_GLINT, "vanilla_position_tex", null);
        assertRoute("lightning", GeometryProgram.LIGHTNING, "vanilla_position_color", GeometryProgram.SHADOW_LIGHTNING);
        assertRoute("beacon_beam_opaque", GeometryProgram.BEACON_BEAM, "vanilla_block", GeometryProgram.SHADOW_ENTITIES);
        assertRoute("crumbling", GeometryProgram.DAMAGED_BLOCK, "vanilla_block", null);
        assertRoute("opaque_particle", GeometryProgram.PARTICLES, "vanilla_particle", GeometryProgram.SHADOW);
        assertRoute("translucent_particle", GeometryProgram.PARTICLES_TRANSLUCENT, "vanilla_particle", GeometryProgram.SHADOW);
        assertRoute("weather", GeometryProgram.WEATHER, "vanilla_particle", null);
        assertRoute("sky", GeometryProgram.SKY_BASIC, "vanilla_position", null);
        assertRoute("stars", GeometryProgram.SKY_BASIC, "vanilla_position", null);
        assertRoute("sunrise_sunset", GeometryProgram.SKY_BASIC, "vanilla_position_color", null);
        assertRoute("end_sky", GeometryProgram.SKY_TEXTURED, "vanilla_position_tex_color", null);
        assertRoute("celestial", GeometryProgram.SKY_TEXTURED, "vanilla_position_tex", null);
        assertRoute("clouds", GeometryProgram.CLOUDS, "vanilla_clouds", null);
        assertRoute("flat_clouds", GeometryProgram.CLOUDS, "vanilla_clouds", null);
        assertRoute("lines", GeometryProgram.LINE, "vanilla_lines", null);
        assertRoute("secondary_block_outline", GeometryProgram.LINE, "vanilla_lines", null);
        assertRoute("leash", GeometryProgram.BASIC, "vanilla_position_color_lightmap", GeometryProgram.SHADOW);
        assertRoute("world_border", GeometryProgram.TEXTURED, "vanilla_position_tex", null);
        assertRoute("text", GeometryProgram.ENTITIES_TRANSLUCENT, "vanilla_text", GeometryProgram.SHADOW_ENTITIES);
        assertRoute("text_see_through", GeometryProgram.ENTITIES_TRANSLUCENT, "vanilla_position_tex_color", null);
        assertRoute("end_portal", GeometryProgram.BLOCK, "vanilla_position", GeometryProgram.SHADOW_BLOCK);
        assertRoute("end_gateway", GeometryProgram.BLOCK, "vanilla_position", GeometryProgram.SHADOW_BLOCK);
    }

    private static void assertRoute(String name, GeometryProgram gbuffers, String profile, GeometryProgram shadow) {
        assertEquals(new PipelineMapping.Mapped(gbuffers, Optional.ofNullable(shadow), profile), VanillaPipelineTable.lookupPath("pipeline/" + name), name);
    }

    @Test
    void extendedTerrainLocationsRoundTrip() {
        Identifier vanilla = Identifier.withDefaultNamespace("pipeline/solid_terrain_multidraw");
        Identifier extended = VanillaPipelineTable.extendedTerrainLocation(vanilla);
        assertEquals("shaderbridge:extended_terrain/minecraft/pipeline/solid_terrain_multidraw", extended.toString());
        assertEquals(Optional.of(vanilla), VanillaPipelineTable.extendedTerrainOrigin(extended));
        assertEquals(Optional.empty(), VanillaPipelineTable.extendedTerrainOrigin(vanilla));
        assertEquals(Optional.empty(), VanillaPipelineTable.extendedTerrainOrigin(Identifier.fromNamespaceAndPath("shaderbridge", "extended_terrain/x")));
        assertEquals(Optional.empty(), VanillaPipelineTable.extendedTerrainOrigin(Identifier.fromNamespaceAndPath("othermod", "extended_terrain/a/b")));
        assertEquals(Optional.of("vanilla_terrain"), VanillaPipelineTable.extendedProfile("vanilla_terrain_basic"));
        assertEquals(Optional.of("vanilla_terrain_section_ext"), VanillaPipelineTable.extendedProfile("vanilla_terrain_section"));
        assertEquals(Optional.empty(), VanillaPipelineTable.extendedProfile("vanilla_block"));
        assertEquals(Optional.of("vanilla_terrain_basic"), VanillaPipelineTable.basicProfile("vanilla_terrain"));
        assertEquals(Optional.empty(), VanillaPipelineTable.basicProfile("vanilla_terrain_basic"));
    }

    @Test
    void extendedTerrainClonesRouteToTheExtendedProfiles() {
        PipelineRouter router = new PipelineRouter(ProfileVertexFormats.get());
        Map<RenderPipeline, PipelineMapping> expected = new LinkedHashMap<>();
        expected.put(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, mapped(GeometryProgram.TERRAIN_SOLID, GeometryProgram.SHADOW_SOLID, "vanilla_terrain"));
        expected.put(RenderPipelines.SOLID_TERRAIN, mapped(GeometryProgram.TERRAIN_SOLID, GeometryProgram.SHADOW_SOLID, "vanilla_terrain_section_ext"));
        expected.put(RenderPipelines.CUTOUT_TERRAIN_MULTIDRAW, mapped(GeometryProgram.TERRAIN_CUTOUT, GeometryProgram.SHADOW_CUTOUT, "vanilla_terrain"));
        expected.put(RenderPipelines.CUTOUT_TERRAIN, mapped(GeometryProgram.TERRAIN_CUTOUT, GeometryProgram.SHADOW_CUTOUT, "vanilla_terrain_section_ext"));
        expected.put(RenderPipelines.TRANSLUCENT_TERRAIN_MULTIDRAW, mapped(GeometryProgram.WATER, GeometryProgram.SHADOW_WATER, "vanilla_terrain"));
        expected.put(RenderPipelines.TRANSLUCENT_TERRAIN, mapped(GeometryProgram.WATER, GeometryProgram.SHADOW_WATER, "vanilla_terrain_section_ext"));
        for (Map.Entry<RenderPipeline, PipelineMapping> e : expected.entrySet()) {
            RenderPipeline clone = ExtendedTerrainPipelines.extended(e.getKey());
            assertEquals(e.getValue(), VanillaPipelineTable.lookup(clone.getLocation()), clone.toString());
            assertEquals(e.getValue(), router.route(clone), clone.toString());
            // The vanilla pipeline keeps its basic profile.
            PipelineMapping basic = router.route(e.getKey());
            assertInstanceOf(PipelineMapping.Mapped.class, basic);
            assertEquals(VanillaPipelineTable.basicProfile(((PipelineMapping.Mapped) e.getValue()).profile()).orElseThrow(),
                ((PipelineMapping.Mapped) basic).profile());
        }
        for (RenderPipeline vanillaOnly : List.of(RenderPipelines.WIREFRAME, RenderPipelines.WIREFRAME_MULTIDRAW)) {
            assertInstanceOf(PipelineMapping.Vanilla.class, router.route(ExtendedTerrainPipelines.extended(vanillaOnly)), vanillaOnly.toString());
        }
    }

    @Test
    void extendedLocationsWithoutTheExtensionAttributesFallBackToTheBasicProfile() {
        // A pipeline at an extended location whose buffers carry only the BLOCK elements (another
        // mod's, or a future layout change) still draws with the program, without the extension.
        RenderPipeline vanilla = RenderPipelines.CUTOUT_TERRAIN_MULTIDRAW;
        RenderPipeline relocated = new RenderPipeline(VanillaPipelineTable.extendedTerrainLocation(vanilla.getLocation()), vanilla.getShaders(),
            vanilla.getShaderDefines(), vanilla.getBindGroupLayouts(), vanilla.getColorTargetStates().toArray(ColorTargetState[]::new),
            vanilla.getDepthStencilState(), vanilla.getPolygonMode(), vanilla.isCull(), vanilla.getVertexFormatBindings().toArray(VertexFormat[]::new),
            vanilla.getPrimitiveTopology(), vanilla.pushConstantSize(), vanilla.getSortKey()) {
        };
        assertEquals(mapped(GeometryProgram.TERRAIN_CUTOUT, GeometryProgram.SHADOW_CUTOUT, "vanilla_terrain_basic"),
            new PipelineRouter(ProfileVertexFormats.get()).route(relocated));
    }

    private static PipelineMapping mapped(GeometryProgram gbuffers, GeometryProgram shadow, String profile) {
        return new PipelineMapping.Mapped(gbuffers, Optional.ofNullable(shadow), profile);
    }

    @Test
    void guiDebugOitAndInternalPipelinesStayVanilla() {
        for (String name : List.of("gui", "gui_textured", "crosshair", "debug_quads", "oit_composite", "oit_accumulate_entity", "blit_depth",
            "lightmap", "panorama", "tracy_blit", "outline_cull", "entity_outline_blit", "wireframe", "wireframe_multidraw",
            "water_mask")) {
            assertInstanceOf(PipelineMapping.Vanilla.class, VanillaPipelineTable.lookupPath("pipeline/" + name), name);
        }
        assertInstanceOf(PipelineMapping.Vanilla.class, VanillaPipelineTable.lookup(Identifier.fromNamespaceAndPath("othermod", "pipeline/solid_terrain")));
        assertInstanceOf(PipelineMapping.Vanilla.class, VanillaPipelineTable.lookupPath("pipeline/added_in_a_later_version"));
        assertFalse(VanillaPipelineTable.covers("pipeline/added_in_a_later_version"));
        assertTrue(VanillaPipelineTable.covers("pipeline/oit_transmittance_anything"));
    }
}
