//! On-the-fly generation of struct definitions from SystemVerilog sources.
//!
//! This feature requires the slang C++ library and is only available on WASI targets.

#[cfg(target_os = "wasi")]
use std::path::Path;

use crate::config::Config;
use crate::meta_config::SourcesConfig;

/// Optional hierarchy info from the waveform file, used to infer
/// root prefix and top module when not explicitly configured.
pub struct HierarchyHints {
    /// Root scope prefix (e.g. "TOP").
    pub root_prefix: String,
    /// Component (module) names for root scopes, used as fallback top modules.
    pub top_modules: Vec<String>,
}

/// Generate a Config by parsing SystemVerilog sources using slang.
///
/// `config_dir` is the directory containing struct_config.toml, used to
/// resolve relative paths in the source configuration.
///
/// `hints` provides optional hierarchy info from the waveform file for
/// inferring root prefix and top module names.
#[cfg(target_os = "wasi")]
pub fn generate_from_sources(
    sources: &SourcesConfig,
    config_dir: &str,
    hints: Option<&HierarchyHints>,
) -> Result<Config, String> {
    // Resolve flist paths relative to config_dir.
    let flist_paths: Vec<String> = sources
        .flist
        .iter()
        .map(|f| resolve_path(f, config_dir))
        .collect();
    let flist_as_paths: Vec<&Path> = flist_paths.iter().map(|p| Path::new(p.as_str())).collect();

    // Resolve source file paths relative to config_dir.
    let file_args: Vec<String> = sources
        .files
        .iter()
        .map(|f| resolve_path(f, config_dir))
        .collect();

    // Resolve include paths relative to config_dir.
    let include_args: Vec<String> = sources
        .includes
        .iter()
        .map(|i| resolve_path(i, config_dir))
        .collect();

    // Collect all sources from file lists and direct arguments.
    let (files, includes, defines) = surfer_struct_gen::collect_sources(
        &file_args,
        &flist_as_paths,
        &include_args,
        &sources.defines,
    )?;

    if files.is_empty() {
        return Err("No source files found in configuration".to_string());
    }

    // Use configured top_modules, or fall back to hierarchy hints.
    let top_modules = if sources.top_modules.is_empty() {
        hints.map(|h| h.top_modules.clone()).unwrap_or_default()
    } else {
        sources.top_modules.clone()
    };

    // Use root prefix from hierarchy hints, or default to "TOP".
    let root_prefix = hints.map(|h| h.root_prefix.as_str()).unwrap_or("TOP");

    // Generate TOML string from sources.
    let opts = surfer_struct_gen::GenerateOpts {
        files: &files,
        includes: &includes,
        defines: &defines,
        top_modules: &top_modules,
        param_overrides: &sources.param_overrides,
        public_only: sources.public_only,
        auto_map: sources.auto_map,
        manual_mappings: &sources.mappings,
        root_prefix,
    };

    let toml_string = surfer_struct_gen::generate_struct_defs(&opts)?;
    Config::from_toml(&toml_string).map_err(|e| format!("Failed to parse generated TOML: {e}"))
}

#[cfg(not(target_os = "wasi"))]
pub fn generate_from_sources(
    _sources: &SourcesConfig,
    _config_dir: &str,
    _hints: Option<&HierarchyHints>,
) -> Result<Config, String> {
    Err("On-the-fly SystemVerilog parsing is not supported on this platform (requires WASI). Use a pre-generated struct_defs.toml instead.".to_string())
}

/// Resolve a path relative to a base directory, unless it's already absolute.
#[cfg(target_os = "wasi")]
fn resolve_path(path: &str, base_dir: &str) -> String {
    if Path::new(path).is_absolute() {
        path.to_string()
    } else {
        format!("{base_dir}/{path}")
    }
}
