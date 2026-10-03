package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.util.List;
import org.junit.jupiter.api.Test;

class ModelValidationTest {
    private static JsonObject serdePack() throws Exception {
        return JsonParser.parseString(ModelJsonTest.resource("compiled_pack_serde.json")).getAsJsonObject();
    }

    private static List<String> problems(JsonObject pack) throws Exception {
        return ModelValidation.problems(ModelJson.parse(pack.toString(), CompiledPack.class));
    }

    @Test
    void consistentModelsHaveNoProblems() throws Exception {
        assertEquals(List.of(), problems(serdePack()));
        assertEquals(List.of(), ModelValidation.problems(ModelJson.parse(ModelJsonTest.resource("compiled_pack_sample.json"), CompiledPack.class)));
    }

    @Test
    void danglingProgramIndicesAndBlobsAreReported() throws Exception {
        JsonObject pack = serdePack();
        JsonObject world0 = pack.getAsJsonArray("dimensions").get(0).getAsJsonObject();
        world0.getAsJsonObject("geometry").getAsJsonObject("terrain").addProperty("program", 99);
        JsonObject stage = world0.getAsJsonArray("programs").get(0).getAsJsonObject().getAsJsonArray("stages").get(0).getAsJsonObject();
        stage.addProperty("spirv", 500);
        stage.addProperty("glsl_vulkan", 0);
        List<String> problems = problems(pack);
        assertEquals(3, problems.size(), problems.toString());
        assertTrue(problems.get(0).contains("geometry terrain refers to program 99"), problems.get(0));
        assertTrue(problems.get(1).contains("spirv refers to blob 500"), problems.get(1));
        assertTrue(problems.get(2).contains("glsl_vulkan refers to a spirv blob"), problems.get(2));
    }
}
