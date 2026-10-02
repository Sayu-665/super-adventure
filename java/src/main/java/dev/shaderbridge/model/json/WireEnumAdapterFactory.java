package dev.shaderbridge.model.json;

import com.google.gson.Gson;
import com.google.gson.JsonParseException;
import com.google.gson.TypeAdapter;
import com.google.gson.TypeAdapterFactory;
import com.google.gson.reflect.TypeToken;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import com.google.gson.stream.JsonWriter;
import java.io.IOException;
import java.util.HashMap;
import java.util.Map;

/**
 * Serializes every {@link WireEnum} by its {@link WireEnum#wireName()}. Unknown strings are
 * rejected, because the model is a versioned contract and silently mapping an unknown value would
 * hide a mismatch between the native library and the mod. Also used for map keys, since Gson
 * reads map keys through the key type's adapter.
 */
public final class WireEnumAdapterFactory implements TypeAdapterFactory {
    @Override
    @SuppressWarnings({"unchecked", "rawtypes"})
    public <T> TypeAdapter<T> create(Gson gson, TypeToken<T> type) {
        Class<? super T> raw = type.getRawType();
        if (!raw.isEnum() || !WireEnum.class.isAssignableFrom(raw)) {
            return null;
        }
        return (TypeAdapter<T>) new Adapter(raw);
    }

    private static final class Adapter<E extends Enum<E> & WireEnum> extends TypeAdapter<E> {
        private final Class<E> type;
        private final Map<String, E> byName = new HashMap<>();

        Adapter(Class<E> type) {
            this.type = type;
            for (E constant : type.getEnumConstants()) {
                byName.put(constant.wireName(), constant);
            }
        }

        @Override
        public void write(JsonWriter out, E value) throws IOException {
            if (value == null) {
                out.nullValue();
            } else {
                out.value(value.wireName());
            }
        }

        @Override
        public E read(JsonReader in) throws IOException {
            if (in.peek() == JsonToken.NULL) {
                in.nextNull();
                return null;
            }
            String name = in.nextString();
            E constant = byName.get(name);
            if (constant == null) {
                throw new JsonParseException("Unknown " + type.getSimpleName() + " value '" + name + "' at " + in.getPath());
            }
            return constant;
        }
    }
}
