//! Library for generating struct_defs.toml from SystemVerilog sources using slang.

pub mod dedup;
pub mod flist;
pub mod slang;
pub mod toml_gen;
pub mod types;

use std::collections::HashMap;
use std::path::Path;

use dedup::{UniqueStruct, deduplicate_enums, deduplicate_structs};
use flist::FlistContents;
use toml_gen::MappingEntry;
use types::ReflectedData;

/// Options for generating struct definitions from SystemVerilog sources.
pub struct GenerateOpts<'a> {
    /// Source files to parse.
    pub files: &'a [String],
    /// Include directories.
    pub includes: &'a [String],
    /// Preprocessor defines.
    pub defines: &'a [String],
    /// Top module name(s) for elaboration.
    pub top_modules: &'a [String],
    /// Parameter overrides (e.g. "NrLanes=4").
    pub param_overrides: &'a [String],
    /// Only include types annotated with /* public */.
    pub public_only: bool,
    /// Automatically generate mappings from elaborated signal hierarchy.
    pub auto_map: bool,
    /// Manual signal-to-type mappings ("pattern=struct_type").
    pub manual_mappings: &'a [String],
}

/// Collect all source files, includes, and defines from file lists and direct arguments.
pub fn collect_sources(
    files: &[String],
    flist_paths: &[impl AsRef<Path>],
    includes: &[String],
    defines: &[String],
) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let mut all_files = Vec::new();
    let mut all_includes: Vec<String> = includes.to_vec();
    let mut all_defines: Vec<String> = defines.to_vec();

    // Classify positional args.
    for arg in files {
        flist::classify_arg(arg, &mut all_files, &mut all_includes, &mut all_defines);
    }

    // Parse file lists.
    for flist_path in flist_paths {
        let flist_path = flist_path.as_ref();
        let FlistContents {
            files: f,
            includes: i,
            defines: d,
        } = flist::parse_flist(flist_path)?;
        all_files.extend(f);
        all_includes.extend(i);
        all_defines.extend(d);
    }

    Ok((all_files, all_includes, all_defines))
}

/// Generate struct definitions TOML from SystemVerilog sources.
///
/// Parses sources with slang, reflects types, deduplicates, and returns a TOML string.
pub fn generate_struct_defs(opts: &GenerateOpts) -> Result<String, String> {
    if opts.files.is_empty() {
        return Err("No source files specified".to_string());
    }

    // Parse all source files with slang.
    let mut session = slang::SlangSession::new();
    session.parse_group(opts.files, opts.includes, opts.defines)?;

    // Reflect types.
    let reflected_json =
        session.reflect_types(opts.public_only, opts.top_modules, opts.param_overrides)?;

    let data: ReflectedData = serde_json::from_str(&reflected_json)
        .map_err(|e| format!("Error parsing reflected JSON: {e}"))?;

    // Deduplicate parameterized types.
    let structs = deduplicate_structs(&data.structs);
    let enums = deduplicate_enums(&data.enums);

    // Build mappings.
    let mappings = build_mappings(
        &structs,
        &data.signal_mappings,
        opts.auto_map,
        opts.manual_mappings,
    );

    Ok(toml_gen::generate_toml(&structs, &enums, &mappings))
}

/// Build signal-to-struct mapping entries from manual mappings and auto-map data.
fn build_mappings(
    structs: &[UniqueStruct],
    signal_mappings: &[types::ReflectedSignalMapping],
    auto_map: bool,
    manual_mappings: &[String],
) -> Vec<MappingEntry> {
    let mut all_mappings = Vec::new();

    // Manual mappings.
    for mapping in manual_mappings {
        if let Some((pattern, struct_type)) = mapping.split_once('=') {
            all_mappings.push(MappingEntry {
                pattern: pattern.trim().to_string(),
                struct_type: struct_type.trim().to_string(),
                array_size: 1,
            });
        } else {
            eprintln!(
                "Warning: ignoring malformed mapping '{mapping}' (expected 'pattern=struct_type')"
            );
        }
    }

    // Auto-generated mappings.
    if auto_map {
        let type_to_keys: HashMap<&str, Vec<&UniqueStruct>> = {
            let mut m: HashMap<&str, Vec<&UniqueStruct>> = HashMap::new();
            for s in structs {
                m.entry(&s.sv_name).or_default().push(s);
            }
            m
        };

        if signal_mappings.is_empty() {
            // Fallback: no instance data, use type names as patterns.
            for s in structs {
                let pattern = format!("*{}*", s.sv_name);
                if !all_mappings.iter().any(|m| m.struct_type == s.key) {
                    all_mappings.push(MappingEntry {
                        pattern,
                        struct_type: s.key.clone(),
                        array_size: 1,
                    });
                }
            }
        } else {
            // Use exact hierarchical paths from the elaborated design.
            for sm in signal_mappings {
                let Some(candidates) = type_to_keys.get(sm.type_name.as_str()) else {
                    continue;
                };
                let elem_width = if sm.array_size > 1 {
                    sm.width / sm.array_size
                } else {
                    sm.width
                };
                let key = if candidates.len() == 1 {
                    &candidates[0].key
                } else if let Some(s) = candidates.iter().find(|s| s.total_width == elem_width) {
                    &s.key
                } else {
                    continue;
                };
                all_mappings.push(MappingEntry {
                    pattern: sm.path.clone(),
                    struct_type: key.clone(),
                    array_size: sm.array_size,
                });
            }
        }
    }

    all_mappings
}
