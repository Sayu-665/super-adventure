package dev.shaderbridge.gui;

import java.util.List;
import java.util.function.Function;
import net.minecraft.client.gui.components.AbstractSliderButton;
import net.minecraft.network.chat.Component;

/** A slider over an option's allowed values ({@code sliders=} in {@code shaders.properties}). */
final class OptionSlider extends AbstractSliderButton {
    private final OptionsEditor editor;
    private final String option;
    private final List<String> values;
    private final Function<String, Component> label;
    private final Runnable onChange;

    OptionSlider(int width, OptionsEditor editor, String option, Function<String, Component> label, Runnable onChange) {
        super(0, 0, width, 20, Component.empty(), 0);
        this.editor = editor;
        this.option = option;
        this.values = OptionsEditor.allowedValues(editor.option(option).orElseThrow());
        this.label = label;
        this.onChange = onChange;
        refresh();
    }

    /** Moves the handle to the pending value after a change elsewhere on the screen. */
    void refresh() {
        int index = Math.max(0, values.indexOf(editor.value(option)));
        this.value = values.size() > 1 ? (double) index / (values.size() - 1) : 0;
        updateMessage();
    }

    private int index() {
        return (int) Math.round(value * (values.size() - 1));
    }

    @Override
    protected void updateMessage() {
        setMessage(label.apply(values.get(index())));
    }

    @Override
    protected void applyValue() {
        String selected = values.get(index());
        if (!selected.equals(editor.value(option))) {
            editor.set(option, selected);
            onChange.run();
        }
    }
}
