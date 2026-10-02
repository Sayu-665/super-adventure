package dev.shaderbridge.model.json;

import com.google.gson.FieldNamingStrategy;
import com.google.gson.Gson;
import com.google.gson.JsonElement;
import com.google.gson.TypeAdapter;
import com.google.gson.TypeAdapterFactory;
import com.google.gson.annotations.SerializedName;
import com.google.gson.reflect.TypeToken;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonWriter;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.RecordComponent;
import java.util.ArrayList;
import java.util.List;

/**
 * Writes records without the {@link OmitIfNull} components that are null, reproducing serde's
 * {@code skip_serializing_if = "Option::is_none"}. Reading is unchanged: a missing field is null.
 */
public final class OmitIfNullAdapterFactory implements TypeAdapterFactory {
    private final FieldNamingStrategy naming;

    /** @param naming the field naming strategy of the Gson instance, to find the JSON names */
    public OmitIfNullAdapterFactory(FieldNamingStrategy naming) {
        this.naming = naming;
    }

    @Override
    public <T> TypeAdapter<T> create(Gson gson, TypeToken<T> type) {
        Class<? super T> raw = type.getRawType();
        if (!raw.isRecord()) {
            return null;
        }
        List<String> omitted = omittedNames(raw);
        if (omitted.isEmpty()) {
            return null;
        }
        TypeAdapter<T> delegate = gson.getDelegateAdapter(this, type);
        TypeAdapter<JsonElement> elements = gson.getAdapter(JsonElement.class);
        return new TypeAdapter<T>() {
            @Override
            public void write(JsonWriter out, T value) throws IOException {
                JsonElement tree = delegate.toJsonTree(value);
                if (tree.isJsonObject()) {
                    for (String name : omitted) {
                        JsonElement field = tree.getAsJsonObject().get(name);
                        if (field != null && field.isJsonNull()) {
                            tree.getAsJsonObject().remove(name);
                        }
                    }
                }
                elements.write(out, tree);
            }

            @Override
            public T read(JsonReader in) throws IOException {
                return delegate.read(in);
            }
        };
    }

    private List<String> omittedNames(Class<?> record) {
        List<String> names = new ArrayList<>();
        for (RecordComponent component : record.getRecordComponents()) {
            if (!component.isAnnotationPresent(OmitIfNull.class)) {
                continue;
            }
            try {
                Field field = record.getDeclaredField(component.getName());
                SerializedName explicit = field.getAnnotation(SerializedName.class);
                names.add(explicit != null ? explicit.value() : naming.translateName(field));
            } catch (NoSuchFieldException e) {
                throw new IllegalStateException("record component without a field: " + component, e);
            }
        }
        return names;
    }
}
