package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

class PackPipelineCacheTest {
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);

    private static PackPipeline composite(SpirvModules modules) {
        Program composite = TUTORIAL.program("composite", "fullscreen");
        PackPipelineFactory factory = new PackPipelineFactory(TUTORIAL.pack().info().sourceHash(), modules, PipelineCapabilities.baseline(),
            DrawProfiles.get());
        PackPipelineFactory.Result result = factory.build(TUTORIAL.dim(), new ProgramVariant("", composite, TUTORIAL.blobs()),
            PipelineShape.fullscreen(), AttachmentLayout.fullscreen(TUTORIAL.dim(), composite), DepthMode.REVERSED_ZERO_TO_ONE);
        return ((PackPipelineFactory.Result.Built) result).pipeline();
    }

    @Test
    void compilesAsynchronouslyAndFinishesOnPoll() {
        SpirvModules modules = new SpirvModules();
        FakeDevice fake = new FakeDevice(modules, true);
        List<Runnable> renderThread = new ArrayList<>();
        PackPipelineCache cache = new PackPipelineCache(fake.device(), new PackShaderSource(modules), Runnable::run, renderThread::add, modules);
        PackPipeline pipeline = composite(modules);
        assertInstanceOf(PackPipelineCache.State.Compiling.class, cache.request(pipeline));
        assertEquals(2, modules.size());
        cache.poll();
        assertInstanceOf(PackPipelineCache.State.Compiling.class, cache.state(pipeline.key()), "not finished before the compile completes");
        fake.completeAll();
        cache.poll();
        PackPipelineCache.State.Ready ready = assertInstanceOf(PackPipelineCache.State.Ready.class, cache.state(pipeline.key()));
        assertSame(fake.compiled.getFirst(), ready.pipeline());
        assertEquals(0, modules.size(), "modules are released once compiled");
        assertFalse(cache.injectionInactive());
        // A second request for the same key reuses the entry and releases the duplicate's modules.
        PackPipeline duplicate = composite(modules);
        assertSame(cache.state(pipeline.key()), cache.request(duplicate));
        assertEquals(0, modules.size());
        cache.close();
        assertTrue(fake.compiled.getFirst().isClosed());
        assertThrows(IllegalStateException.class, () -> cache.request(composite(modules)));
    }

    @Test
    void anInactiveHookIsDetected() {
        SpirvModules modules = new SpirvModules();
        FakeDevice fake = new FakeDevice(modules, false);
        PackPipelineCache cache = new PackPipelineCache(fake.device(), new PackShaderSource(modules), Runnable::run, Runnable::run, modules);
        PackPipeline pipeline = composite(modules);
        cache.request(pipeline);
        fake.completeAll();
        cache.poll();
        PackPipelineCache.State.Failed failed = assertInstanceOf(PackPipelineCache.State.Failed.class, cache.state(pipeline.key()));
        assertTrue(failed.reason().contains("hook"), failed.reason());
        assertTrue(cache.injectionInactive());
    }

    @Test
    void compilesNotFinishedAtCloseAreClosedOnTheRenderThread() {
        SpirvModules modules = new SpirvModules();
        FakeDevice fake = new FakeDevice(modules, true);
        List<Runnable> renderThread = new ArrayList<>();
        PackPipelineCache cache = new PackPipelineCache(fake.device(), new PackShaderSource(modules), Runnable::run, renderThread::add, modules);
        cache.request(composite(modules));
        fake.completeAll();
        cache.close();
        assertEquals(0, modules.size());
        renderThread.forEach(Runnable::run);
        assertEquals(1, fake.compiled.size());
        assertTrue(fake.compiled.getFirst().isClosed());
    }
}
