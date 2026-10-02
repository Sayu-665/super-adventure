package dev.shaderbridge.model.json;

import com.google.gson.Gson;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import com.google.gson.JsonParser;
import com.google.gson.TypeAdapter;
import com.google.gson.TypeAdapterFactory;
import com.google.gson.reflect.TypeToken;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import com.google.gson.stream.JsonWriter;
import java.io.IOException;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.RecordComponent;
import java.util.HashMap;
import java.util.Map;

/**
 * Reads and writes a sealed interface whose permitted subclasses are records, using the serde
 * representations of Rust enums with data:
 *
 * <ul>
 *   <li><b>internally tagged</b> ({@code #[serde(tag = "type")]}): {@code {"type": "relative", "x": 1.0, "y": 1.0}}.
 *       Each variant record's components are the remaining fields.</li>
 *   <li><b>adjacently tagged</b> ({@code #[serde(tag = "type", content = "value")]}):
 *       {@code {"type": "color_tex", "value": 3}}. Each variant record has zero components (unit
 *       variant, no content field) or exactly one (the content).</li>
 * </ul>
 *
 * <p>The tag of a variant is its {@link Tag} annotation if present, otherwise the serde snake_case
 * form of its simple class name.
 */
public final class TaggedUnionAdapterFactory implements TypeAdapterFactory {
    private static final String TAG_FIELD = "type";

    private final Class<?> baseType;
    private final String contentField;
    private final Map<String, Variant> byTag = new HashMap<>();
    private final Map<Class<?>, Variant> byClass = new HashMap<>();

    private TaggedUnionAdapterFactory(Class<?> baseType, String contentField) {
        if (!baseType.isSealed()) {
            throw new IllegalArgumentException(baseType + " is not a sealed interface");
        }
        this.baseType = baseType;
        this.contentField = contentField;
        for (Class<?> sub : baseType.getPermittedSubclasses()) {
            Variant variant = Variant.of(sub, contentField != null);
            if (byTag.put(variant.tag, variant) != null) {
                throw new IllegalArgumentException("Duplicate tag '" + variant.tag + "' in " + baseType);
            }
            byClass.put(sub, variant);
        }
    }

    /**
     * @param sealedBase the sealed interface; its permitted subclasses must be records
     * @return a factory for the {@code #[serde(tag = "type")]} representation
     */
    public static TaggedUnionAdapterFactory internallyTagged(Class<?> sealedBase) {
        return new TaggedUnionAdapterFactory(sealedBase, null);
    }

    /**
     * @param sealedBase   the sealed interface; its permitted subclasses must be records with at most one component
     * @param contentField the serde {@code content} key, e.g. {@code "value"} or {@code "name"}
     * @return a factory for the {@code #[serde(tag = "type", content = ...)]} representation
     */
    public static TaggedUnionAdapterFactory adjacentlyTagged(Class<?> sealedBase, String contentField) {
        return new TaggedUnionAdapterFactory(sealedBase, contentField);
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> TypeAdapter<T> create(Gson gson, TypeToken<T> type) {
        if (type.getRawType() != baseType) {
            return null;
        }
        return (TypeAdapter<T>) new Adapter(gson).nullSafe();
    }

    private final class Adapter extends TypeAdapter<Object> {
        private final Gson gson;

        Adapter(Gson gson) {
            this.gson = gson;
        }

        @Override
        public void write(JsonWriter out, Object value) throws IOException {
            Variant variant = byClass.get(value.getClass());
            JsonObject object = new JsonObject();
            object.addProperty(TAG_FIELD, variant.tag);
            if (contentField == null) {
                JsonObject body = gson.getAdapter(variant.type).toJsonTree(cast(value)).getAsJsonObject();
                body.entrySet().forEach(e -> object.add(e.getKey(), e.getValue()));
            } else if (variant.component != null) {
                Object content = variant.content(value);
                JsonElement tree = content == null
                    ? JsonNull.INSTANCE
                    : gson.getAdapter(TypeToken.get(variant.component.getGenericType())).toJsonTree(cast(content));
                object.add(contentField, tree);
            }
            gson.getAdapter(JsonElement.class).write(out, object);
        }

        @Override
        public Object read(JsonReader in) throws IOException {
            if (in.peek() != JsonToken.BEGIN_OBJECT) {
                throw new JsonParseException("Expected a " + baseType.getSimpleName() + " object at " + in.getPath());
            }
            String path = in.getPath();
            JsonObject object = JsonParser.parseReader(in).getAsJsonObject();
            JsonElement tagElement = object.get(TAG_FIELD);
            if (tagElement == null || !tagElement.isJsonPrimitive()) {
                throw new JsonParseException("Missing \"type\" in " + baseType.getSimpleName() + " at " + path);
            }
            Variant variant = byTag.get(tagElement.getAsString());
            if (variant == null) {
                throw new JsonParseException("Unknown " + baseType.getSimpleName() + " type '" + tagElement.getAsString() + "' at " + path);
            }
            if (contentField == null) {
                JsonObject body = object.deepCopy();
                body.remove(TAG_FIELD);
                return gson.getAdapter(variant.type).fromJsonTree(body);
            }
            if (variant.component == null) {
                return variant.construct();
            }
            JsonElement content = object.get(contentField);
            if (content == null) {
                throw new JsonParseException("Missing \"" + contentField + "\" in " + baseType.getSimpleName() + " at " + path);
            }
            return variant.construct(gson.getAdapter(TypeToken.get(variant.component.getGenericType())).fromJsonTree(content));
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T cast(Object value) {
        return (T) value;
    }

    private record Variant(String tag, Class<?> type, RecordComponent component, Constructor<?> constructor) {
        static Variant of(Class<?> type, boolean adjacent) {
            if (!type.isRecord()) {
                throw new IllegalArgumentException(type + " must be a record");
            }
            RecordComponent[] components = type.getRecordComponents();
            if (adjacent && components.length > 1) {
                throw new IllegalArgumentException(type + " has more than one component");
            }
            Tag explicit = type.getAnnotation(Tag.class);
            String tag = explicit != null ? explicit.value() : SerdeNames.snakeCase(type.getSimpleName());
            Class<?>[] parameterTypes = new Class<?>[components.length];
            for (int i = 0; i < components.length; i++) {
                parameterTypes[i] = components[i].getType();
            }
            try {
                Constructor<?> constructor = type.getDeclaredConstructor(parameterTypes);
                return new Variant(tag, type, components.length == 1 ? components[0] : null, constructor);
            } catch (NoSuchMethodException e) {
                throw new IllegalArgumentException("No canonical constructor in " + type, e);
            }
        }

        Object construct(Object... args) {
            try {
                return constructor.newInstance(args);
            } catch (InvocationTargetException e) {
                throw new JsonParseException("Invalid " + type.getSimpleName() + ": " + e.getCause().getMessage(), e.getCause());
            } catch (ReflectiveOperationException e) {
                throw new JsonParseException("Cannot construct " + type.getSimpleName(), e);
            }
        }

        Object content(Object value) {
            try {
                return component.getAccessor().invoke(value);
            } catch (ReflectiveOperationException e) {
                throw new IllegalStateException("Cannot read " + component, e);
            }
        }
    }
}
