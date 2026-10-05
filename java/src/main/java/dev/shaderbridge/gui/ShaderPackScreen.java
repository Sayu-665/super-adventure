package dev.shaderbridge.gui;

import com.mojang.blaze3d.Blaze3D;
import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.model.Diagnostic;
import dev.shaderbridge.model.Severity;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.pack.PackEntry;
import dev.shaderbridge.render.frame.RenderBridge;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.concurrent.CompletionException;
import net.minecraft.ChatFormatting;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.ObjectSelectionList;
import net.minecraft.client.gui.components.Tooltip;
import net.minecraft.client.gui.layouts.HeaderAndFooterLayout;
import net.minecraft.client.gui.layouts.LinearLayout;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.input.KeyEvent;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.network.chat.CommonComponents;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.util.FormattedCharSequence;

/**
 * Lists the packs in {@code shaderpacks/}: select one and apply it, enable or disable shaders,
 * open the pack's options or the folder, reload, and read the native library status, the
 * rendering diagnostics of the active pack (programs that fall back or are skipped, unsupported
 * features, draws that cannot be shaded, why it stopped rendering) and its compile diagnostics in
 * the panel on the right.
 */
public final class ShaderPackScreen extends Screen {
    private static final int FOOTER_HEIGHT = 60;
    private static final int MAX_DIAGNOSTICS = 200;

    private final Screen parent;
    private final HeaderAndFooterLayout layout = new HeaderAndFooterLayout(this, 33, FOOTER_HEIGHT);
    private PackList packList;
    private InfoPanel info;
    private Button applyButton;
    private Button toggleButton;
    private Button optionsButton;
    private String selected;
    private String optionsError;
    private boolean loadingOptions;
    private Object shownState;

    /** @param parent screen to return to, or null to return to the game */
    public ShaderPackScreen(Screen parent) {
        super(Component.translatable("shaderbridge.screen.packs.title"));
        this.parent = parent;
    }

    private ShaderBridge bridge() {
        return ShaderBridge.get();
    }

    @Override
    protected void init() {
        selected = bridge().config().get().selectedPack();
        layout.addTitleHeader(title, font);
        packList = addRenderableWidget(new PackList());
        info = addRenderableWidget(new InfoPanel());
        LinearLayout footer = layout.addToFooter(LinearLayout.vertical().spacing(4));
        LinearLayout top = footer.addChild(LinearLayout.horizontal().spacing(8));
        LinearLayout bottom = footer.addChild(LinearLayout.horizontal().spacing(8));
        applyButton = top.addChild(Button.builder(Component.translatable("shaderbridge.screen.packs.apply"), b -> apply()).width(100).build());
        toggleButton = top.addChild(Button.builder(Component.empty(), b -> bridge().setEnabled(!bridge().config().get().enabled())).width(100).build());
        optionsButton = top.addChild(Button.builder(Component.translatable("shaderbridge.screen.packs.options"), b -> openOptions()).width(100).build());
        bottom.addChild(Button.builder(Component.translatable("shaderbridge.screen.packs.open_folder"), b -> Blaze3D.openPath(bridge().packs().directory()))
            .tooltip(Tooltip.create(Component.literal(bridge().packs().directory().toString()))).width(100).build());
        bottom.addChild(Button.builder(Component.translatable("shaderbridge.screen.packs.reload"), b -> {
            packList.reload();
            bridge().reload();
        }).width(100).build());
        bottom.addChild(Button.builder(CommonComponents.GUI_DONE, b -> onClose()).width(100).build());
        layout.visitWidgets(this::addRenderableWidget);
        packList.reload();
        repositionElements();
    }

    @Override
    protected void repositionElements() {
        layout.arrangeElements();
        int top = layout.getHeaderHeight();
        int height = layout.getContentHeight();
        int columnWidth = Math.min(300, (width - 24) / 2);
        packList.updateSizeAndPosition(columnWidth, height, width / 2 - columnWidth - 4, top);
        info.updateSizeAndPosition(columnWidth, height, width / 2 + 4, top);
        shownState = null;
        refresh();
    }

    @Override
    public void tick() {
        refresh();
    }

    @Override
    public void onClose() {
        minecraft.gui.setScreen(parent);
    }

    /** Rebuilds the info panel and button states when anything they show has changed. */
    private void refresh() {
        Object state = List.of(String.valueOf(bridge().status()), String.valueOf(selected), bridge().config().get(),
            String.valueOf(optionsError), loadingOptions, bridge().activePack().map(System::identityHashCode).orElse(0), RenderBridge.diagnostics());
        if (state.equals(shownState)) {
            return;
        }
        shownState = state;
        boolean enabled = bridge().config().get().enabled();
        toggleButton.setMessage(CommonComponents.optionNameValue(Component.translatable("shaderbridge.screen.packs.shaders"), CommonComponents.optionStatus(enabled)));
        PackEntry entry = packList.entry(selected);
        applyButton.active = entry != null && entry.valid() && NativeLibrary.isLoaded();
        optionsButton.active = entry != null && entry.valid() && NativeLibrary.isLoaded() && !loadingOptions;
        info.show(lines(entry));
    }

    private void apply() {
        if (selected != null) {
            optionsError = null;
            bridge().selectPack(selected);
        }
    }

    private void openOptions() {
        PackEntry entry = packList.entry(selected);
        if (entry == null) {
            return;
        }
        loadingOptions = true;
        optionsError = null;
        OptionsLoader.load(entry, minecraft.options.languageCode).whenComplete((editor, error) -> minecraft.execute(() -> {
            loadingOptions = false;
            if (error != null) {
                Throwable cause = error instanceof CompletionException && error.getCause() != null ? error.getCause() : error;
                optionsError = cause.getMessage();
            } else if (minecraft.gui.screen() == this) {
                minecraft.gui.setScreen(PackOptionsScreen.main(this, entry.name(), editor));
            }
        }));
    }

    /** The text of the info panel for the selected pack. */
    private List<Component> lines(PackEntry entry) {
        List<Component> lines = new ArrayList<>();
        if (NativeLibrary.status() instanceof NativeLibrary.Status.Failed failed) {
            lines.add(Component.translatable("shaderbridge.status.native_failed", failed.reason()).withStyle(ChatFormatting.RED));
        } else if (NativeLibrary.status() instanceof NativeLibrary.Status.Loaded loaded) {
            lines.add(Component.translatable("shaderbridge.status.native_loaded", loaded.version()).withStyle(ChatFormatting.GRAY));
        }
        lines.add(statusLine());
        if (loadingOptions) {
            lines.add(Component.translatable("shaderbridge.screen.options.loading"));
        }
        if (optionsError != null) {
            lines.add(Component.translatable("shaderbridge.screen.options.error", optionsError).withStyle(ChatFormatting.RED));
        }
        if (entry != null && !entry.valid()) {
            lines.add(Component.translatable("shaderbridge.screen.packs.invalid", Objects.toString(entry.error(), "")).withStyle(ChatFormatting.RED));
        }
        LoadedPack active = bridge().activePack().orElse(null);
        if (active != null && active.name().equals(selected)) {
            lines.add(Component.literal(active.summary().describe()).withStyle(active.summary().errors() > 0 ? ChatFormatting.RED : ChatFormatting.GREEN));
            if (!active.model().info().featuresUnsupported().isEmpty()) {
                lines.add(Component.translatable("shaderbridge.status.unsupported_features", String.join(", ", active.model().info().featuresUnsupported()))
                    .withStyle(ChatFormatting.RED));
            }
            List<String> rendering = RenderBridge.diagnostics();
            if (!rendering.isEmpty()) {
                lines.add(Component.translatable("shaderbridge.status.render_diagnostics", rendering.size()).withStyle(ChatFormatting.GOLD));
                rendering.stream().limit(MAX_DIAGNOSTICS).map(m -> Component.literal("- " + m).withStyle(ChatFormatting.YELLOW)).forEach(lines::add);
            }
            List<Diagnostic> compile = active.diagnostics().stream()
                .filter(d -> d.severity() != Severity.INFO)
                .sorted((a, b) -> b.severity().compareTo(a.severity()))
                .limit(MAX_DIAGNOSTICS)
                .toList();
            if (!compile.isEmpty()) {
                lines.add(Component.translatable("shaderbridge.status.compile_diagnostics").withStyle(ChatFormatting.GOLD));
                compile.stream().map(ShaderPackScreen::diagnosticLine).forEach(lines::add);
            }
        }
        return lines;
    }

    private Component statusLine() {
        return switch (bridge().status()) {
            case ShaderBridge.Status.Idle _ -> idleLine();
            case ShaderBridge.Status.Compiling compiling -> Component.translatable("shaderbridge.status.compiling", compiling.pack()).withStyle(ChatFormatting.YELLOW);
            case ShaderBridge.Status.Ready ready -> Component.translatable("shaderbridge.status.ready", ready.pack()).withStyle(ChatFormatting.GREEN);
            case ShaderBridge.Status.Failed failed -> Component.translatable("shaderbridge.status.failed", failed.pack(), failed.reason()).withStyle(ChatFormatting.RED);
        };
    }

    private Component idleLine() {
        if (!bridge().config().get().enabled()) {
            return Component.translatable("shaderbridge.status.disabled");
        }
        String pack = bridge().config().get().selectedPack();
        return pack == null ? Component.translatable("shaderbridge.status.idle") : Component.translatable("shaderbridge.status.pending", pack);
    }

    private static Component diagnosticLine(Diagnostic diagnostic) {
        ChatFormatting color = diagnostic.severity() == Severity.ERROR ? ChatFormatting.RED : ChatFormatting.YELLOW;
        return Component.literal(diagnostic.toString()).withStyle(color);
    }

    /** The packs, with the active one marked. */
    private final class PackList extends ObjectSelectionList<PackList.Entry> {
        PackList() {
            super(ShaderPackScreen.this.minecraft, 200, 100, 33, 18);
        }

        /** Rescans the folder and keeps the selection. */
        void reload() {
            List<Entry> entries = bridge().packs().scan().stream().map(Entry::new).toList();
            replaceEntries(entries);
            entries.stream().filter(e -> e.pack.name().equals(selected)).findFirst().ifPresent(this::setSelected);
            shownState = null;
        }

        PackEntry entry(String name) {
            return children().stream().map(e -> e.pack).filter(p -> p.name().equals(name)).findFirst().orElse(null);
        }

        @Override
        public int getRowWidth() {
            return getWidth() - 12;
        }

        /** One pack. */
        final class Entry extends ObjectSelectionList.Entry<Entry> {
            private final PackEntry pack;

            Entry(PackEntry pack) {
                this.pack = pack;
            }

            @Override
            public void extractContent(GuiGraphicsExtractor graphics, int mouseX, int mouseY, boolean hovered, float a) {
                boolean active = bridge().activePack().map(p -> p.name().equals(pack.name())).orElse(false);
                MutableComponent name = Component.literal(pack.name());
                if (!pack.valid()) {
                    name.withStyle(ChatFormatting.GRAY, ChatFormatting.STRIKETHROUGH);
                } else if (active) {
                    name.withStyle(ChatFormatting.GREEN);
                }
                graphics.text(font, name, getContentX() + 2, getContentYMiddle() - font.lineHeight / 2, -1);
                if (hovered && !pack.valid() && pack.error() != null) {
                    graphics.setTooltipForNextFrame(font, Component.literal(pack.error()), mouseX, mouseY);
                }
            }

            @Override
            public boolean mouseClicked(MouseButtonEvent event, boolean doubleClick) {
                select();
                if (doubleClick) {
                    apply();
                }
                return true;
            }

            @Override
            public boolean keyPressed(KeyEvent event) {
                if (event.isSelection()) {
                    select();
                    apply();
                    return true;
                }
                return super.keyPressed(event);
            }

            private void select() {
                setSelected(this);
                selected = pack.name();
                optionsError = null;
            }

            @Override
            public Component getNarration() {
                return Component.translatable("narrator.select", pack.name());
            }
        }
    }

    /** Word-wrapped status and diagnostics, scrollable. */
    private final class InfoPanel extends ObjectSelectionList<InfoPanel.Line> {
        private List<Component> text = List.of();

        InfoPanel() {
            super(ShaderPackScreen.this.minecraft, 200, 100, 33, font.lineHeight + 1);
        }

        void show(List<Component> lines) {
            text = lines;
            rewrap();
        }

        @Override
        public void updateSizeAndPosition(int width, int height, int x, int y) {
            super.updateSizeAndPosition(width, height, x, y);
            rewrap();
        }

        private void rewrap() {
            List<Line> lines = new ArrayList<>();
            for (Component component : text) {
                for (FormattedCharSequence line : font.split(component, Math.max(20, getRowWidth() - 4))) {
                    lines.add(new Line(line));
                }
            }
            replaceEntries(lines);
        }

        @Override
        public int getRowWidth() {
            return getWidth() - 12;
        }

        @Override
        protected boolean entriesCanBeSelected() {
            return false;
        }

        /** One wrapped line. */
        final class Line extends ObjectSelectionList.Entry<Line> {
            private final FormattedCharSequence line;

            Line(FormattedCharSequence line) {
                this.line = line;
            }

            @Override
            public void extractContent(GuiGraphicsExtractor graphics, int mouseX, int mouseY, boolean hovered, float a) {
                graphics.text(font, line, getContentX() + 2, getContentY(), -1);
            }

            @Override
            public Component getNarration() {
                return Component.empty();
            }
        }
    }
}
