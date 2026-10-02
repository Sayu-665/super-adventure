import org.anarres.cpp.*;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.stream.Collectors;

/**
 * Reference implementation for sb-preprocess's differential tests: reproduces
 * Iris 1.11's preprocessing on top of the real JCPP 1.4.14 library Iris ships.
 *
 * <ul>
 *   <li>GLSL: include expansion as in Iris's IncludeGraph / FileNode /
 *       IncludeProcessor (lines split with {@code \R}, any trimmed line starting
 *       with {@code #include} is an include, {@code ..} clamped at the root),
 *       followed by {@code JcppProcessor.glslPreprocessSource} (the
 *       {@code #version}/{@code #extension} marker hack, NUL removal,
 *       KEEPCOMMENTS).</li>
 *   <li>Properties: {@code PropertiesPreprocessor.process} including the line
 *       prefilter and the backslash marker.</li>
 * </ul>
 *
 * Modes:
 * <pre>
 *   glsl  &lt;shadersRoot&gt; &lt;outDir&gt; &lt;definesFile&gt;   (program paths on stdin)
 *   cases &lt;inDir&gt; &lt;outDir&gt; &lt;definesFile&gt;         (every *.glsl file in inDir)
 *   props &lt;listFile&gt; &lt;outDir&gt; &lt;definesFile&gt;      (lines "id|dimensionVariant(0/1)|path")
 * </pre>
 * GLSL results are written as {@code <out>.hdr} (hoisted lines), {@code <out>.body}
 * (JCPP output) and {@code <out>.diag}; a program Iris cannot load gets
 * {@code <out>.fail}. Defines files hold one {@code NAME} or {@code NAME=VALUE} per line.
 */
public class IrisHarness {
    static final String VERSION_MARKER = "#warning IRIS_JCPP_GLSL_VERSION";
    static final String EXTENSION_MARKER = "#warning IRIS_JCPP_GLSL_EXTENSION";

    static class GlslListener extends DefaultPreprocessorListener {
        final StringBuilder hdr = new StringBuilder();
        final StringBuilder diags = new StringBuilder();

        @Override
        public void handleWarning(Source source, int line, int column, String msg) {
            if (msg.startsWith(VERSION_MARKER)) {
                hdr.append(msg.replace(VERSION_MARKER, "#version ")).append('\n');
            } else if (msg.startsWith(EXTENSION_MARKER)) {
                hdr.append(msg.replace(EXTENSION_MARKER, "#extension ")).append('\n');
            } else {
                diags.append("warning:").append(line).append(':').append(msg.replace('\n', ' ')).append('\n');
            }
        }

        @Override
        public void handleError(Source source, int line, int column, String msg) {
            diags.append("error:").append(line).append(':').append(msg.replace('\n', ' ')).append('\n');
        }
    }

    static class PropsListener extends DefaultPreprocessorListener {
        @Override
        public void handleWarning(Source source, int line, int column, String msg) {
        }

        @Override
        public void handleError(Source source, int line, int column, String msg) {
        }
    }

    static Path root;
    static final Map<String, String[]> cache = new HashMap<>();

    static String[] lines(String abs) throws IOException {
        if (cache.containsKey(abs)) return cache.get(abs);
        Path p = root.resolve(abs.substring(1));
        String[] l = Files.isRegularFile(p) ? Files.readString(p).split("\\R") : null;
        cache.put(abs, l);
        return l;
    }

    /** AbsolutePackPath.normalizeAbsolutePath. */
    static String norm(String path) {
        List<String> out = new ArrayList<>();
        for (String s : path.split("/")) {
            if (s.isEmpty() || s.equals(".")) continue;
            if (s.equals("..")) {
                if (!out.isEmpty()) out.remove(out.size() - 1);
            } else {
                out.add(s);
            }
        }
        if (out.isEmpty()) return "/";
        StringBuilder b = new StringBuilder();
        for (String s : out) b.append('/').append(s);
        return b.toString();
    }

    /** Returns null on success, else why Iris cannot load the program. */
    static String expand(String abs, Deque<String> stack, StringBuilder out) throws IOException {
        String[] ls = lines(abs);
        if (ls == null) return "missing " + abs;
        if (stack.contains(abs)) return "cycle " + abs;
        stack.push(abs);
        String dir = abs.substring(0, abs.lastIndexOf('/'));
        for (String line : ls) {
            String t = line.trim();
            if (t.startsWith("#include")) {
                if (t.length() < 9) return "bad include in " + abs;
                String target = t.substring(9).trim();
                if (target.startsWith("\"")) target = target.substring(1);
                if (target.endsWith("\"")) target = target.substring(0, target.length() - 1);
                String resolved = target.startsWith("/") ? norm(target) : norm(dir + "/" + target);
                String r = expand(resolved, stack, out);
                if (r != null) return r;
            } else {
                out.append(line).append('\n');
            }
        }
        stack.pop();
        return null;
    }

    static void glslPreprocess(String source, List<String[]> defines, Path outBase) throws IOException {
        Files.createDirectories(outBase.toAbsolutePath().getParent());
        if (source.contains(VERSION_MARKER) || source.contains(EXTENSION_MARKER)) {
            Files.writeString(Paths.get(outBase + ".fail"), "marker");
            return;
        }
        source = source.replace("#version", VERSION_MARKER);
        source = source.replace("#extension", EXTENSION_MARKER);
        source = source.replace("\u0000", "");
        GlslListener listener = new GlslListener();
        StringBuilder builder = new StringBuilder();
        try {
            Preprocessor pp = new Preprocessor();
            for (String[] d : defines) pp.addMacro(d[0], d[1]);
            pp.setListener(listener);
            pp.addInput(new StringLexerSource(source, true));
            pp.addFeature(Feature.KEEPCOMMENTS);
            for (; ; ) {
                Token tok = pp.token();
                if (tok == null || tok.getType() == Token.EOF) break;
                builder.append(tok.getText());
            }
        } catch (Exception e) {
            Files.writeString(Paths.get(outBase + ".fail"), "exception " + e);
            return;
        }
        builder.append("\n");
        Files.writeString(Paths.get(outBase + ".hdr"), listener.hdr.toString());
        Files.writeString(Paths.get(outBase + ".body"), builder.toString());
        Files.writeString(Paths.get(outBase + ".diag"), listener.diags.toString());
    }

    static String propsPreprocess(String source, List<String[]> defines, boolean dimensionVariant) throws Exception {
        Preprocessor pp = new Preprocessor();
        for (String[] d : defines) {
            // PropertiesPreprocessor.preprocessSource(source, options, env) defines empty
            // values as "1"; the dimension.properties overload keeps them empty.
            if (!dimensionVariant && d[1].isEmpty()) pp.addMacro(d[0]);
            else pp.addMacro(d[0], d[1]);
        }
        pp.setListener(new PropsListener());
        source = Arrays.stream(source.split("\\R")).map(String::trim).filter(s -> !s.isBlank())
            .map(line -> {
                if (line.startsWith("#")) {
                    for (PreprocessorCommand command : PreprocessorCommand.values()) {
                        if (line.startsWith("#" + (command.name().replace("PP_", "").toLowerCase(Locale.ROOT)))) {
                            return line;
                        }
                    }
                    return "";
                }
                return line.replace("#", "");
            }).collect(Collectors.joining("\n")) + "\n";
        source = source.replace("\\", "IRIS_PASSTHROUGHBACKSLASH");
        pp.addInput(new StringLexerSource(source, true));
        pp.addFeature(Feature.KEEPCOMMENTS);
        StringBuilder builder = new StringBuilder();
        try {
            for (; ; ) {
                Token tok = pp.token();
                if (tok == null || tok.getType() == Token.EOF) break;
                builder.append(tok.getText());
            }
        } catch (Exception e) {
            // Iris logs "Properties pre-processing failed" and keeps the partial output.
        }
        return builder.toString().replace("IRIS_PASSTHROUGHBACKSLASH", "\\");
    }

    static List<String[]> readDefines(String file) throws IOException {
        List<String[]> d = new ArrayList<>();
        for (String l : Files.readAllLines(Paths.get(file))) {
            if (l.isEmpty()) continue;
            int eq = l.indexOf('=');
            d.add(eq < 0 ? new String[]{l, ""} : new String[]{l.substring(0, eq), l.substring(eq + 1)});
        }
        return d;
    }

    public static void main(String[] args) throws Exception {
        String mode = args[0];
        List<String[]> defines = readDefines(args[3]);
        Path outDir = Paths.get(args[2]);
        switch (mode) {
            case "glsl" -> {
                root = Paths.get(args[1]);
                BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
                String prog;
                while ((prog = in.readLine()) != null) {
                    if (prog.isEmpty()) continue;
                    Path base = outDir.resolve(prog);
                    StringBuilder sb = new StringBuilder();
                    String r;
                    try {
                        r = expand("/" + prog, new ArrayDeque<>(), sb);
                    } catch (Exception e) {
                        r = "exception " + e;
                    }
                    if (r != null) {
                        Files.createDirectories(base.toAbsolutePath().getParent());
                        Files.writeString(Paths.get(base + ".fail"), r);
                        continue;
                    }
                    glslPreprocess(sb.toString(), defines, base);
                }
            }
            case "cases" -> {
                try (DirectoryStream<Path> ds = Files.newDirectoryStream(Paths.get(args[1]), "*.glsl")) {
                    for (Path p : ds) {
                        String name = p.getFileName().toString().replaceAll("\\.glsl$", "");
                        // IncludeGraph splits with \R; IncludeProcessor re-joins with '\n'.
                        StringBuilder sb = new StringBuilder();
                        for (String line : Files.readString(p).split("\\R")) sb.append(line).append('\n');
                        glslPreprocess(sb.toString(), defines, outDir.resolve(name));
                    }
                }
            }
            case "props" -> {
                Files.createDirectories(outDir);
                for (String l : Files.readAllLines(Paths.get(args[1]))) {
                    if (l.isEmpty()) continue;
                    String[] parts = l.split("\\|", 3);
                    String text = Files.readString(Paths.get(parts[2]), StandardCharsets.ISO_8859_1);
                    String out = propsPreprocess(text, defines, parts[1].equals("1"));
                    Files.writeString(outDir.resolve(parts[0] + ".out"), out, StandardCharsets.ISO_8859_1);
                }
            }
            default -> throw new IllegalArgumentException("unknown mode " + mode);
        }
    }
}
