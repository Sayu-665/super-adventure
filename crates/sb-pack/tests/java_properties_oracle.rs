//! Differential test of [`sb_pack::properties::parse`] against the real
//! `java.util.Properties.load` (the parser Iris uses for every `.properties` file).
//!
//! Thousands of mutated inputs are written to a temporary directory, loaded by a small
//! Java program (run with the JDK's single-file source launcher) and compared entry by
//! entry. Skipped when no `java` executable (JDK 11+) is available.

use sb_pack::properties;
use std::path::Path;
use std::process::Command;

/// Loads every `case_*.txt` (UTF-8) with `Properties.load(Reader)` and writes the
/// entries, in first-insertion order, as hex-encoded UTF-8 (lone surrogates replaced
/// by U+FFFD, as the Rust parser does). Malformed `\uXXXX` escapes make Java throw;
/// such cases are written as `ERROR`.
const ORACLE: &str = r#"
import java.nio.file.*;
import java.util.*;
import java.io.*;

public class PropsOracle {
    public static void main(String[] a) throws Exception {
        try (var ds = Files.newDirectoryStream(Paths.get(a[0]), "case_*.txt")) {
            for (Path p : ds) {
                StringBuilder sb = new StringBuilder();
                try {
                    LinkedHashMap<String, String> m = new LinkedHashMap<>();
                    Properties pr = new Properties() {
                        @Override
                        public synchronized Object put(Object k, Object v) {
                            m.put((String) k, (String) v);
                            return super.put(k, v);
                        }
                    };
                    pr.load(new StringReader(Files.readString(p)));
                    for (var e : m.entrySet()) {
                        sb.append(hex(e.getKey())).append(' ').append(hex(e.getValue())).append('\n');
                    }
                } catch (IllegalArgumentException ex) {
                    sb.append("ERROR\n");
                }
                Files.writeString(Paths.get(p.toString().replace(".txt", ".out")), sb.toString());
            }
        }
    }

    static String hex(String s) {
        StringBuilder t = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (Character.isHighSurrogate(c) && i + 1 < s.length() && Character.isLowSurrogate(s.charAt(i + 1))) {
                t.append(c).append(s.charAt(i + 1));
                i++;
            } else if (Character.isSurrogate(c)) {
                t.append('�');
            } else {
                t.append(c);
            }
        }
        StringBuilder h = new StringBuilder("x");
        for (byte x : t.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8)) {
            h.append(String.format("%02x", x));
        }
        return h.toString();
    }
}
"#;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const SEEDS: &[&str] = &[
    "a=1\nb : 2\nc 3\n  # comment\n! bang\nd=x \\\n   y\ne\\=f=g\\:h\n",
    "key\\ with\\ space = v\\tal\\u00e9\n\\\n#x=1\nlast=\\",
    "x = \\\r\n  y\r\n\r\n#c\\\nz=\\u0041\\uD83D\\uDE00",
    "screen=<profile> [A] B *\nprofile.LOW=!B C=1\nblock.10=stone \\\n\n  dirt\n",
];

const FRAGMENTS: &[&str] = &[
    "\\", "\\\n", "\n", "\r", "\r\n", "#", "!", "=", ":", " ", "\t", "\u{c}", "\\u", "\\u00",
    "\\u0041", "\\uD800", "\\uDC00", "é", "😀", "\\t", "\\n", "\\\\", "  ", "k", "v", "\\ ",
];

fn mutate(rng: &mut Rng, seed: &str) -> String {
    let mut s: Vec<char> = seed.chars().collect();
    for _ in 0..1 + rng.below(6) {
        match rng.below(4) {
            0 if !s.is_empty() => {
                let i = rng.below(s.len());
                s.remove(i);
            }
            _ => {
                let f = FRAGMENTS[rng.below(FRAGMENTS.len())];
                let i = rng.below(s.len() + 1);
                for (k, c) in f.chars().enumerate() {
                    s.insert(i + k, c);
                }
            }
        }
    }
    s.into_iter().collect()
}

fn hex(s: &str) -> String {
    let mut h = String::from("x");
    for b in s.as_bytes() {
        h.push_str(&format!("{b:02x}"));
    }
    h
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run_oracle(dir: &Path) -> bool {
    let src = dir.join("PropsOracle.java");
    std::fs::write(&src, ORACLE).unwrap();
    Command::new("java")
        .arg(&src)
        .arg(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn properties_parser_matches_java() {
    if !java_available() {
        eprintln!("no `java` executable; skipping the java.util.Properties oracle");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut rng = Rng(0x5EED_1234_ABCD_0042);
    let mut cases: Vec<String> = SEEDS.iter().map(|s| s.to_string()).collect();
    while cases.len() < 3000 {
        let seed = SEEDS[cases.len() % SEEDS.len()];
        cases.push(mutate(&mut rng, seed));
    }
    for (i, text) in cases.iter().enumerate() {
        std::fs::write(tmp.path().join(format!("case_{i}.txt")), text).unwrap();
    }
    if !run_oracle(tmp.path()) {
        eprintln!("the Java oracle could not run (needs JDK 11+); skipping");
        return;
    }
    let mut compared = 0;
    let mut failures = Vec::new();
    for (i, text) in cases.iter().enumerate() {
        let java = std::fs::read_to_string(tmp.path().join(format!("case_{i}.out"))).unwrap();
        if java == "ERROR\n" {
            // Java rejects malformed \uXXXX escapes; the Rust parser keeps them literally
            // and reports a diagnostic instead.
            let (_, diags) = properties::parse_with_diagnostics(text, "x.properties");
            assert!(!diags.is_empty(), "case {i}: {text:?} should be diagnosed");
            continue;
        }
        compared += 1;
        let ours: String = properties::parse(text)
            .iter()
            .map(|e| format!("{} {}\n", hex(&e.key), hex(&e.value)))
            .collect();
        if ours != java {
            failures.push(format!("{text:?}\n  java: {java:?}\n  ours: {ours:?}"));
        }
    }
    assert!(compared > 1000, "too few comparable cases ({compared})");
    assert!(
        failures.is_empty(),
        "{} of {compared} cases differ from Java:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
