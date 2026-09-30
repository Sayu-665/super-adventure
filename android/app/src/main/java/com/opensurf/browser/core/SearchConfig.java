package com.opensurf.browser.core;

/** The user's search settings: engine, SafeSearch state and optional custom template. */
public final class SearchConfig {
    private final String engineId;
    private final boolean safeSearch;
    private final String customTemplate;

    public SearchConfig(String engineId, boolean safeSearch, String customTemplate) {
        this.engineId = engineId == null ? SearchEngines.DEFAULT_ID : engineId;
        this.safeSearch = safeSearch;
        this.customTemplate = customTemplate == null ? "" : customTemplate.trim();
    }

    /** DuckDuckGo with SafeSearch off. */
    public static SearchConfig defaults() {
        return new SearchConfig(SearchEngines.DEFAULT_ID, false, "");
    }

    public boolean isSafeSearch() {
        return safeSearch;
    }

    /** True when a valid custom template is in use (an invalid one falls back to the default). */
    public boolean isCustom() {
        return SearchEngines.CUSTOM_ID.equals(engineId)
                && SearchEngines.isValidCustomTemplate(customTemplate);
    }

    /** The built-in engine in effect; the default engine for unknown ids or a custom engine. */
    public SearchEngine builtInEngine() {
        SearchEngine engine = SearchEngines.byId(engineId);
        return engine != null ? engine : SearchEngines.defaultEngine();
    }

    public String engineName() {
        return isCustom() ? SearchEngines.CUSTOM_NAME : builtInEngine().getName();
    }

    /** Result-page URL for {@code query}. The SafeSearch toggle never modifies a custom template. */
    public String searchUrl(String query) {
        if (isCustom()) {
            return SearchEngines.applyCustomTemplate(customTemplate, query);
        }
        return builtInEngine().searchUrl(query, safeSearch);
    }
}
