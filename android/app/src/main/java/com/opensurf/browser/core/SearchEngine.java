package com.opensurf.browser.core;

/** A built-in search engine with one URL template per SafeSearch state. "{q}" marks the query. */
public final class SearchEngine {
    static final String QUERY_PLACEHOLDER = "{q}";

    private final String id;
    private final String name;
    private final String safeOffTemplate;
    private final String safeOnTemplate;

    SearchEngine(String id, String name, String safeOffTemplate, String safeOnTemplate) {
        this.id = id;
        this.name = name;
        this.safeOffTemplate = safeOffTemplate;
        this.safeOnTemplate = safeOnTemplate;
    }

    public String getId() {
        return id;
    }

    public String getName() {
        return name;
    }

    /** Builds the result-page URL for {@code query}; the query is percent-encoded here. */
    public String searchUrl(String query, boolean safeSearch) {
        String template = safeSearch ? safeOnTemplate : safeOffTemplate;
        return template.replace(QUERY_PLACEHOLDER, SearchEngines.encodeQuery(query));
    }
}
