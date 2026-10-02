package dev.shaderbridge.model;

import java.util.List;
import java.util.Map;

/**
 * Id maps from {@code block.properties}, {@code item.properties}, {@code entity.properties} and
 * {@code dimension.properties}. Entries are raw strings such as {@code minecraft:oak_leaves},
 * {@code stone}, {@code minecraft:wheat:age=7} or {@code %minecraft:logs} (a tag).
 *
 * @param blocks     block id to entries
 * @param items      item id to entries
 * @param entities   entity id to entries
 * @param layers     {@code layer.<solid|cutout|cutout_mipped|translucent>} overrides
 * @param dimensions dimension folder to dimension ids ({@code *} = wildcard)
 */
public record IdMaps(
    Map<Integer, List<String>> blocks,
    Map<Integer, List<String>> items,
    Map<Integer, List<String>> entities,
    Map<String, List<String>> layers,
    Map<String, List<String>> dimensions
) {
    public IdMaps {
        blocks = Copies.map(blocks);
        items = Copies.map(items);
        entities = Copies.map(entities);
        layers = Copies.map(layers);
        dimensions = Copies.map(dimensions);
    }
}
