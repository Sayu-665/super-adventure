package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link PassRedirect}: render pass creation is handed to ShaderBridge only while armed, never for its own passes. */
class PassRedirectTest {
    private static RenderPass pass() {
        return (RenderPass) Proxy.newProxyInstance(RenderPass.class.getClassLoader(), new Class<?>[] {RenderPass.class}, (p, m, a) -> {
            throw new UnsupportedOperationException(m.getName());
        });
    }

    private static RenderPassDescriptor descriptor() {
        return RenderPassDescriptor.builder(() -> "requested").build();
    }

    @Test
    void passesAreRedirectedOnlyWhileArmed() {
        assertFalse(PassRedirect.armed());
        assertNull(PassRedirect.redirect(descriptor()));
        List<RenderPass> created = new ArrayList<>();
        List<RenderPass> nested = new ArrayList<>();
        PassRedirect.Armed armed = PassRedirect.arm(requested -> {
            // The factory's own pass creation goes to Mojang's encoder.
            nested.add(PassRedirect.redirect(requested));
            RenderPass pass = pass();
            ActivePasses.open(pass, r -> ActivePasses.Substitution.skip());
            created.add(pass);
            return pass;
        });
        try {
            assertTrue(PassRedirect.armed());
            assertThrows(IllegalStateException.class, () -> PassRedirect.arm(r -> pass()), "one redirection at a time");
            RenderPass first = PassRedirect.redirect(descriptor());
            RenderPass second = PassRedirect.redirect(descriptor());
            assertEquals(List.of(first, second), created, "one pass of ours per requested pass");
            assertEquals(2, nested.size());
            assertNull(nested.getFirst());
            assertTrue(ActivePasses.owns(second));
        } finally {
            armed.close();
        }
        assertFalse(PassRedirect.armed());
        assertFalse(ActivePasses.owns(created.get(1)), "closing the redirection forgets its last pass");
        assertNull(PassRedirect.redirect(descriptor()));
        RenderPass ours = pass();
        PassRedirect.Armed again = PassRedirect.arm(r -> ours);
        try {
            assertSame(ours, PassRedirect.redirect(descriptor()), "it can be armed again");
        } finally {
            again.close();
        }
    }
}
