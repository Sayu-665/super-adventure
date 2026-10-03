package dev.shaderbridge.gui;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.OptionKind;
import dev.shaderbridge.model.OptionScreen;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.PackOption;
import dev.shaderbridge.model.ScreenEntry;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class OptionsEditorTest {
    private static PackOption option(String name, OptionKind kind, String defaultValue, String... allowed) {
        return new PackOption(name, kind, defaultValue, defaultValue, List.of(allowed), "// [" + String.join(" ", allowed) + "]", "settings.glsl", 1);
    }

    private static OptionsModel model() {
        Map<String, Map<String, String>> profiles = new LinkedHashMap<>();
        profiles.put("LOW", Map.of("SHADOWS", "false", "QUALITY", "1"));
        profiles.put("HIGH", Map.of("SHADOWS", "true", "QUALITY", "3"));
        Map<String, String> lang = Map.of(
            "option.QUALITY", "Quality",
            "value.QUALITY.3", "Ultra",
            "prefix.DISTANCE", "~",
            "suffix.DISTANCE", " m",
            "option.SHADOWS.comment", "Draws shadows",
            "screen.MORE", "More Settings",
            "profile.LOW", "Potato");
        return new OptionsModel(
            List.of(option("SHADOWS", OptionKind.BOOLEAN_DEFINE, "true"), option("QUALITY", OptionKind.VALUE_DEFINE, "2", "1", "2", "3"),
                option("DISTANCE", OptionKind.CONST, "64", "32", "64"), option("HIDDEN", OptionKind.VALUE_DEFINE, "a", "a", "b")),
            List.of(new ScreenEntry.ProfileSelector(), new ScreenEntry.OptionEntry("SHADOWS"), new ScreenEntry.ScreenLink("MORE"), new ScreenEntry.Rest()),
            2, Map.of("MORE", new OptionScreen(List.of(new ScreenEntry.OptionEntry("QUALITY")), 1)), List.of("DISTANCE"), profiles, null, Map.of(), lang);
    }

    @Test
    void savedValuesOverrideTheModel() {
        OptionsEditor editor = new OptionsEditor(model(), PackOptionValues.empty().with("QUALITY", "3"));
        assertEquals("3", editor.value("QUALITY"));
        assertEquals("true", editor.value("SHADOWS"));
        assertTrue(editor.isChanged("QUALITY"));
        assertEquals(PackOptionValues.empty().with("QUALITY", "3"), editor.changedValues());
    }

    @Test
    void cyclingWrapsAndBooleansToggle() {
        OptionsEditor editor = new OptionsEditor(model(), PackOptionValues.empty());
        editor.cycle("QUALITY", 1);
        assertEquals("3", editor.value("QUALITY"));
        editor.cycle("QUALITY", 1);
        assertEquals("1", editor.value("QUALITY"));
        editor.cycle("QUALITY", -1);
        assertEquals("3", editor.value("QUALITY"));
        editor.cycle("SHADOWS", 1);
        assertEquals("false", editor.value("SHADOWS"));
        editor.reset("SHADOWS");
        assertFalse(editor.isChanged("SHADOWS"));
        editor.resetAll();
        assertTrue(editor.changedValues().isEmpty());
    }

    @Test
    void profilesAreDetectedAndApplied() {
        OptionsEditor editor = new OptionsEditor(model(), PackOptionValues.empty());
        assertEquals(Optional.empty(), editor.currentProfile());
        editor.cycleProfile(1);
        assertEquals(Optional.of("LOW"), editor.currentProfile());
        assertEquals("false", editor.value("SHADOWS"));
        editor.cycleProfile(1);
        assertEquals(Optional.of("HIGH"), editor.currentProfile());
        editor.cycleProfile(1);
        assertEquals(Optional.of("LOW"), editor.currentProfile());
        editor.cycle("QUALITY", 1);
        assertEquals(Optional.empty(), editor.currentProfile(), "a changed option makes the profile custom");
    }

    @Test
    void profileMatchingPrefersProfilesWithMoreSettings() {
        Map<String, Map<String, String>> profiles = new LinkedHashMap<>();
        profiles.put("BASE", Map.of("SHADOWS", "true"));
        profiles.put("EXTENDED", Map.of("SHADOWS", "true", "QUALITY", "2", "NOT_AN_OPTION", "x"));
        OptionsModel base = model();
        OptionsModel model = new OptionsModel(base.options(), base.mainScreen(), base.mainScreenColumns(), base.screens(), base.sliders(),
            profiles, null, Map.of(), base.lang());
        assertEquals(Optional.of("EXTENDED"), new OptionsEditor(model, PackOptionValues.empty()).currentProfile(),
            "both match: Iris scans profiles with more settings first; unknown options are ignored");
        OptionsEditor editor = new OptionsEditor(model, PackOptionValues.empty().with("QUALITY", "3").with("DISTANCE", "32"));
        assertEquals(Optional.of("BASE"), editor.currentProfile());
        editor.cycleProfile(1);
        assertEquals(Optional.of("EXTENDED"), editor.currentProfile(), "cycling wraps around the scan order");
        assertEquals("2", editor.value("QUALITY"));
        assertEquals("32", editor.value("DISTANCE"), "a profile leaves options it does not mention alone");
    }

    @Test
    void restExpandsToUnplacedOptions() {
        OptionsEditor editor = new OptionsEditor(model(), PackOptionValues.empty());
        List<ScreenEntry> expanded = editor.expand(editor.model().mainScreen());
        assertEquals(List.of(new ScreenEntry.ProfileSelector(), new ScreenEntry.OptionEntry("SHADOWS"), new ScreenEntry.ScreenLink("MORE"),
            new ScreenEntry.OptionEntry("DISTANCE"), new ScreenEntry.OptionEntry("HIDDEN")), expanded);
    }

    @Test
    void langStringsFollowOptiFineKeys() {
        PackLang lang = new OptionsEditor(model(), PackOptionValues.empty()).lang();
        assertEquals("Quality", lang.optionName("QUALITY"));
        assertEquals("SHADOWS", lang.optionName("SHADOWS"));
        assertEquals("Ultra", lang.value("QUALITY", "3"));
        assertEquals("2", lang.value("QUALITY", "2"));
        assertEquals("~64 m", lang.value("DISTANCE", "64"));
        assertEquals(Optional.of("Draws shadows"), lang.comment(model().options().get(0)));
        assertEquals(Optional.of("// [1 2 3]"), lang.comment(model().options().get(1)), "the source comment is the fallback tooltip");
        assertEquals("More Settings", lang.screenName("MORE"));
        assertEquals("Potato", lang.profileName("LOW"));
        assertEquals("HIGH", lang.profileName("HIGH"));
        assertTrue(lang.profileComment().isEmpty());
    }
}
