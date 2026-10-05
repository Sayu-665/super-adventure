package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.device.GpuDevice;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.StorageBuffer;
import dev.shaderbridge.model.TextureSource;
import dev.shaderbridge.render.pipeline.PipelineCapabilities;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.RawPath;
import dev.shaderbridge.render.targets.CustomTextureIds;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.stream.Stream;

/**
 * Entry point of the raw Vulkan path for the render integration: what Minecraft's device lets
 * pack pipelines do, the storage usage of render targets, and the raw path of each dimension
 * pipeline. On the Vulkan backend these come from Minecraft's {@code VulkanDevice}; on the OpenGL
 * backend the raw path declines every program (they are skipped or fall back along the pack's
 * program chain, with a message), while pipelines may mask and blend per attachment, which
 * Mojang's GL backend always does ({@code glColorMaski}, {@code glEnablei}).
 */
public sealed interface RawBackend {
    /**
     * @param gpu Minecraft's GPU device
     * @return its raw backend
     */
    static RawBackend of(GpuDevice gpu) {
        return VulkanContext.of(gpu).<RawBackend>map(Vulkan::new).orElseGet(() -> new Unavailable(
            "the raw Vulkan path needs Minecraft's Vulkan backend (" + gpu.getDeviceInfo().backendName() + " is active)",
            new PipelineCapabilities(true, PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS)));
    }

    /** @return what renderpearl pipelines may do on the device */
    PipelineCapabilities capabilities();

    /**
     * Requests the storage usage for the render targets a dimension pipeline binds as storage
     * images. Call before the targets are created; close when they are gone.
     *
     * @param dim the dimension pipeline
     * @return the request
     */
    AutoCloseable requestStorage(DimensionPipeline dim);

    /**
     * @param dim a dimension pipeline
     * @return the pack files the raw path of the dimension pipeline reads, by path relative to
     *     {@code shaders/}: storage buffer initial data and the raw textures it provides
     */
    List<String> packFiles(DimensionPipeline dim);

    /**
     * @param context   what the raw path works with
     * @param blobs     the SPIR-V of the dimension's programs (storage buffer block sizes)
     * @param packFiles the contents of the {@link #packFiles} that exist, by path
     * @return the raw path of the dimension pipeline; close it with the pack's resources
     */
    DimensionRawPath open(RawContext context, Blobs blobs, Map<String, byte[]> packFiles);

    /** A raw path that is closed with the resources of its dimension pipeline. */
    interface DimensionRawPath extends RawPath, AutoCloseable {
        @Override
        void close();
    }

    /**
     * Minecraft's Vulkan device.
     *
     * @param ctx the device
     */
    record Vulkan(VulkanContext ctx) implements RawBackend {
        @Override
        public PipelineCapabilities capabilities() {
            return ctx.features().capabilities();
        }

        @Override
        public AutoCloseable requestStorage(DimensionPipeline dim) {
            return StorageUsage.request(StorageTargets.labels(dim));
        }

        @Override
        public List<String> packFiles(DimensionPipeline dim) {
            Stream<String> buffers = dim.targets().buffers().stream().map(StorageBuffer::file).filter(Objects::nonNull);
            Stream<String> textures = dim.targets().customTextures().stream().filter(RawTextureData::needed)
                .map(t -> ((TextureSource.Raw) t.source()).path());
            return Stream.concat(buffers, textures).distinct().toList();
        }

        @Override
        public DimensionRawPath open(RawContext context, Blobs blobs, Map<String, byte[]> packFiles) {
            Map<Integer, byte[]> initial = new HashMap<>();
            for (StorageBuffer buffer : context.dim().targets().buffers()) {
                if (buffer.file() != null) {
                    byte[] data = packFiles.get(buffer.file());
                    if (data == null) {
                        context.diagnostics().report("storage buffer " + buffer.index() + ": its initial data " + buffer.file() + " is missing");
                    } else {
                        initial.put(buffer.index(), data);
                    }
                }
            }
            RawResources resources = new RawResources(ctx, context.dim(), StorageBlocks.declared(context.dim(), blobs), initial,
                textures(context, packFiles), context.diagnostics()::report);
            return new VulkanRawPath(ctx, context, resources);
        }

        /** Converts the raw textures the raw path provides, reporting those it cannot. */
        private static List<RawTextureData.Upload> textures(RawContext context, Map<String, byte[]> packFiles) {
            List<RawTextureData.Upload> uploads = new ArrayList<>();
            for (CustomTexture texture : context.dim().targets().customTextures()) {
                if (!RawTextureData.needed(texture)) {
                    continue;
                }
                String path = ((TextureSource.Raw) texture.source()).path();
                byte[] data = packFiles.get(path);
                String name = "custom texture " + CustomTextureIds.id(texture);
                switch (data == null ? new RawTextureData.Result.Unsupported(path + " is missing") : RawTextureData.convert(texture, data)) {
                    case RawTextureData.Result.Converted c -> uploads.add(c.upload());
                    case RawTextureData.Result.Unsupported u -> context.diagnostics().report(name + " is not provided: " + u.reason());
                }
            }
            return uploads;
        }
    }

    /**
     * No raw path (another backend).
     *
     * @param reason       why, for the messages of declined programs
     * @param capabilities what the backend's pipelines may do
     */
    record Unavailable(String reason, PipelineCapabilities capabilities) implements RawBackend {
        @Override
        public AutoCloseable requestStorage(DimensionPipeline dim) {
            return () -> {
            };
        }

        @Override
        public List<String> packFiles(DimensionPipeline dim) {
            return List.of();
        }

        @Override
        public DimensionRawPath open(RawContext context, Blobs blobs, Map<String, byte[]> packFiles) {
            return new Declining(reason);
        }
    }

    /**
     * Declines every program.
     *
     * @param reason why
     */
    record Declining(String reason) implements DimensionRawPath {
        @Override
        public Admission admit(DimensionPipeline dim, ProgramVariant program, List<String> renderpearlProblems) {
            return new Admission.Rejected(reason);
        }

        @Override
        public void close() {
            // Nothing was created.
        }
    }
}
