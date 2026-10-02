package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.FieldNamingPolicy;
import com.google.gson.annotations.SerializedName;
import dev.shaderbridge.model.json.OmitIfNull;
import dev.shaderbridge.model.json.SerdeNames;
import dev.shaderbridge.model.json.Tag;
import dev.shaderbridge.model.json.WireEnum;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.RecordComponent;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

/**
 * Checks the Java mirror against the serde declarations in {@code crates/sb-core}: every
 * serialized struct has a record with the same JSON field names, every enum has the same wire
 * names, in the same order.
 */
class ContractSyncTest {
    private static final List<String> FILES = List.of(
        "crates/sb-core/src/model.rs",
        "crates/sb-core/src/diag.rs",
        "crates/sb-core/src/glsl_type.rs",
        "crates/sb-core/src/program.rs",
        "crates/sb-core/src/stage.rs");
    /** Rust types that are not part of the CompiledPack JSON. */
    private static final Set<String> NOT_IN_MODEL = Set.of("GeometryGroup", "ProgramName");
    /** Rust names that are spelled differently in Java to avoid clashes. */
    private static final Map<String, String> JAVA_NAMES = Map.of("Screen", "OptionScreen");

    @Test
    void everySerializedRustTypeIsMirrored() throws Exception {
        List<String> checked = new ArrayList<>();
        int skipped = 0;
        for (String file : FILES) {
            for (RustSource.Item item : RustSource.items(RustSource.read(file)).values()) {
                if (!item.attributes().contains("Serialize") || NOT_IN_MODEL.contains(item.name())) {
                    continue;
                }
                Class<?> type = Class.forName("dev.shaderbridge.model." + JAVA_NAMES.getOrDefault(item.name(), item.name()));
                if (item.kind().equals("struct")) {
                    assertTrue(type.isRecord(), type + " must be a record");
                    assertEquals(item.fields(), jsonFields(type), "fields of " + item.name());
                    assertEquals(item.skippedFields(), omittedFields(type), "skip_serializing_if fields of " + item.name());
                    skipped += item.skippedFields().size();
                } else if (type.isEnum()) {
                    assertEquals(wireNames(item.variants(), item.renameAll()), enumWireNames(type), "values of " + item.name());
                } else {
                    checkUnion(item, type);
                }
                checked.add(item.name());
            }
        }
        assertTrue(checked.size() > 50, "only checked " + checked);
        assertEquals(8, skipped, "skip_serializing_if fields found in the Rust sources");
    }

    @Test
    void geometryProgramsMatchTheMacro() throws IOException {
        String source = RustSource.read("crates/sb-core/src/program.rs");
        Matcher m = Pattern.compile("(?m)^    (\\w+) = \"(\\w+)\", fallback").matcher(source);
        List<String> wire = new ArrayList<>();
        List<String> files = new ArrayList<>();
        while (m.find()) {
            wire.add(SerdeNames.snakeCase(m.group(1)));
            files.add(m.group(2));
        }
        assertEquals(wire, enumWireNames(GeometryProgram.class));
        assertEquals(files, Arrays.stream(GeometryProgram.values()).map(GeometryProgram::fileName).toList());
    }

    @Test
    void textureFormatsMatchTheMacro() throws IOException {
        String source = RustSource.read("crates/sb-core/src/format.rs");
        Matcher m = Pattern.compile("(?m)^    \\w+ = \"(\\w+)\", \\d+").matcher(source);
        List<String> names = new ArrayList<>();
        while (m.find()) {
            names.add(m.group(1));
        }
        assertEquals(58, names.size());
        assertEquals(names, enumWireNames(TextureFormat.class));
    }

    private static void checkUnion(RustSource.Item item, Class<?> type) {
        assertTrue(type.isSealed(), type + " must be sealed");
        Map<String, Class<?>> javaVariants = new LinkedHashMap<>();
        for (Class<?> sub : type.getPermittedSubclasses()) {
            Tag tag = sub.getAnnotation(Tag.class);
            javaVariants.put(tag != null ? tag.value() : SerdeNames.snakeCase(sub.getSimpleName()), sub);
        }
        List<String> rustVariants = item.variants();
        assertEquals(wireNames(rustVariants, item.renameAll()), List.copyOf(javaVariants.keySet()), "variants of " + item.name());
        if (!item.attributes().contains("content =")) {
            for (String variant : rustVariants) {
                List<String> rustFields = variantFields(item.body(), variant);
                assertEquals(rustFields, jsonFields(javaVariants.get(SerdeNames.snakeCase(variant))), "fields of " + item.name() + "::" + variant);
            }
        }
    }

    private static List<String> variantFields(String body, String variant) {
        Matcher m = Pattern.compile("(?m)^    " + variant + "\\s*\\{").matcher(body);
        if (!m.find()) {
            return List.of();
        }
        String fields = body.substring(m.end(), body.indexOf('}', m.end()));
        fields = fields.replaceAll("(?m)//.*$", "");
        List<String> out = new ArrayList<>();
        Matcher f = Pattern.compile("(\\w+)\\s*:").matcher(fields);
        while (f.find()) {
            out.add(f.group(1));
        }
        return out;
    }

    private static List<String> wireNames(List<String> variants, String renameAll) {
        return variants.stream().map(v -> switch (renameAll == null ? "" : renameAll) {
            case "snake_case" -> SerdeNames.snakeCase(v);
            case "lowercase" -> v.toLowerCase(java.util.Locale.ROOT);
            default -> v;
        }).toList();
    }

    private static List<String> enumWireNames(Class<?> type) {
        return Arrays.stream(type.getEnumConstants()).map(c -> ((WireEnum) c).wireName()).toList();
    }

    private static List<String> omittedFields(Class<?> record) {
        List<String> all = jsonFields(record);
        List<String> out = new ArrayList<>();
        RecordComponent[] components = record.getRecordComponents();
        for (int i = 0; i < components.length; i++) {
            if (components[i].isAnnotationPresent(OmitIfNull.class)) {
                out.add(all.get(i));
            }
        }
        return out;
    }

    private static List<String> jsonFields(Class<?> record) {
        List<String> out = new ArrayList<>();
        for (RecordComponent component : record.getRecordComponents()) {
            try {
                Field field = record.getDeclaredField(component.getName());
                SerializedName explicit = field.getAnnotation(SerializedName.class);
                out.add(explicit != null ? explicit.value() : FieldNamingPolicy.LOWER_CASE_WITH_UNDERSCORES.translateName(field));
            } catch (NoSuchFieldException e) {
                throw new AssertionError(e);
            }
        }
        return out;
    }
}
