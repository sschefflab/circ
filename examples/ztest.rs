/// Test discovery for ZoKratesCurly programs.
/// Reads a .zok file, finds every function marked with a @test annotation,
/// and prints each one's name and inputs. It does NOT run, compile, or
/// check anything — discovery and printing only.

use circ::cfg::{clap, CircOpt};
use clap::Parser;
use std::path::PathBuf;
use zokrates_curly_pest_ast::SymbolDeclaration;

#[derive(Debug, Parser)]
#[command(name = "ztest", about = "List @test functions in a ZoKratesCurly program")]
struct Options {
    /// Input file
    #[arg(name = "PATH")]
    path: PathBuf,

    #[command(flatten)]
    circ: CircOpt,
}

fn main() {
    env_logger::Builder::from_default_env()
        .format_level(false)
        .format_timestamp(None)
        .init();

    let options = Options::parse();
    circ::cfg::set(&options.circ);

    // Read the whole source file into a string; the parser borrows from it.
    let source = match std::fs::read_to_string(&options.path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", options.path.display(), e);
            std::process::exit(1);
        }
    };

    // Parse the source into an AST. No type checking happens here.
    let file = match zokrates_curly_pest_ast::generate_ast(&source) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Parse error in {}:\n{}", options.path.display(), e);
            std::process::exit(1);
        }
    };

    // Walk the top-level declarations, keeping only functions marked @test.
    for decl in &file.declarations {
        // Only function declarations can carry a @test annotation.
        let SymbolDeclaration::Function(f) = decl else {
            continue;
        };

        // `f.test` is Some(..) only when the function was annotated @test.
        let Some(test) = &f.test else {
            continue;
        };

        // Print each input as written in the source (no evaluation):
        // `span().as_str()` gives the literal text, e.g. `3 * 3`.
        let inputs: Vec<String> = test
            .inputs
            .iter()
            .map(|input| format!("{} = {}", input.name.value, input.value.span().as_str()))
            .collect();

        if inputs.is_empty() {
            println!("found {} with no inputs", f.id.value);
        } else {
            println!("found {} with {}", f.id.value, inputs.join(", "));
        }
    }
}
