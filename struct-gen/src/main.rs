use std::io::Write;
use std::path::PathBuf;

use clap::Parser;

use surfer_struct_gen::{GenerateOpts, collect_sources, generate_struct_defs};

/// Generate struct_defs.toml for the surfer-struct-translator WASM plugin.
///
/// Parses and elaborates SystemVerilog sources using slang, extracts all packed
/// struct and enum type definitions, and writes a TOML configuration file that
/// the surfer-struct-translator plugin can consume.
///
/// Supports Verilator-style file lists (-f) with +incdir+ and +define+ directives.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// SystemVerilog source files to parse (also accepts +incdir+ and +define+ prefixed args).
    files: Vec<String>,

    /// Read a Verilator-style file list (.f / flist).
    #[arg(short = 'f', long = "flist")]
    flist: Vec<PathBuf>,

    /// Include directories (passed to the preprocessor).
    #[arg(short = 'I', long = "include")]
    includes: Vec<String>,

    /// Preprocessor defines (e.g. -DFOO=1 or +define+FOO=1).
    #[arg(short = 'D', long = "define")]
    defines: Vec<String>,

    /// Top module name(s) for elaboration.
    #[arg(long = "top")]
    top_modules: Vec<String>,

    /// Output file path [default: stdout].
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Only include types annotated with /* public */.
    #[arg(long)]
    public_only: bool,

    /// Generate mapping entries (glob pattern like "*axi_req*=axi_req_t").
    #[arg(short, long = "map")]
    mappings: Vec<String>,

    /// Automatically generate mappings for every extracted struct type.
    #[arg(long)]
    auto_map: bool,

    /// Top-level parameter overrides (e.g. -GNrLanes=4).
    #[arg(short = 'G', value_name = "NAME=VALUE")]
    param_overrides: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    let (files, includes, defines) =
        match collect_sources(&cli.files, &cli.flist, &cli.includes, &cli.defines) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };

    if files.is_empty() {
        eprintln!("Error: no source files specified");
        std::process::exit(1);
    }

    eprintln!("Parsing {} source files...", files.len());

    let opts = GenerateOpts {
        files: &files,
        includes: &includes,
        defines: &defines,
        top_modules: &cli.top_modules,
        param_overrides: &cli.param_overrides,
        public_only: cli.public_only,
        auto_map: cli.auto_map,
        manual_mappings: &cli.mappings,
    };

    let toml_output = match generate_struct_defs(&opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    // Write output.
    if let Some(path) = cli.output {
        match std::fs::File::create(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(toml_output.as_bytes()) {
                    eprintln!("Error writing to {}: {e}", path.display());
                    std::process::exit(1);
                }
                eprintln!("Wrote {}", path.display());
            }
            Err(e) => {
                eprintln!("Error creating {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    } else {
        print!("{toml_output}");
    }
}
