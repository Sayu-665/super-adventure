package dev.shaderbridge.model;

import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * Everything the options GUI needs ({@code sb_core::model::OptionsModel}).
 *
 * @param options                  every option of the pack
 * @param mainScreen               entries of the main screen ({@code screen=})
 * @param mainScreenColumns        {@code screen.columns}, or null for the default
 * @param screens                  sub-screens by name ({@code screen.<NAME>=})
 * @param sliders                  options shown as sliders
 * @param profiles                 profile name to settings (option name to value)
 * @param currentProfile           the profile the current values match, or null
 * @param profileDisabledPrograms  programs each profile disables
 * @param lang                     lang strings of the selected language (fallback en_us)
 */
public record OptionsModel(
    List<PackOption> options,
    List<ScreenEntry> mainScreen,
    Integer mainScreenColumns,
    Map<String, OptionScreen> screens,
    List<String> sliders,
    Map<String, Map<String, String>> profiles,
    String currentProfile,
    Map<String, List<String>> profileDisabledPrograms,
    Map<String, String> lang
) {
    public OptionsModel {
        options = Copies.list(options);
        mainScreen = Copies.list(mainScreen);
        screens = Copies.map(screens);
        sliders = Copies.list(sliders);
        profiles = Copies.map(profiles);
        profileDisabledPrograms = Copies.map(profileDisabledPrograms);
        lang = Copies.map(lang);
    }

    /**
     * @param name option name
     * @return the option, if the pack declares it
     */
    public Optional<PackOption> option(String name) {
        return options.stream().filter(o -> o.name().equals(name)).findFirst();
    }
}
