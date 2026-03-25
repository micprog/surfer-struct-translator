//! Meta-configuration for the struct translator plugin.
//!
//! Supports two modes:
//! 1. Pre-generated: load a struct_defs.toml file directly
//! 2. Source-based: parse SystemVerilog sources on the fly using slang

use serde::Deserialize;

/// Top-level plugin configuration (struct_config.toml).
#[derive(Deserialize)]
pub struct MetaConfig {
    /// Path to a pre-generated struct_defs.toml (relative to config dir).
    pub struct_defs_file: Option<String>,
    /// Source-based configuration for on-the-fly generation.
    pub sources: Option<SourcesConfig>,
}

/// Configuration for parsing SystemVerilog sources.
#[derive(Deserialize)]
pub struct SourcesConfig {
    /// Verilator-style file lists (.f files).
    #[serde(default)]
    pub flist: Vec<String>,
    /// Individual source files.
    #[serde(default)]
    pub files: Vec<String>,
    /// Include directories.
    #[serde(default)]
    pub includes: Vec<String>,
    /// Preprocessor defines.
    #[serde(default)]
    pub defines: Vec<String>,
    /// Top module name(s) for elaboration.
    #[serde(default)]
    pub top_modules: Vec<String>,
    /// Parameter overrides (e.g. "NrLanes=4").
    #[serde(default)]
    pub param_overrides: Vec<String>,
    /// Only include types annotated with /* public */.
    #[serde(default)]
    pub public_only: bool,
    /// Automatically generate per-signal mappings from elaborated hierarchy.
    #[serde(default = "default_true")]
    pub auto_map: bool,
    /// Manual signal-to-type mappings ("pattern=struct_type").
    #[serde(default)]
    pub mappings: Vec<String>,
}

fn default_true() -> bool {
    true
}
