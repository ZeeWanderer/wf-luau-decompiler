use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Dialect {
    Warframe,
    Luau,
}

#[derive(Parser, Debug)]
#[command(name = "wf-luau-decompiler", version, about)]
struct Cli {
    /// Normalized bytecode file, or '-' for standard input.
    #[arg(default_value = "-")]
    input: PathBuf,

    /// Input bytecode dialect.
    #[arg(long, value_enum, default_value_t = Dialect::Warframe)]
    dialect: Dialect,

    /// Luau opcode decode key. Ignored for normalized Warframe bytecode.
    #[arg(long, default_value_t = 1)]
    decode_key: u8,

    /// Optional source name used in reconstructed output.
    #[arg(long)]
    script_name: Option<String>,

    /// Emit a versioned JSON result record to standard error.
    #[arg(long, value_name = "FORMAT", value_parser = ["json"])]
    diagnostics: Option<String>,

    #[arg(long)]
    dont_reuse_var: bool,

    #[arg(long)]
    no_synth_helpers: bool,

    #[arg(long)]
    assume_no_nan: bool,
}

#[derive(Serialize)]
struct Diagnostic<'a> {
    protocol: u8,
    tool: &'static str,
    version: &'static str,
    ok: bool,
    dialect: &'a str,
    input_bytes: usize,
    elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

fn main() {
    luau_lifter::install_quiet_panic_hook();
    let cli = Cli::parse();
    let started = Instant::now();
    let dialect = match cli.dialect {
        Dialect::Warframe => "warframe",
        Dialect::Luau => "luau",
    };

    let bytecode = match read_input(&cli.input) {
        Ok(bytecode) => bytecode,
        Err(error) => exit_error(&cli, dialect, 0, started, &error),
    };
    let options = luau_lifter::DecompileOptions {
        dont_reuse_var: cli.dont_reuse_var,
        no_synth_helpers: cli.no_synth_helpers,
        assume_no_nan: cli.assume_no_nan,
    };
    let result = match cli.dialect {
        Dialect::Warframe => luau_lifter::try_decompile_warframe_bytecode_with_options(
            &bytecode,
            cli.script_name.as_deref(),
            options,
        ),
        Dialect::Luau => luau_lifter::try_decompile_bytecode_with_options(
            &bytecode,
            cli.decode_key,
            cli.script_name.as_deref(),
            options,
        ),
    };

    match result {
        Ok(source) => {
            print!("{source}");
            emit_diagnostic(&cli, dialect, bytecode.len(), started, None);
        }
        Err(error) => exit_error(&cli, dialect, bytecode.len(), started, &error),
    }
}

fn read_input(path: &PathBuf) -> Result<Vec<u8>, String> {
    if path.as_os_str() == "-" {
        let mut bytecode = Vec::new();
        io::stdin()
            .read_to_end(&mut bytecode)
            .map_err(|error| format!("could not read standard input: {error}"))?;
        Ok(bytecode)
    } else {
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))
    }
}

fn exit_error(cli: &Cli, dialect: &str, input_bytes: usize, started: Instant, error: &str) -> ! {
    if cli.diagnostics.is_some() {
        emit_diagnostic(cli, dialect, input_bytes, started, Some(error));
    } else {
        eprintln!("error: {error}");
    }
    std::process::exit(1)
}

fn emit_diagnostic(
    cli: &Cli,
    dialect: &str,
    input_bytes: usize,
    started: Instant,
    error: Option<&str>,
) {
    if cli.diagnostics.is_none() {
        return;
    }
    let diagnostic = Diagnostic {
        protocol: 1,
        tool: "wf-luau-decompiler",
        version: env!("CARGO_PKG_VERSION"),
        ok: error.is_none(),
        dialect,
        input_bytes,
        elapsed_ms: started.elapsed().as_millis(),
        error,
    };
    match serde_json::to_string(&diagnostic) {
        Ok(json) => eprintln!("{json}"),
        Err(serialization_error) => {
            eprintln!("error: could not serialize diagnostics: {serialization_error}")
        }
    }
}
