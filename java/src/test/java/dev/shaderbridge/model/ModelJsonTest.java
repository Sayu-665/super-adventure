package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.shaderbridge.model.json.ModelParseException;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Parses the hand-written sample, which covers every variant of every model enum. */
class ModelJsonTest {
    static String sample;
    static CompiledPack pack;

    static String resource(String name) throws IOException {
        try (InputStream in = ModelJsonTest.class.getResourceAsStream(name)) {
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    @BeforeAll
    static void parseSample() throws Exception {
        sample = resource("compiled_pack_sample.json");
        pack = ModelJson.parse(sample, CompiledPack.class);
    }

    private static DimensionPipeline world0() {
        return pack.dimension("world0").orElseThrow();
    }

    @Test
    void topLevel() {
        assertEquals(CompiledPack.FORMAT_VERSION, pack.formatVersion());
        assertEquals("SamplePack.zip", pack.info().name());
        assertEquals(List.of("BLOCK_EMISSION_ATTRIBUTE"), pack.info().featuresUnsupported());
        assertEquals(3, pack.dimensions().size());
        assertEquals(List.of("world0", "world-1", ""), pack.dimensions().stream().map(DimensionPipeline::folder).toList());
        assertEquals(new BlobInfo(BlobKind.BYTES, 56, 3), pack.blobs().get(2));
    }

    @Test
    void environment() {
        CompileEnvironment env = pack.info().environment();
        assertEquals(DepthMode.REVERSED_ZERO_TO_ONE, env.depthMode());
        assertEquals(List.of(OutputTarget.VULKAN, OutputTarget.RENDERPEARL), env.targets());
        assertEquals("1", env.extraMacros().get("SB_DEBUG"));
        assertTrue(env.extraMacros().containsKey("SB_FLAG"));
        assertNull(env.extraMacros().get("SB_FLAG"));
        assertFalse(env.device().comparisonSamplers());
        assertEquals(32, env.device().maxDescriptorsPerProgram());
    }

    @Test
    void comparisonSamplersDefaultsToTrue() throws ModelParseException {
        String json = "{\"geometry_shader\":true,\"tessellation_shader\":true,\"storage_image_read_without_format\":true,"
            + "\"storage_image_write_without_format\":true,\"depth_clip_control\":false,\"max_push_constants_size\":128,\"max_color_attachments\":8}";
        DeviceCaps caps = ModelJson.parse(json, DeviceCaps.class);
        assertTrue(caps.comparisonSamplers());
        assertNull(caps.maxDescriptorsPerProgram());
    }

    /** Fields serde reads with {@code #[serde(default)]} may be absent (models of older builds). */
    @Test
    void slotVariantsAndShadowEmulationDefault() throws ModelParseException {
        GeometrySlot slot = ModelJson.parse("{\"program\":3,\"resolved_from\":\"terrain_solid\"}", GeometrySlot.class);
        assertEquals(new GeometrySlot(3, GeometryProgram.TERRAIN_SOLID), slot);
        assertEquals(Map.of(), slot.variants());
        GeometrySlot withVariants = ModelJson.parse("{\"program\":3,\"resolved_from\":\"terrain\",\"variants\":{\"sodium_terrain\":7}}", GeometrySlot.class);
        assertEquals(Map.of("sodium_terrain", 7), withVariants.variants());
        assertThrows(UnsupportedOperationException.class, () -> withVariants.variants().put("x", 1));
        assertEquals("{\"program\":3,\"resolved_from\":\"terrain\",\"variants\":{\"sodium_terrain\":7}}", ModelJson.toJson(withVariants));

        String use = "{\"name\":\"shadowtex0\",\"set\":1,\"binding\":4,\"use_alt\":false,\"stages\":[\"fragment\"]";
        BindingUse plain = ModelJson.parse(use + "}", BindingUse.class);
        assertFalse(plain.shadowEmulated());
        assertEquals(new BindingUse("shadowtex0", 1, 4, false, List.of(ShaderStage.FRAGMENT)), plain);
        BindingUse emulated = ModelJson.parse(use + ",\"shadow_emulated\":true}", BindingUse.class);
        assertTrue(emulated.shadowEmulated());
        assertEquals(use + ",\"shadow_emulated\":true}", ModelJson.toJson(emulated));
    }

    @Test
    void optionsModel() {
        OptionsModel options = pack.options();
        assertEquals(OptionKind.BOOLEAN_DEFINE, options.options().get(0).kind());
        assertEquals("true", options.options().get(0).defaultValue());
        assertEquals(OptionKind.VALUE_DEFINE, options.option("SHADOW_QUALITY").orElseThrow().kind());
        assertEquals(OptionKind.CONST, options.option("sunPathRotation").orElseThrow().kind());
        assertEquals(List.of(
            new ScreenEntry.ProfileSelector(),
            new ScreenEntry.Empty(),
            new ScreenEntry.OptionEntry("SHADOWS"),
            new ScreenEntry.ScreenLink("LIGHTING"),
            new ScreenEntry.Rest()), options.mainScreen());
        assertEquals(2, options.mainScreenColumns());
        assertEquals(1, options.screens().get("LIGHTING").columns());
        assertNull(options.screens().get("EMPTY").columns());
        assertEquals(List.of("LOW", "HIGH"), List.copyOf(options.profiles().keySet()));
        assertEquals("1", options.profiles().get("LOW").get("SHADOW_QUALITY"));
        assertNull(options.currentProfile());
        assertEquals(List.of("world0/composite2"), options.profileDisabledPrograms().get("LOW"));
        assertEquals(" deg", options.lang().get("suffix.sunPathRotation"));
    }

    @Test
    void idMapsKeepIntegerKeysInOrder() {
        IdMaps ids = pack.idMaps();
        assertEquals(List.of(10001, 10002), List.copyOf(ids.blocks().keySet()));
        assertEquals(List.of("minecraft:wheat:age=7", "%minecraft:logs"), ids.blocks().get(10002));
        assertEquals(List.of("*"), ids.dimensions().get("world1"));
    }

    @Test
    void renderTargets() {
        RenderTargets targets = world0().targets();
        assertEquals(new TargetSize.Relative(1, 1), targets.colortex().get(0).size());
        assertEquals(new TargetSize.Absolute(512, 256), targets.colortex().get(1).size());
        assertEquals(new TargetSize.PerAxis(new AxisSize.Relative(0.5f), new AxisSize.Absolute(64)), targets.colortex().get(2).size());
        assertEquals(TextureFormat.R11F_G11F_B10F, targets.colortex().get(1).format());
        assertEquals(List.of(0f, 0.5f, 1f, 1f), targets.colortex().get(1).clearColor());
        assertNull(targets.colortex().get(0).clearColor());
        assertEquals(new TextureSource.PackImage("tex/noise.png"), targets.noiseTexture());
        assertEquals(new TextureSource.Resource("minecraft:textures/environment/clouds.png"), targets.customTextures().get(0).source());
        assertEquals(new TextureSource.Dynamic("minecraft:dynamic/lightmap_1"), targets.customTextures().get(1).source());
        TextureSource.Raw raw = (TextureSource.Raw) targets.customTextures().get(2).source();
        assertEquals(List.of(32, 32, 32), raw.size());
        assertEquals(TextureFormat.RGB8, raw.format());
        assertEquals(new ImageSize.Absolute3D(64, 64, 64), targets.images().get(0).size());
        assertEquals(new ImageSize.Absolute1D(256), targets.images().get(1).size());
        assertEquals(new ImageSize.Relative(0.5f, 0.5f), targets.images().get(2).size());
        assertEquals(new ImageSize.Absolute2D(16, 16), targets.images().get(3).size());
        assertEquals(List.of(1f, 0.5f), targets.buffers().get(1).relative());
        assertNull(targets.buffers().get(0).relative());
        ShadowSettings shadow = targets.shadow();
        assertNull(shadow.fov());
        assertEquals(List.of(true, false), shadow.hardwareFiltering());
        assertEquals(90f, pack.dimension("world-1").orElseThrow().targets().shadow().fov());
    }

    @Test
    void targetSizeResolution() {
        assertEquals("[960, 540]", Arrays.toString(new TargetSize.Relative(0.5f, 0.5f).resolve(1920, 1080)));
        assertEquals("[1, 1]", Arrays.toString(new TargetSize.Absolute(0, 0).resolve(1920, 1080)));
        assertEquals("[640, 64]", Arrays.toString(new TargetSize.PerAxis(new AxisSize.Relative(1 / 3f), new AxisSize.Absolute(64)).resolve(1920, 1080)));
    }

    @Test
    void uniforms() {
        UniformLayout layout = world0().uniforms();
        assertEquals(112, layout.frame().size());
        BlockMember view = layout.frame().member("gbufferModelView").orElseThrow();
        assertEquals(GlslType.MAT4, view.ty());
        assertEquals(new UniformSource.Builtin("gbufferModelView"), view.source());
        assertEquals(new UniformSource.Custom("screenDark"), layout.frame().member("screenDark").orElseThrow().source());
        BlockMember tint = layout.frame().member("packTint").orElseThrow();
        assertEquals(new UniformSource.Unset(), tint.source());
        assertEquals(List.of(0.25f, 0.75f), tint.defaultValues());
        assertEquals(GlslType.FLOAT.withArray(1), layout.frame().member("weights").orElseThrow().ty());
        assertEquals(new UniformSource.Builtin("hideGUI"), layout.frame().member("hideGUI__int").orElseThrow().source());
        assertEquals(GlslType.VEC4, layout.draw().member("entityColor").orElseThrow().ty());
        CustomUniform dayFactor = world0().customUniforms().get(1);
        assertTrue(dayFactor.isVariable());
        assertEquals(GlslType.BOOL, dayFactor.ty());
        assertNull(dayFactor.location());
        assertEquals(new SourceLocation("shaders.properties", 42, null), world0().customUniforms().get(0).location());
    }

    @Test
    void everyResourceRefVariant() {
        List<ResourceRef> refs = world0().bindings().entries().stream().map(BindingEntry::resource).toList();
        assertEquals(List.of(
            new ResourceRef.ColorTex(0),
            new ResourceRef.DepthTex(0),
            new ResourceRef.ShadowTex(0),
            new ResourceRef.ShadowTexHw(0),
            new ResourceRef.ShadowColor(0),
            new ResourceRef.Noise(),
            new ResourceRef.Atlas(),
            new ResourceRef.Lightmap(),
            new ResourceRef.Normals(),
            new ResourceRef.Specular(),
            new ResourceRef.Overlay(),
            new ResourceRef.DhDepthTex(0),
            new ResourceRef.DhBlockAtlas(),
            new ResourceRef.White(),
            new ResourceRef.CustomTexture("custom.lutTex.3d"),
            new ResourceRef.Image("voxelImg"),
            new ResourceRef.ColorImage(0),
            new ResourceRef.ShadowColorImage(0),
            new ResourceRef.Ssbo(0),
            new ResourceRef.UniformBlock("CustomBlock"),
            new ResourceRef.Unknown("mysterySampler")), refs);
        assertEquals(ResourceRef.class.getPermittedSubclasses().length, refs.size());
    }

    @Test
    void resourceKinds() {
        BindingTable table = world0().bindings();
        assertEquals(new ResourceKind.Sampler("2d", true, "float"), table.get("shadowtex0").orElseThrow().kind());
        assertEquals(new ResourceKind.StorageImage("2d", "rgba16f", "float", false, true), table.get("colorimg0").orElseThrow().kind());
        assertEquals(new ResourceKind.StorageImage("2d", null, "float", true, false), table.get("shadowcolorimg0").orElseThrow().kind());
        assertEquals(new ResourceKind.StorageBuffer(), table.get("bufferObject0").orElseThrow().kind());
        assertEquals(new ResourceKind.UniformBuffer(), table.get("CustomBlock").orElseThrow().kind());
    }

    @Test
    void programs() {
        List<Program> programs = world0().programs();
        Program terrain = programs.get(0);
        assertEquals(new ProgramKind.Geometry(GeometryProgram.TERRAIN), terrain.kind());
        assertEquals(new BlobId(0), terrain.stage(ShaderStage.VERTEX).orElseThrow().spirv());
        assertNull(terrain.stage(ShaderStage.FRAGMENT).orElseThrow().spirv());
        assertEquals(List.of(ShaderStage.values()), terrain.bindingsUsed().get(0).stages());
        assertEquals(new BlendMode(BlendFactor.SRC_ALPHA, BlendFactor.ONE_MINUS_SRC_ALPHA, BlendFactor.ONE, BlendFactor.ONE_MINUS_SRC_ALPHA), terrain.blend());
        Map<Integer, BlendMode> perBuffer = terrain.blendPerBuffer();
        assertEquals(List.of(1, 2), List.copyOf(perBuffer.keySet()));
        assertNull(perBuffer.get(1));
        assertEquals(BlendFactor.SRC_COLOR, perBuffer.get(2).dstColor());
        assertEquals(Boolean.TRUE, terrain.cull());
        assertNull(terrain.vertexInputs().get(1).semantic());

        assertEquals(new ProgramKind.Composite(PassGroup.COMPOSITE, 0), programs.get(1).kind());
        assertEquals(new ViewportScale(0.5f, 0.25f, 0.25f), programs.get(1).viewport());
        assertNull(programs.get(1).cull());
        assertEquals(new ProgramKind.Compute(PassGroup.COMPOSITE, 3, 'b'), programs.get(2).kind());
        ComputeInfo compute = programs.get(2).compute();
        assertEquals(List.of(8, 8, 1), compute.localSize());
        assertEquals(new WorkGroups.Absolute(4, 2, 1), compute.workGroups());
        assertEquals(new IndirectDispatch(1, 16), compute.indirect());
        assertEquals(new ProgramKind.Compute(PassGroup.COMPOSITE, 3, null), programs.get(3).kind());
        assertEquals(new WorkGroups.Relative(1f, 0.5f), programs.get(3).compute().workGroups());
        assertNull(programs.get(3).compute().indirect());
        assertEquals(new ProgramKind.GeometryCompute(GeometryProgram.SHADOW, 'a'), programs.get(4).kind());
        assertEquals("world0/gbuffers_terrain", programs.get(5).synthesizedFrom());

        List<AlphaFunc> funcs = programs.stream().map(Program::alphaTest).filter(t -> t != null).map(AlphaTest::func).sorted().toList();
        assertEquals(List.of(AlphaFunc.values()), funcs);
    }

    @Test
    void geometryCoversEveryProgram() {
        DimensionPipeline world0 = world0();
        assertEquals(List.of(GeometryProgram.values()), List.copyOf(world0.geometry().keySet()));
        assertEquals(new GeometrySlot(0, GeometryProgram.TERRAIN), world0.geometry().get(GeometryProgram.DAMAGED_BLOCK));
        assertEquals("world0/gbuffers_hand", world0.programFor(GeometryProgram.HAND_WATER).orElseThrow().name());
        assertTrue(pack.dimension("world-1").orElseThrow().programFor(GeometryProgram.BASIC).isEmpty());
    }

    @Test
    void passesAndDistantHorizons() {
        DimensionPipeline world0 = world0();
        assertEquals(List.of(PassGroup.values()), world0.passes().stream().map(Pass::group).toList());
        Pass composite = world0.passes().get(8);
        assertEquals(Integer.valueOf(1), composite.program());
        assertEquals(List.of(3, 2), composite.computes());
        assertEquals(List.of(true, false, false), composite.flipState());
        assertNull(world0.passes().get(0).program());
        assertEquals(List.of(DhStrategy.SYNTHESIZED, DhStrategy.NATIVE, DhStrategy.DISABLED),
            pack.dimensions().stream().map(d -> d.distantHorizons().strategy()).toList());
        assertEquals(List.of(1), world0.endOfFrameCopies());
        assertEquals(Map.of("solid", true, "translucent", false), world0.settings().backFace());
        assertEquals(-30f, world0.settings().sunPathRotation());
    }

    @Test
    void diagnostics() {
        List<Diagnostic> diagnostics = pack.diagnostics();
        assertEquals(List.of(Severity.ERROR, Severity.WARNING, Severity.INFO), diagnostics.stream().map(Diagnostic::severity).toList());
        assertEquals("error[spv.compile] world0/composite (fragment) at world0/composite.fsh:12:5: 'foo' : undeclared identifier",
            diagnostics.get(0).toString());
        assertEquals("info[dh.synthesized]: dh_terrain synthesized from gbuffers_terrain", diagnostics.get(2).toString());
        assertNull(diagnostics.get(1).stage());
    }

    @Test
    void roundTripPreservesEveryField() throws ModelParseException {
        String written = ModelJson.toJson(pack);
        assertEquals(pack, ModelJson.parse(written, CompiledPack.class));
        assertEquals(withoutNulls(JsonParser.parseString(sample)), withoutNulls(JsonParser.parseString(written)));
    }

    @Test
    void rejectsUnknownVariantsAndValues() {
        assertThrows(ModelParseException.class, () -> ModelJson.parse("{\"type\":\"bogus\",\"value\":1}", ResourceRef.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse("{\"value\":1}", ResourceRef.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse("\"forward\"", DepthMode.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse("{\"type\":\"color_tex\"}", ResourceRef.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse("not json", CompiledPack.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse("{}", CompiledPack.class));
        assertThrows(ModelParseException.class, () -> ModelJson.parse(null, CompiledPack.class));
    }

    /** Rust skips {@code None} fields that Gson writes as {@code null}; both mean "absent". */
    private static JsonElement withoutNulls(JsonElement element) {
        if (element.isJsonObject()) {
            JsonObject out = new JsonObject();
            for (Map.Entry<String, JsonElement> e : element.getAsJsonObject().entrySet()) {
                if (!e.getValue().isJsonNull() || isOptionalMapValue(e.getKey())) {
                    out.add(e.getKey(), withoutNulls(e.getValue()));
                }
            }
            return out;
        }
        if (element.isJsonArray()) {
            var out = new com.google.gson.JsonArray();
            element.getAsJsonArray().forEach(e -> out.add(withoutNulls(e)));
            return out;
        }
        if (element.isJsonPrimitive() && element.getAsJsonPrimitive().isNumber()) {
            return new com.google.gson.JsonPrimitive(element.getAsDouble());
        }
        return element;
    }

    private static boolean isOptionalMapValue(String key) {
        return key.equals("SB_FLAG") || key.equals("1");
    }
}
