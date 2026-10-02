package dev.shaderbridge.gui;

import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.OptionScreen;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.PackOption;
import dev.shaderbridge.model.ScreenEntry;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * The pending (unsaved) option values of one pack while its options screens are open, shared by
 * the main screen and every sub-screen.
 */
public final class OptionsEditor {
    private final OptionsModel model;
    private final Map<String, PackOption> options = new LinkedHashMap<>();
    private final Map<String, String> pending = new HashMap<>();
    private final PackLang lang;

    /**
     * @param model the pack's options model
     * @param saved the user's stored values; options without one start at the model's value
     */
    public OptionsEditor(OptionsModel model, PackOptionValues saved) {
        this.model = model;
        this.lang = new PackLang(model.lang());
        for (PackOption option : model.options()) {
            options.put(option.name(), option);
            pending.put(option.name(), saved.get(option.name()).orElse(option.value()));
        }
    }

    /** @return the options model */
    public OptionsModel model() {
        return model;
    }

    /** @return the pack's lang strings */
    public PackLang lang() {
        return lang;
    }

    /**
     * @param name option name
     * @return the option, if the pack declares it
     */
    public Optional<PackOption> option(String name) {
        return Optional.ofNullable(options.get(name));
    }

    /**
     * @param name option name
     * @return the pending value
     */
    public String value(String name) {
        return pending.get(name);
    }

    /**
     * @param name  option name
     * @param value new pending value
     */
    public void set(String name, String value) {
        if (options.containsKey(name)) {
            pending.put(name, value);
        }
    }

    /**
     * Steps through the allowed values (booleans toggle).
     *
     * @param name  option name
     * @param steps +1 for the next value, -1 for the previous one
     */
    public void cycle(String name, int steps) {
        PackOption option = options.get(name);
        if (option == null) {
            return;
        }
        List<String> values = allowedValues(option);
        int index = Math.max(0, values.indexOf(pending.get(name)));
        pending.put(name, values.get(Math.floorMod(index + steps, values.size())));
    }

    /**
     * @param option an option
     * @return its values in display order: {@code false, true} for booleans
     */
    public static List<String> allowedValues(PackOption option) {
        if (option.kind().isBoolean()) {
            return List.of("false", "true");
        }
        if (option.allowed().isEmpty()) {
            return List.of(option.defaultValue());
        }
        return option.allowed();
    }

    /** @param name option name; reverts it to the pack default */
    public void reset(String name) {
        PackOption option = options.get(name);
        if (option != null) {
            pending.put(name, option.defaultValue());
        }
    }

    /** Reverts every option to its default. */
    public void resetAll() {
        options.values().forEach(o -> pending.put(o.name(), o.defaultValue()));
    }

    /**
     * @param name option name
     * @return true if the pending value differs from the pack default
     */
    public boolean isChanged(String name) {
        PackOption option = options.get(name);
        return option != null && !option.defaultValue().equals(pending.get(name));
    }

    /** @return the values to store: only those that differ from the defaults, as Iris stores them */
    public PackOptionValues changedValues() {
        PackOptionValues values = PackOptionValues.empty();
        for (PackOption option : options.values()) {
            if (isChanged(option.name())) {
                values = values.with(option.name(), pending.get(option.name()));
            }
        }
        return values;
    }

    /** @return the first profile whose settings all equal the pending values ("custom" if none) */
    public Optional<String> currentProfile() {
        for (Map.Entry<String, Map<String, String>> profile : model.profiles().entrySet()) {
            boolean matches = profile.getValue().entrySet().stream().allMatch(s -> s.getValue().equals(pending.get(s.getKey())));
            if (matches) {
                return Optional.of(profile.getKey());
            }
        }
        return Optional.empty();
    }

    /**
     * Applies the next or previous profile in declaration order (from "custom", the first or last).
     *
     * @param steps +1 for the next profile, -1 for the previous one
     */
    public void cycleProfile(int steps) {
        List<String> names = List.copyOf(model.profiles().keySet());
        if (names.isEmpty()) {
            return;
        }
        int current = currentProfile().map(names::indexOf).orElse(steps > 0 ? -1 : 0);
        String next = names.get(Math.floorMod(current + steps, names.size()));
        model.profiles().get(next).forEach(this::set);
    }

    /**
     * Expands {@code *} into the options that appear on no screen.
     *
     * @param entries entries of a screen
     * @return the entries with every {@link ScreenEntry.Rest} replaced
     */
    public List<ScreenEntry> expand(List<ScreenEntry> entries) {
        List<ScreenEntry> out = new ArrayList<>();
        for (ScreenEntry entry : entries) {
            if (entry instanceof ScreenEntry.Rest) {
                Set<String> placed = placedOptions();
                options.keySet().stream().filter(name -> !placed.contains(name)).map(ScreenEntry.OptionEntry::new).forEach(out::add);
            } else {
                out.add(entry);
            }
        }
        return out;
    }

    private Set<String> placedOptions() {
        Set<String> placed = new HashSet<>();
        collect(model.mainScreen(), placed);
        for (OptionScreen screen : model.screens().values()) {
            collect(screen.entries(), placed);
        }
        return placed;
    }

    private static void collect(List<ScreenEntry> entries, Set<String> placed) {
        for (ScreenEntry entry : entries) {
            if (entry instanceof ScreenEntry.OptionEntry option) {
                placed.add(option.name());
            }
        }
    }
}
