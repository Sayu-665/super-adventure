package dev.shaderbridge.gui;

import com.mojang.blaze3d.platform.InputConstants;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.AbstractButton;
import net.minecraft.client.gui.narration.NarrationElementOutput;
import net.minecraft.client.input.InputWithModifiers;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.client.input.MouseButtonInfo;
import net.minecraft.network.chat.Component;

/**
 * A button that steps through values the OptiFine way: left click (or Enter) selects the next
 * value, right click the previous one, and shift-click resets to the default.
 */
final class CyclingButton extends AbstractButton {
    /** What the button changes. */
    interface Behavior {
        /** @param steps +1 for the next value, -1 for the previous one */
        void cycle(int steps);

        /** Restores the default value. */
        void reset();

        /** @return the current label */
        Component message();
    }

    private final Behavior behavior;
    private final Runnable onChange;

    CyclingButton(int width, Behavior behavior, Runnable onChange) {
        super(0, 0, width, 20, behavior.message());
        this.behavior = behavior;
        this.onChange = onChange;
    }

    /** Re-reads the label after a change elsewhere on the screen. */
    void refresh() {
        setMessage(behavior.message());
    }

    @Override
    protected boolean isValidClickButton(MouseButtonInfo button) {
        return button.button() == InputConstants.MOUSE_BUTTON_LEFT || button.button() == InputConstants.MOUSE_BUTTON_RIGHT;
    }

    @Override
    public void onClick(MouseButtonEvent event, boolean doubleClick) {
        if (event.hasShiftDown()) {
            behavior.reset();
        } else {
            behavior.cycle(event.button() == InputConstants.MOUSE_BUTTON_RIGHT ? -1 : 1);
        }
        onChange.run();
    }

    @Override
    public void onPress(InputWithModifiers input) {
        if (input.hasShiftDown()) {
            behavior.reset();
        } else {
            behavior.cycle(1);
        }
        onChange.run();
    }

    @Override
    protected void extractContents(GuiGraphicsExtractor graphics, int mouseX, int mouseY, float a) {
        extractDefaultSprite(graphics);
        extractDefaultLabel(graphics.textRendererForWidget(this, GuiGraphicsExtractor.HoveredTextEffects.NONE));
    }

    @Override
    protected void updateWidgetNarration(NarrationElementOutput output) {
        defaultButtonNarrationText(output);
    }
}
