package dev.shaderbridge.render.pipeline;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A reader for the TOML subset the draw profiles use: {@code key = value} pairs with basic,
 * literal and multi-line strings, integers, booleans and (possibly multi-line) arrays of those,
 * {@code [table]} headers and {@code [[array-of-tables]]} headers, and {@code #} comments. Dotted
 * keys, inline tables, floats and dates are rejected.
 */
final class TomlSubset {
    private final String text;
    private int pos;
    private int line = 1;

    private TomlSubset(String text) {
        this.text = text;
    }

    /**
     * Parses a document.
     *
     * @param text TOML text
     * @return the root table; tables are {@code Map<String, Object>}, arrays of tables and arrays
     *     are {@code List<Object>}, scalars are {@code String}, {@code Long} or {@code Boolean}
     * @throws IllegalArgumentException on syntax outside the subset
     */
    static Map<String, Object> parse(String text) {
        return new TomlSubset(text).document();
    }

    private Map<String, Object> document() {
        Map<String, Object> root = new LinkedHashMap<>();
        Map<String, Object> current = root;
        while (true) {
            skipTrivia();
            if (pos >= text.length()) {
                return root;
            }
            if (peek() == '[') {
                current = header(root);
            } else {
                String key = key();
                skipBlanks();
                expect('=');
                skipBlanks();
                Object value = value();
                if (current.put(key, value) != null) {
                    throw error("duplicate key `" + key + "`");
                }
                endOfLine();
            }
        }
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> header(Map<String, Object> root) {
        boolean array = text.startsWith("[[", pos);
        pos += array ? 2 : 1;
        skipBlanks();
        String name = key();
        skipBlanks();
        if (array) {
            expect(']');
        }
        expect(']');
        endOfLine();
        Map<String, Object> table = new LinkedHashMap<>();
        Object existing = root.get(name);
        if (array) {
            if (existing == null) {
                root.put(name, new ArrayList<>(List.of(table)));
            } else if (existing instanceof List<?> list) {
                ((List<Object>) list).add(table);
            } else {
                throw error("`" + name + "` is not an array of tables");
            }
        } else if (existing != null) {
            throw error("table `" + name + "` is defined twice");
        } else {
            root.put(name, table);
        }
        return table;
    }

    private String key() {
        if (peek() == '"') {
            return basicString();
        }
        int start = pos;
        while (pos < text.length() && (Character.isLetterOrDigit(peek()) || peek() == '_' || peek() == '-')) {
            pos++;
        }
        if (start == pos) {
            throw error("expected a key");
        }
        return text.substring(start, pos);
    }

    private Object value() {
        char c = peek();
        if (text.startsWith("\"\"\"", pos)) {
            return multiLine("\"\"\"", true);
        }
        if (text.startsWith("'''", pos)) {
            return multiLine("'''", false);
        }
        if (c == '"') {
            return basicString();
        }
        if (c == '\'') {
            int end = text.indexOf('\'', pos + 1);
            if (end < 0 || text.substring(pos, end).contains("\n")) {
                throw error("unterminated literal string");
            }
            String s = text.substring(pos + 1, end);
            pos = end + 1;
            return s;
        }
        if (c == '[') {
            return array();
        }
        if (text.startsWith("true", pos)) {
            pos += 4;
            return Boolean.TRUE;
        }
        if (text.startsWith("false", pos)) {
            pos += 5;
            return Boolean.FALSE;
        }
        int start = pos;
        if (c == '-' || c == '+') {
            pos++;
        }
        while (pos < text.length() && (Character.isDigit(peek()) || peek() == '_')) {
            pos++;
        }
        if (start == pos || (pos < text.length() && (peek() == '.' || peek() == 'e' || peek() == 'E'))) {
            throw error("unsupported value");
        }
        try {
            return Long.parseLong(text.substring(start, pos).replace("_", ""));
        } catch (NumberFormatException e) {
            throw error("bad integer");
        }
    }

    private List<Object> array() {
        expect('[');
        List<Object> out = new ArrayList<>();
        while (true) {
            skipTrivia();
            if (peek() == ']') {
                pos++;
                return out;
            }
            out.add(value());
            skipTrivia();
            if (peek() == ',') {
                pos++;
            } else if (peek() != ']') {
                throw error("expected `,` or `]` in array");
            }
        }
    }

    private String basicString() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (true) {
            if (pos >= text.length() || peek() == '\n') {
                throw error("unterminated string");
            }
            char c = text.charAt(pos++);
            if (c == '"') {
                return out.toString();
            }
            if (c == '\\') {
                escape(out);
            } else {
                out.append(c);
            }
        }
    }

    private String multiLine(String delimiter, boolean escapes) {
        pos += 3;
        if (text.startsWith("\r\n", pos)) {
            pos += 2;
        } else if (text.startsWith("\n", pos)) {
            pos++;
        }
        StringBuilder out = new StringBuilder();
        while (!text.startsWith(delimiter, pos)) {
            if (pos >= text.length()) {
                throw error("unterminated multi-line string");
            }
            char c = text.charAt(pos++);
            if (c == '\n') {
                line++;
            }
            if (escapes && c == '\\') {
                escape(out);
            } else {
                out.append(c);
            }
        }
        pos += 3;
        return out.toString();
    }

    private void escape(StringBuilder out) {
        if (pos >= text.length()) {
            throw error("unterminated escape");
        }
        char e = text.charAt(pos++);
        switch (e) {
            case 'n' -> out.append('\n');
            case 't' -> out.append('\t');
            case 'r' -> out.append('\r');
            case '"' -> out.append('"');
            case '\\' -> out.append('\\');
            case 'u' -> {
                if (pos + 4 > text.length()) {
                    throw error("bad unicode escape");
                }
                out.append((char) Integer.parseInt(text.substring(pos, pos + 4), 16));
                pos += 4;
            }
            default -> throw error("unsupported escape \\" + e);
        }
    }

    private void skipBlanks() {
        while (pos < text.length() && (peek() == ' ' || peek() == '\t')) {
            pos++;
        }
    }

    /** Skips blanks, newlines and comments. */
    private void skipTrivia() {
        while (pos < text.length()) {
            char c = peek();
            if (c == '#') {
                while (pos < text.length() && peek() != '\n') {
                    pos++;
                }
            } else if (c == '\n') {
                line++;
                pos++;
            } else if (Character.isWhitespace(c)) {
                pos++;
            } else {
                return;
            }
        }
    }

    private void endOfLine() {
        skipBlanks();
        if (pos < text.length() && peek() == '#') {
            while (pos < text.length() && peek() != '\n') {
                pos++;
            }
        }
        if (pos < text.length() && peek() == '\r') {
            pos++;
        }
        if (pos < text.length() && peek() != '\n') {
            throw error("unexpected text after value");
        }
    }

    private void expect(char c) {
        if (pos >= text.length() || peek() != c) {
            throw error("expected `" + c + "`");
        }
        pos++;
    }

    private char peek() {
        return pos < text.length() ? text.charAt(pos) : '\0';
    }

    private IllegalArgumentException error(String message) {
        return new IllegalArgumentException("line " + line + ": " + message);
    }
}
