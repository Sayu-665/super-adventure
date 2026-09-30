package com.opensurf.browser;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.text.Editable;
import android.text.TextWatcher;
import android.util.TypedValue;
import android.view.View;
import android.widget.EditText;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import com.opensurf.browser.core.SearchEngine;
import com.opensurf.browser.core.SearchEngines;

import java.util.HashMap;
import java.util.Map;

/**
 * Settings (SPEC E), built from framework widgets only. Every change is saved immediately;
 * the browser applies it when it resumes.
 */
public class SettingsActivity extends Activity {
    /** Result extra: browsing data was cleared, so the browser clears its tabs too. */
    static final String EXTRA_DATA_CLEARED = "com.opensurf.browser.extra.DATA_CLEARED";

    private Prefs prefs;
    private final Map<Integer, String> engineIdsByViewId = new HashMap<>();
    private int customRadioId;
    private RadioGroup engineGroup;
    private EditText customTemplate;
    private TextView customHelp;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_settings);
        prefs = new Prefs(this);
        SystemBars.enableEdgeToEdge(getWindow());
        SystemBars.applyInsetsAsPadding(findViewById(R.id.settings_root));
        findViewById(R.id.settings_up).setOnClickListener(v -> finish());

        engineGroup = findViewById(R.id.engine_group);
        customTemplate = findViewById(R.id.custom_template);
        customHelp = findViewById(R.id.custom_help);
        buildEngineChoices();

        String storedEngine = prefs.engineId();
        customTemplate.setText(prefs.customTemplate());
        int checkedId = customRadioId;
        if (!SearchEngines.CUSTOM_ID.equals(storedEngine)
                || !SearchEngines.isValidCustomTemplate(prefs.customTemplate())) {
            SearchEngine engine = SearchEngines.byId(storedEngine);
            checkedId = idForEngine(engine != null ? engine.getId() : SearchEngines.DEFAULT_ID);
        }
        engineGroup.check(checkedId);
        updateCustomTemplateUi();
        engineGroup.setOnCheckedChangeListener((group, id) -> onEngineChecked(id));
        customTemplate.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {
            }

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {
            }

            @Override
            public void afterTextChanged(Editable s) {
                onCustomTemplateEdited(s.toString());
            }
        });

        bindSwitch(R.id.safe_search_switch, prefs.safeSearch(), prefs::setSafeSearch);
        bindSwitch(R.id.javascript_switch, prefs.javaScript(), prefs::setJavaScript);
        bindSwitch(R.id.desktop_switch, prefs.desktopSite(), prefs::setDesktopSite);
        findViewById(R.id.clear_data).setOnClickListener(v -> confirmClearBrowsingData());

        TextView about = findViewById(R.id.about_text);
        about.setText(getString(R.string.settings_about_text, versionName()));
    }

    @Override
    protected void onPause() {
        super.onPause();
        if (isCustomChecked() && !SearchEngines.isValidCustomTemplate(customTemplate.getText().toString())) {
            Toast.makeText(this, R.string.settings_custom_not_saved, Toast.LENGTH_LONG).show();
        }
    }

    private void buildEngineChoices() {
        for (SearchEngine engine : SearchEngines.builtIn()) {
            int id = addRadio(engine.getName());
            engineIdsByViewId.put(id, engine.getId());
        }
        customRadioId = addRadio(getString(R.string.settings_custom_engine));
        engineIdsByViewId.put(customRadioId, SearchEngines.CUSTOM_ID);
    }

    private int addRadio(CharSequence label) {
        RadioButton radio = new RadioButton(this);
        radio.setId(View.generateViewId());
        radio.setText(label);
        radio.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        radio.setTextColor(getColor(R.color.text_primary));
        radio.setMinHeight(Math.round(48 * getResources().getDisplayMetrics().density));
        engineGroup.addView(radio, new RadioGroup.LayoutParams(
                RadioGroup.LayoutParams.MATCH_PARENT, RadioGroup.LayoutParams.WRAP_CONTENT));
        return radio.getId();
    }

    private int idForEngine(String engineId) {
        for (Map.Entry<Integer, String> entry : engineIdsByViewId.entrySet()) {
            if (entry.getValue().equals(engineId)) {
                return entry.getKey();
            }
        }
        return View.NO_ID;
    }

    private boolean isCustomChecked() {
        return engineGroup.getCheckedRadioButtonId() == customRadioId;
    }

    private void onEngineChecked(int viewId) {
        String engineId = engineIdsByViewId.get(viewId);
        if (engineId == null) {
            return;
        }
        if (SearchEngines.CUSTOM_ID.equals(engineId)) {
            // Only switch to the custom engine once its template is valid.
            String template = customTemplate.getText().toString();
            if (SearchEngines.isValidCustomTemplate(template)) {
                prefs.setCustomTemplate(template);
                prefs.setEngineId(SearchEngines.CUSTOM_ID);
            }
            customTemplate.requestFocus();
        } else {
            prefs.setEngineId(engineId);
        }
        updateCustomTemplateUi();
    }

    private void onCustomTemplateEdited(String template) {
        boolean valid = SearchEngines.isValidCustomTemplate(template);
        if (valid) {
            prefs.setCustomTemplate(template);
            if (isCustomChecked()) {
                prefs.setEngineId(SearchEngines.CUSTOM_ID);
            }
        }
        updateCustomTemplateUi();
    }

    private void updateCustomTemplateUi() {
        boolean custom = isCustomChecked();
        customTemplate.setVisibility(custom ? View.VISIBLE : View.GONE);
        customHelp.setVisibility(custom ? View.VISIBLE : View.GONE);
        String template = customTemplate.getText().toString();
        boolean showError = custom && !template.trim().isEmpty()
                && !SearchEngines.isValidCustomTemplate(template);
        customTemplate.setError(showError ? getString(R.string.settings_custom_invalid) : null);
    }

    private interface BooleanSetter {
        void set(boolean value);
    }

    private void bindSwitch(int viewId, boolean checked, BooleanSetter setter) {
        Switch toggle = findViewById(viewId);
        toggle.setChecked(checked);
        toggle.setOnCheckedChangeListener((button, isChecked) -> setter.set(isChecked));
    }

    private void confirmClearBrowsingData() {
        new AlertDialog.Builder(this)
                .setTitle(R.string.clear_data_title)
                .setMessage(R.string.clear_data_message)
                .setPositiveButton(R.string.clear_data_confirm, (d, w) -> {
                    BrowsingData.clearShared(this);
                    // The browser clears the HTTP cache and tab histories when we return.
                    setResult(RESULT_OK, new Intent().putExtra(EXTRA_DATA_CLEARED, true));
                    Toast.makeText(this, R.string.data_cleared, Toast.LENGTH_SHORT).show();
                })
                .setNegativeButton(R.string.cancel, null)
                .show();
    }

    @SuppressWarnings("deprecation")
    private String versionName() {
        try {
            PackageInfo info = getPackageManager().getPackageInfo(getPackageName(), 0);
            return info.versionName;
        } catch (PackageManager.NameNotFoundException e) {
            return "";
        }
    }
}
