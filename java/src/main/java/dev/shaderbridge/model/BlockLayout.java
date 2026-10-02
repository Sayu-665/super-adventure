package dev.shaderbridge.model;

import java.util.List;
import java.util.Optional;

/**
 * std140 layout of one uniform block.
 *
 * @param name    GLSL block name
 * @param set     descriptor set
 * @param binding binding within the set
 * @param size    total std140 size in bytes (a multiple of 16)
 * @param members members sorted by offset
 */
public record BlockLayout(String name, int set, int binding, int size, List<BlockMember> members) {
    public BlockLayout {
        members = Copies.list(members);
        if (size < 0) {
            throw new IllegalArgumentException("negative block size " + size);
        }
    }

    /**
     * @param memberName GLSL member name
     * @return the member, if the block has it
     */
    public Optional<BlockMember> member(String memberName) {
        return members.stream().filter(m -> m.name().equals(memberName)).findFirst();
    }
}
