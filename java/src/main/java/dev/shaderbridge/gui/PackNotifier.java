package dev.shaderbridge.gui;

import dev.shaderbridge.pack.DiagnosticSummary;
import dev.shaderbridge.pack.LoadedPack;
import net.minecraft.ChatFormatting;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.components.toasts.SystemToast;
import net.minecraft.network.chat.Component;

/**
 * Short compile reports for the player: in chat while in a world (if enabled), otherwise as a
 * toast. Details always go to the log.
 */
public final class PackNotifier {
    private static final SystemToast.SystemToastId TOAST = new SystemToast.SystemToastId();

    private PackNotifier() {
    }

    /**
     * @param pack   the compiled pack
     * @param inChat report in chat when a world is loaded
     */
    public static void compiled(LoadedPack pack, boolean inChat) {
        DiagnosticSummary summary = pack.summary();
        ChatFormatting color = summary.errors() > 0 ? ChatFormatting.RED : summary.warnings() > 0 ? ChatFormatting.YELLOW : ChatFormatting.GREEN;
        Component counts = Component.literal(summary.describe()).withStyle(color);
        show(Component.translatable("shaderbridge.message.compiled", pack.name(), counts), inChat);
    }

    /**
     * @param pack   the pack that failed
     * @param reason why
     */
    public static void error(String pack, String reason) {
        show(Component.translatable("shaderbridge.message.failed", pack, Component.literal(reason).withStyle(ChatFormatting.RED)), true);
    }

    /**
     * @param enabled the new state of shader packs
     */
    public static void toggled(boolean enabled) {
        show(Component.translatable(enabled ? "shaderbridge.message.enabled" : "shaderbridge.message.disabled"), true);
    }

    private static void show(Component message, boolean inChat) {
        Minecraft mc = Minecraft.getInstance();
        if (inChat && mc.player != null) {
            mc.gui.hud.getChat().addClientSystemMessage(Component.literal("[ShaderBridge] ").withStyle(ChatFormatting.AQUA).append(message));
        } else {
            SystemToast.addOrUpdate(mc.gui.toastManager(), TOAST, Component.literal("ShaderBridge"), message);
        }
    }
}
