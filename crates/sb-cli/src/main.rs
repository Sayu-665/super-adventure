//! `shaderbridge` binary: see the `sb_cli` library for the commands.

fn main() {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = sb_cli::run(std::env::args(), &mut stdout.lock(), &mut stderr.lock());
    std::process::exit(code);
}
