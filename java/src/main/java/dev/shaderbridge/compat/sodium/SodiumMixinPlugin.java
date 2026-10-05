package dev.shaderbridge.compat.sodium;

import java.io.IOException;
import org.objectweb.asm.tree.ClassNode;
import org.spongepowered.asm.mixin.extensibility.IMixinConfigPlugin;
import org.spongepowered.asm.service.MixinService;

/**
 * The configuration plugin of {@code shaderbridge-sodium.mixins.json} (outside the mixin packages,
 * which Mixin forbids loading classes from). It applies the configuration's mixins
 * ({@code dev.shaderbridge.mixin.sodium}) all together or not at all: only
 * when Sodium is installed and every member they hook or call exists in its class files
 * ({@link SodiumTargets}, read without loading any class). The decision is made once, when Mixin
 * loads the configuration, and published through {@link SodiumIntegration}; with another Sodium
 * version that moved something, Sodium is left untouched and ShaderBridge refuses shader packs
 * with a message.
 */
public final class SodiumMixinPlugin implements IMixinConfigPlugin {
    private boolean apply;

    @Override
    public void onLoad(String mixinPackage) {
        apply = SodiumIntegration.decide(SodiumCompat.installedVersion(), SodiumMixinPlugin::readClass);
    }

    @Override
    public boolean shouldApplyMixin(String targetClassName, String mixinClassName) {
        return apply;
    }

    /** Reads a class file through Mixin's bytecode provider, without running transformers. */
    private static ClassNode readClass(String internalName) {
        try {
            return MixinService.getService().getBytecodeProvider().getClassNode(internalName.replace('/', '.'), false);
        } catch (ClassNotFoundException | IOException e) {
            return null;
        }
    }
}
