package dev.shaderbridge.model;

import com.google.gson.FieldNamingPolicy;
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import com.google.gson.JsonParser;
import com.google.gson.TypeAdapter;
import com.google.gson.TypeAdapterFactory;
import com.google.gson.reflect.TypeToken;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonWriter;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.model.json.TaggedUnionAdapterFactory;
import dev.shaderbridge.model.json.WireEnumAdapterFactory;
import java.io.IOException;

/**
 * JSON binding of the {@code sb_core::model} contract.
 *
 * <p>Field names follow serde's default (the Rust snake_case field names), which Gson derives from
 * the Java record components with {@link FieldNamingPolicy#LOWER_CASE_WITH_UNDERSCORES}. Enums with
 * data use {@link TaggedUnionAdapterFactory}, unit enums use {@link WireEnumAdapterFactory}. Nulls
 * are written explicitly so that {@code Option<T>} values and maps with optional values survive a
 * round trip.
 */
public final class ModelJson {
    private static final Gson GSON = new GsonBuilder()
        .setFieldNamingPolicy(FieldNamingPolicy.LOWER_CASE_WITH_UNDERSCORES)
        .serializeNulls()
        .enableComplexMapKeySerialization()
        .disableHtmlEscaping()
        .registerTypeAdapterFactory(new WireEnumAdapterFactory())
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(TargetSize.class))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.adjacentlyTagged(AxisSize.class, "value"))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(TextureSource.class))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(ImageSize.class))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(ResourceKind.class))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.adjacentlyTagged(ResourceRef.class, "value"))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.adjacentlyTagged(ScreenEntry.class, "value"))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.adjacentlyTagged(UniformSource.class, "name"))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(ProgramKind.class))
        .registerTypeAdapterFactory(TaggedUnionAdapterFactory.internallyTagged(WorkGroups.class))
        .registerTypeAdapterFactory(new DeviceCapsDefaults())
        .registerTypeAdapter(BlobId.class, new BlobIdAdapter().nullSafe())
        .registerTypeAdapter(IndirectDispatch.class, new IndirectDispatchAdapter().nullSafe())
        .create();

    private ModelJson() {
    }

    /**
     * Parses a value of the model.
     *
     * @param json JSON text produced by the native library
     * @param type the target type, e.g. {@code CompiledPack.class}
     * @param <T>  the target type
     * @return the parsed value, never null
     * @throws ModelParseException if the JSON is malformed or does not match the contract
     */
    public static <T> T parse(String json, Class<T> type) throws ModelParseException {
        return parse(json, TypeToken.get(type));
    }

    /**
     * Parses a value of a generic model type, e.g. a list.
     *
     * @param json JSON text produced by the native library
     * @param type the target type
     * @param <T>  the target type
     * @return the parsed value, never null
     * @throws ModelParseException if the JSON is malformed or does not match the contract
     */
    public static <T> T parse(String json, TypeToken<T> type) throws ModelParseException {
        String name = type.getType().getTypeName();
        if (json == null) {
            throw new ModelParseException("Cannot parse " + name, new JsonParseException("no JSON text"));
        }
        try {
            T value = GSON.fromJson(json, type);
            if (value == null) {
                throw new JsonParseException("empty document");
            }
            return value;
        } catch (RuntimeException e) {
            throw new ModelParseException("Cannot parse " + name, e);
        }
    }

    /**
     * @param value a model value
     * @return its JSON text in the contract format
     */
    public static String toJson(Object value) {
        return GSON.toJson(value);
    }

    /** {@code BlobId} is {@code #[serde(transparent)]}: a bare number. */
    private static final class BlobIdAdapter extends TypeAdapter<BlobId> {
        @Override
        public void write(JsonWriter out, BlobId value) throws IOException {
            out.value(value.index());
        }

        @Override
        public BlobId read(JsonReader in) throws IOException {
            return new BlobId(in.nextInt());
        }
    }

    /** {@code Option<(u32, u32)>} is a two-element array. */
    private static final class IndirectDispatchAdapter extends TypeAdapter<IndirectDispatch> {
        @Override
        public void write(JsonWriter out, IndirectDispatch value) throws IOException {
            out.beginArray().value(value.buffer()).value(value.offset()).endArray();
        }

        @Override
        public IndirectDispatch read(JsonReader in) throws IOException {
            in.beginArray();
            int buffer = in.nextInt();
            int offset = in.nextInt();
            in.endArray();
            return new IndirectDispatch(buffer, offset);
        }
    }

    /** Applies {@code #[serde(default = "default_true")]} of {@code DeviceCaps::comparison_samplers}. */
    private static final class DeviceCapsDefaults implements TypeAdapterFactory {
        @Override
        @SuppressWarnings("unchecked")
        public <T> TypeAdapter<T> create(Gson gson, TypeToken<T> type) {
            if (type.getRawType() != DeviceCaps.class) {
                return null;
            }
            TypeAdapter<DeviceCaps> delegate = gson.getDelegateAdapter(this, TypeToken.get(DeviceCaps.class));
            TypeAdapter<DeviceCaps> adapter = new TypeAdapter<>() {
                @Override
                public void write(JsonWriter out, DeviceCaps value) throws IOException {
                    delegate.write(out, value);
                }

                @Override
                public DeviceCaps read(JsonReader in) throws IOException {
                    JsonElement element = JsonParser.parseReader(in);
                    if (element.isJsonObject()) {
                        JsonObject object = element.getAsJsonObject();
                        if (!object.has("comparison_samplers")) {
                            object.addProperty("comparison_samplers", true);
                        }
                    }
                    return delegate.fromJsonTree(element);
                }
            };
            return (TypeAdapter<T>) adapter.nullSafe();
        }
    }
}
