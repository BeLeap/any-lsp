use any_lsp::{run_stdio, LspServer};
use std::env;
use std::path::PathBuf;

fn print_help() {
    println!("any-lsp - a language-agnostic LSP server powered by ripgrep");
    println!();
    println!("Usage: any-lsp [--root PATH]");
    println!();
    println!("The server communicates over stdin/stdout using LSP Content-Length framing.");
}

fn main() {
    let mut root = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            }
            "--root" => {
                root = args.next().map(PathBuf::from);
            }
            value if value.starts_with('-') => {
                eprintln!("unknown option: {value}");
                std::process::exit(2);
            }
            value => {
                root = Some(PathBuf::from(value));
            }
        }
    }
    if let Err(error) = run_stdio(&mut LspServer::new(root)) {
        eprintln!("any-lsp: {error}");
        std::process::exit(1);
    }
}
