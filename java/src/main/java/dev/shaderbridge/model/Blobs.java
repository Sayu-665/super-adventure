package dev.shaderbridge.model;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.List;

/**
 * The binary payloads of a {@link CompiledPack}: one concatenated buffer plus the
 * {@link CompiledPack#blobs()} index. Instances are immutable and safe to share between threads;
 * every accessor returns an independent read-only view.
 */
public final class Blobs {
    private final ByteBuffer buffer;
    private final List<BlobInfo> index;

    private Blobs(ByteBuffer buffer, List<BlobInfo> index) {
        this.buffer = buffer;
        this.index = index;
    }

    /**
     * Wraps a blob buffer after checking that every indexed blob lies inside it.
     *
     * @param buffer the concatenated blob buffer (position 0, limit = size); not copied
     * @param index  the blob index of the compiled pack
     * @return the blob table
     * @throws IllegalArgumentException if a blob lies outside the buffer
     */
    public static Blobs of(ByteBuffer buffer, List<BlobInfo> index) {
        ByteBuffer view = buffer.slice().asReadOnlyBuffer().order(ByteOrder.LITTLE_ENDIAN);
        for (int i = 0; i < index.size(); i++) {
            BlobInfo info = index.get(i);
            if (info.offset() + info.len() > view.capacity() || info.offset() + info.len() < 0) {
                throw new IllegalArgumentException(
                    "Blob " + i + " (" + info.offset() + "+" + info.len() + ") lies outside the " + view.capacity() + "-byte blob buffer");
            }
        }
        return new Blobs(view, List.copyOf(index));
    }

    /** @return an empty table, for packs compiled without payloads */
    public static Blobs empty() {
        return new Blobs(ByteBuffer.allocateDirect(0).order(ByteOrder.LITTLE_ENDIAN), List.of());
    }

    /** @return the number of blobs */
    public int count() {
        return index.size();
    }

    /** @return the size of the concatenated buffer in bytes */
    public long byteSize() {
        return buffer.capacity();
    }

    /**
     * @param id a blob id
     * @return the index entry of the blob
     * @throws IllegalArgumentException if the id is out of range
     */
    public BlobInfo info(BlobId id) {
        if (id.index() >= index.size()) {
            throw new IllegalArgumentException("Unknown blob " + id.index() + " (" + index.size() + " blobs)");
        }
        return index.get(id.index());
    }

    /**
     * @param id a blob id
     * @return a read-only little-endian view of the blob's bytes
     */
    public ByteBuffer bytes(BlobId id) {
        BlobInfo info = info(id);
        return buffer.slice((int) info.offset(), (int) info.len()).order(ByteOrder.LITTLE_ENDIAN);
    }

    /**
     * @param id id of a {@link BlobKind#SPIRV} blob
     * @return a read-only little-endian view of the SPIR-V words
     * @throws IllegalArgumentException if the blob is not SPIR-V or not a whole number of words
     */
    public ByteBuffer spirv(BlobId id) {
        BlobInfo info = expect(id, BlobKind.SPIRV);
        if (info.len() % 4 != 0) {
            throw new IllegalArgumentException("SPIR-V blob " + id.index() + " has " + info.len() + " bytes, not a multiple of 4");
        }
        return bytes(id);
    }

    /**
     * @param id id of a {@link BlobKind#GLSL} blob
     * @return the decoded source
     * @throws IllegalArgumentException if the blob is not GLSL or not valid UTF-8
     */
    public String glsl(BlobId id) {
        expect(id, BlobKind.GLSL);
        try {
            return StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(bytes(id))
                .toString();
        } catch (CharacterCodingException e) {
            throw new IllegalArgumentException("GLSL blob " + id.index() + " is not valid UTF-8", e);
        }
    }

    private BlobInfo expect(BlobId id, BlobKind kind) {
        BlobInfo info = info(id);
        if (info.kind() != kind) {
            throw new IllegalArgumentException("Blob " + id.index() + " is " + info.kind().wireName() + ", expected " + kind.wireName());
        }
        return info;
    }
}
