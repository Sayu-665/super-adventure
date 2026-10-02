package dev.shaderbridge.gui;

import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.model.OptionScreen;
import dev.shaderbridge.model.PackOption;
import dev.shaderbridge.model.ScreenEntry;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.ChatFormatting;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.AbstractWidget;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.ContainerObjectSelectionList;
import net.minecraft.client.gui.components.Tooltip;
import net.minecraft.client.gui.components.events.GuiEventListener;
import net.minecraft.client.gui.layouts.HeaderAndFooterLayout;
import net.minecraft.client.gui.layouts.LinearLayout;
import net.minecraft.client.gui.narration.NarratableEntry;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.CommonComponents;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;

/**
 * One page of a pack's options, built from its {@code OptionsModel}: the main screen or a
 * {@code screen.<NAME>} sub-screen, laid out in the pack's column count. Booleans and value
 * options are cycling buttons, options listed in {@code sliders} are sliders, {@code [NAME]}
 * entries open sub-screens, {@code <profile>} cycles profiles, {@code <empty>} leaves a gap and
 * {@code *} lists every option placed nowhere else. The main screen saves the pending values to
 * {@code shaderpacks/<pack>.txt} and recompiles the pack.
 */
public final class PackOptionsScreen extends Screen {
    private static final int DEFAULT_COLUMNS = 2;
    private static final int ROW_HEIGHT = 24;
    private static final int GAP = 6;

    private final Screen parent;
    private final String pack;
    private final OptionsEditor editor;
    private final List<ScreenEntry> entries;
    private final int columns;
    private final boolean root;
    private final HeaderAndFooterLayout layout = new HeaderAndFooterLayout(this);
    private final List<Runnable> refreshers = new ArrayList<>();
    private OptionGrid grid;

    private PackOptionsScreen(Screen parent, String pack, OptionsEditor editor, Component title, List<ScreenEntry> entries, Integer columns, boolean root) {
        super(title);
        this.parent = parent;
        this.pack = pack;
        this.editor = editor;
        this.entries = editor.expand(entries.isEmpty() ? List.of(new ScreenEntry.Rest()) : entries);
        this.columns = columns == null || columns < 1 ? DEFAULT_COLUMNS : columns;
        this.root = root;
    }

    /**
     * The main options screen of a pack.
     *
     * @param parent screen to return to
     * @param pack   pack file name
     * @param editor the pack's options and the user's values
     * @return the screen
     */
    public static PackOptionsScreen main(Screen parent, String pack, OptionsEditor editor) {
        return new PackOptionsScreen(parent, pack, editor, Component.translatable("shaderbridge.screen.options.title", pack),
            editor.model().mainScreen(), editor.model().mainScreenColumns(), true);
    }

    @Override
    protected void init() {
        layout.addTitleHeader(title, font);
        refreshers.clear();
        grid = layout.addToContents(new OptionGrid());
        List<AbstractWidget> row = new ArrayList<>();
        for (ScreenEntry entry : entries) {
            row.add(widget(entry));
            if (row.size() == columns) {
                grid.addRow(row);
                row = new ArrayList<>();
            }
        }
        if (!row.isEmpty()) {
            grid.addRow(row);
        }
        LinearLayout footer = layout.addToFooter(LinearLayout.horizontal().spacing(8));
        if (root) {
            footer.addChild(Button.builder(CommonComponents.GUI_DONE, b -> save()).width(100).build());
            footer.addChild(Button.builder(Component.translatable("shaderbridge.screen.options.reset"), b -> {
                editor.resetAll();
                refreshAll();
            }).width(100).build());
            footer.addChild(Button.builder(CommonComponents.GUI_CANCEL, b -> minecraft.gui.setScreen(parent)).width(100).build());
        } else {
            footer.addChild(Button.builder(CommonComponents.GUI_BACK, b -> onClose()).width(200).build());
        }
        layout.visitWidgets(this::addRenderableWidget);
        repositionElements();
    }

    @Override
    protected void repositionElements() {
        layout.arrangeElements();
        if (grid != null) {
            grid.updateSize(width, layout);
        }
        // Values may have changed on a sub-screen.
        refreshAll();
    }

    @Override
    public void onClose() {
        minecraft.gui.setScreen(parent);
    }

    private void save() {
        ShaderBridge.get().saveOptionValues(pack, editor.changedValues());
        minecraft.gui.setScreen(parent);
    }

    private void refreshAll() {
        refreshers.forEach(Runnable::run);
    }

    private int cellWidth() {
        int rowWidth = Math.min(width - 40, columns * 160 - 10);
        return Math.max(40, (rowWidth - (columns - 1) * GAP) / columns);
    }

    /** @return the widget of an entry, or null for a gap */
    private AbstractWidget widget(ScreenEntry entry) {
        return switch (entry) {
            case ScreenEntry.OptionEntry option -> editor.option(option.name()).map(this::optionWidget).orElse(null);
            case ScreenEntry.ScreenLink link -> linkButton(link.screen());
            case ScreenEntry.ProfileSelector _ -> profileButton();
            case ScreenEntry.Empty _, ScreenEntry.Rest _ -> null;
        };
    }

    private AbstractWidget optionWidget(PackOption option) {
        String name = option.name();
        AbstractWidget widget;
        if (!option.kind().isBoolean() && editor.model().sliders().contains(name)) {
            OptionSlider slider = new OptionSlider(cellWidth(), editor, name, value -> label(option, value), this::refreshAll);
            refreshers.add(slider::refresh);
            widget = slider;
        } else {
            CyclingButton button = new CyclingButton(cellWidth(), new CyclingButton.Behavior() {
                @Override
                public void cycle(int steps) {
                    editor.cycle(name, steps);
                }

                @Override
                public void reset() {
                    editor.reset(name);
                }

                @Override
                public Component message() {
                    return label(option, editor.value(name));
                }
            }, this::refreshAll);
            refreshers.add(button::refresh);
            widget = button;
        }
        editor.lang().comment(option).ifPresent(comment -> widget.setTooltip(Tooltip.create(Component.literal(comment))));
        return widget;
    }

    /** {@code Name: value}, the name highlighted while the value differs from the default. */
    private Component label(PackOption option, String value) {
        MutableComponent name = Component.literal(editor.lang().optionName(option.name()));
        if (editor.isChanged(option.name())) {
            name.withStyle(ChatFormatting.YELLOW);
        }
        Component shown = option.kind().isBoolean()
            ? CommonComponents.optionStatus("true".equals(value))
            : Component.literal(editor.lang().value(option.name(), value));
        return CommonComponents.optionNameValue(name, shown);
    }

    private AbstractWidget linkButton(String screen) {
        Button button = Button.builder(Component.literal(editor.lang().screenName(screen) + "..."), b -> {
            OptionScreen target = editor.model().screens().get(screen);
            List<ScreenEntry> targetEntries = target != null ? target.entries() : List.of();
            Integer targetColumns = target != null ? target.columns() : null;
            minecraft.gui.setScreen(new PackOptionsScreen(this, pack, editor, Component.literal(editor.lang().screenName(screen)), targetEntries, targetColumns, false));
        }).width(cellWidth()).build();
        editor.lang().screenComment(screen).ifPresent(comment -> button.setTooltip(Tooltip.create(Component.literal(comment))));
        return button;
    }

    private AbstractWidget profileButton() {
        CyclingButton button = new CyclingButton(cellWidth(), new CyclingButton.Behavior() {
            @Override
            public void cycle(int steps) {
                editor.cycleProfile(steps);
            }

            @Override
            public void reset() {
                // Resetting every option is the explicit "Reset" button; a profile has no default.
            }

            @Override
            public Component message() {
                Optional<String> profile = editor.currentProfile();
                Component value = profile.<Component>map(p -> Component.literal(editor.lang().profileName(p)))
                    .orElse(Component.translatable("shaderbridge.screen.options.profile.custom"));
                return CommonComponents.optionNameValue(Component.translatable("shaderbridge.screen.options.profile"), value);
            }
        }, this::refreshAll);
        editor.lang().profileComment().ifPresent(comment -> button.setTooltip(Tooltip.create(Component.literal(comment))));
        refreshers.add(button::refresh);
        return button;
    }

    /** Rows of up to {@code columns} widgets. */
    private final class OptionGrid extends ContainerObjectSelectionList<OptionGrid.Row> {
        OptionGrid() {
            super(PackOptionsScreen.this.minecraft, PackOptionsScreen.this.width, layout.getContentHeight(), layout.getHeaderHeight(), ROW_HEIGHT);
            this.centerListVertically = false;
        }

        void addRow(List<AbstractWidget> cells) {
            addEntry(new Row(cells));
        }

        @Override
        public int getRowWidth() {
            return columns * cellWidth() + (columns - 1) * GAP;
        }

        /** One row; null cells are gaps. */
        final class Row extends ContainerObjectSelectionList.Entry<Row> {
            private final List<AbstractWidget> cells;
            private final List<AbstractWidget> widgets;

            Row(List<AbstractWidget> cells) {
                this.cells = cells;
                this.widgets = cells.stream().filter(w -> w != null).toList();
            }

            @Override
            public void extractContent(GuiGraphicsExtractor graphics, int mouseX, int mouseY, boolean hovered, float a) {
                int cell = cellWidth();
                for (int i = 0; i < cells.size(); i++) {
                    AbstractWidget widget = cells.get(i);
                    if (widget != null) {
                        widget.setWidth(cell);
                        widget.setPosition(getContentX() + i * (cell + GAP), getContentY());
                        widget.extractRenderState(graphics, mouseX, mouseY, a);
                    }
                }
            }

            @Override
            public List<? extends GuiEventListener> children() {
                return widgets;
            }

            @Override
            public List<? extends NarratableEntry> narratables() {
                return widgets;
            }
        }
    }
}
