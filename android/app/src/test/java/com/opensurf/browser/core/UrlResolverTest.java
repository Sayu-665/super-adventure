package com.opensurf.browser.core;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/** The mandatory SPEC A table plus edge cases. */
public class UrlResolverTest {
    private static final SearchConfig DEFAULTS = SearchConfig.defaults();

    private static String resolve(String input) {
        return UrlResolver.resolve(input, DEFAULTS);
    }

    private static String ddg(String encoded) {
        return "https://duckduckgo.com/?q=" + encoded + "&kp=-2";
    }

    // --- Mandatory table (identical on every platform) ---

    @Test
    public void bareDomainGetsHttps() {
        assertEquals("https://example.com", resolve("example.com"));
    }

    @Test
    public void inputIsTrimmedAndCaseIsKept() {
        assertEquals("https://Example.COM", resolve("  Example.COM  "));
    }

    @Test
    public void httpUrlLoadsAsIs() {
        assertEquals("http://example.com", resolve("http://example.com"));
    }

    @Test
    public void upperCaseHttpsUrlLoadsUnchanged() {
        assertEquals("HTTPS://example.com/a?b=c", resolve("HTTPS://example.com/a?b=c"));
    }

    @Test
    public void domainWithPathAndQueryGetsHttps() {
        assertEquals("https://sub.example.co.uk/path?x=1", resolve("sub.example.co.uk/path?x=1"));
    }

    @Test
    public void localhostWithPortGetsHttp() {
        assertEquals("http://localhost:8080", resolve("localhost:8080"));
    }

    @Test
    public void ipv4GetsHttp() {
        assertEquals("http://192.168.1.1", resolve("192.168.1.1"));
    }

    @Test
    public void bracketedIpv6WithPortGetsHttp() {
        assertEquals("http://[::1]:3000", resolve("[::1]:3000"));
    }

    @Test
    public void wordsAreSearched() {
        assertEquals(ddg("hello%20world"), resolve("hello world"));
    }

    @Test
    public void plusSignIsPercentEncoded() {
        String url = resolve("what is 2+2");
        assertEquals(ddg("what%20is%202%2B2"), url);
        assertTrue(url.contains("%2B"));
    }

    @Test
    public void singleWordIsSearched() {
        assertEquals(ddg("example"), resolve("example"));
    }

    @Test
    public void searchOperatorsAreSearched() {
        assertEquals(ddg("cats%20site%3Areddit.com"), resolve("cats site:reddit.com"));
    }

    @Test
    public void javascriptSchemeIsSearchedNeverExecuted() {
        assertEquals(ddg("javascript%3Aalert%281%29"), resolve("javascript:alert(1)"));
    }

    @Test
    public void emptyInputDoesNothing() {
        assertNull(resolve(""));
        assertNull(resolve("   "));
        assertNull(resolve(null));
    }

    // --- Additional behaviour ---

    @Test
    public void otherExplicitSchemesAreSearched() {
        assertEquals(ddg("data%3Atext%2Fhtml%2C%3Cb%3Ehi%3C%2Fb%3E"), resolve("data:text/html,<b>hi</b>"));
        assertEquals(ddg("ftp%3A%2F%2Fexample.com"), resolve("ftp://example.com"));
        assertEquals(ddg("file%3A%2F%2F%2Fetc%2Fhosts"), resolve("file:///etc/hosts"));
        assertEquals(ddg("mailto%3Ame%40example.com"), resolve("mailto:me@example.com"));
        assertEquals(ddg("about%3Ablank"), resolve("about:blank"));
    }

    @Test
    public void reservedCharactersAreEncoded() {
        assertEquals(ddg("a%20%26%20b%20%23c%20%3Fd%20%2Be"), resolve("a & b #c ?d +e"));
    }

    @Test
    public void unicodeIsUtf8Encoded() {
        assertEquals(ddg("caf%C3%A9%20%E2%98%95"), resolve("café ☕"));
    }

    @Test
    public void hostEdgeCases() {
        assertEquals("http://localhost", resolve("localhost"));
        assertEquals("http://LOCALHOST/x", resolve("LOCALHOST/x"));
        assertEquals("http://127.0.0.1:3000/app#top", resolve("127.0.0.1:3000/app#top"));
        assertEquals("http://[::1]", resolve("[::1]"));
        assertEquals("https://example.com:8443/x", resolve("example.com:8443/x"));
        assertEquals("https://example.com?q=1", resolve("example.com?q=1"));
        assertEquals("https://münchen.de", resolve("münchen.de"));
        assertEquals("https://my-site.example.org/", resolve("my-site.example.org/"));
    }

    @Test
    public void notAHost() {
        assertEquals(ddg("256.1.1.1"), resolve("256.1.1.1"));
        assertEquals(ddg("1.2.3"), resolve("1.2.3"));
        assertEquals(ddg("example.c"), resolve("example.c"));
        assertEquals(ddg("example.123"), resolve("example.123"));
        assertEquals(ddg("-bad.com"), resolve("-bad.com"));
        assertEquals(ddg("a..com"), resolve("a..com"));
        assertEquals(ddg("example.com%3A99999"), resolve("example.com:99999"));
        assertEquals(ddg("user%40example.com"), resolve("user@example.com"));
        assertEquals(ddg("example.com%2Fsome%20path"), resolve("example.com/some path"));
    }

    @Test
    public void searchUsesSelectedEngine() {
        SearchConfig google = new SearchConfig("google", true, "");
        assertEquals("https://www.google.com/search?q=hello%20world&safe=active",
                UrlResolver.resolve("hello world", google));
        SearchConfig custom = new SearchConfig("custom", false, "https://s.example/find?text=%s");
        assertEquals("https://s.example/find?text=hello%20world",
                UrlResolver.resolve("hello world", custom));
    }

    @Test
    public void sharedTextPrefersEmbeddedUrl() {
        assertEquals("https://example.com/a?b=1",
                UrlResolver.resolveSharedText("Look at this: https://example.com/a?b=1.", DEFAULTS));
        assertEquals("http://example.org/page",
                UrlResolver.resolveSharedText("(see http://example.org/page)", DEFAULTS));
        assertEquals("https://example.com", UrlResolver.resolveSharedText("example.com", DEFAULTS));
        assertEquals(ddg("just%20words"), UrlResolver.resolveSharedText("  just words ", DEFAULTS));
        assertNull(UrlResolver.resolveSharedText("   ", DEFAULTS));
        assertNull(UrlResolver.resolveSharedText(null, DEFAULTS));
    }
}
