//! Library for generating struct_defs.toml from SystemVerilog sources using slang.

pub mod dedup;
pub mod flist;
pub mod slang;
pub mod toml_gen;
pub mod types;

use std::collections::HashMap;
use std::path::Path;

use dedup::{UniqueEnum, UniqueStruct, deduplicate_enums, deduplicate_structs};
use flist::FlistContents;
use toml_gen::MappingEntry;
use types::{ReflectedData, total_elements};

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
    /// Root scope prefix for signal paths (e.g. "TOP"). Empty uses "TOP".
    pub root_prefix: &'a str,
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
    let reflected_json = session.reflect_types(
        opts.public_only,
        opts.top_modules,
        opts.param_overrides,
        opts.root_prefix,
    )?;

    let data: ReflectedData = serde_json::from_str(&reflected_json)
        .map_err(|e| format!("Error parsing reflected JSON: {e}"))?;

    // Deduplicate parameterized types.
    let structs = deduplicate_structs(&data.structs);
    let enums = deduplicate_enums(&data.enums);

    // Build mappings.
    let mappings = build_mappings(
        &structs,
        &enums,
        &data.signal_mappings,
        opts.auto_map,
        opts.manual_mappings,
    );

    Ok(toml_gen::generate_toml(&structs, &enums, &mappings))
}

/// Build signal-to-struct mapping entries from manual mappings and auto-map data.
fn build_mappings(
    structs: &[UniqueStruct],
    enums: &[UniqueEnum],
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
                struct_type: Some(struct_type.trim().to_string()),
                enum_type: None,
                width: None,
                array_dims: vec![],
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
        let enum_to_keys: HashMap<&str, Vec<&UniqueEnum>> = {
            let mut m: HashMap<&str, Vec<&UniqueEnum>> = HashMap::new();
            for e in enums {
                m.entry(&e.inner.name).or_default().push(e);
            }
            m
        };

        if signal_mappings.is_empty() {
            // Fallback: no instance data, use type names as patterns.
            for s in structs {
                let pattern = format!("*{}*", s.sv_name);
                if !all_mappings
                    .iter()
                    .any(|m| m.struct_type.as_deref() == Some(s.key.as_str()))
                {
                    all_mappings.push(MappingEntry {
                        pattern,
                        struct_type: Some(s.key.clone()),
                        enum_type: None,
                        width: None,
                        array_dims: vec![],
                    });
                }
            }
        } else {
            // Use exact hierarchical paths from the elaborated design.
            for sm in signal_mappings {
                let Some(candidates) = type_to_keys.get(sm.type_name.as_str()) else {
                    if sm.kind == "enum" {
                        let Some(enum_candidates) = enum_to_keys.get(sm.type_name.as_str()) else {
                            continue;
                        };
                        let total_elems = total_elements(&sm.array_dims);
                        let elem_width = if total_elems > 1 {
                            sm.width / total_elems
                        } else {
                            sm.width
                        };
                        let key = if enum_candidates.len() == 1 {
                            &enum_candidates[0].key
                        } else if let Some(e) =
                            enum_candidates.iter().find(|e| e.inner.width == elem_width)
                        {
                            &e.key
                        } else {
                            continue;
                        };
                        all_mappings.push(MappingEntry {
                            pattern: sm.path.clone(),
                            struct_type: None,
                            enum_type: Some(key.clone()),
                            width: None,
                            array_dims: sm.array_dims.clone(),
                        });
                        continue;
                    }
                    if sm.kind == "scalar" && !sm.array_dims.is_empty() {
                        let total_elems = total_elements(&sm.array_dims);
                        let elem_width = if total_elems > 1 {
                            sm.width / total_elems
                        } else {
                            sm.width
                        };
                        all_mappings.push(MappingEntry {
                            pattern: sm.path.clone(),
                            struct_type: None,
                            enum_type: None,
                            width: Some(elem_width),
                            array_dims: sm.array_dims.clone(),
                        });
                    }
                    continue;
                };
                let total_elems = total_elements(&sm.array_dims);
                let elem_width = if total_elems > 1 {
                    sm.width / total_elems
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
                    struct_type: Some(key.clone()),
                    enum_type: None,
                    width: None,
                    array_dims: sm.array_dims.clone(),
                });
            }
        }
    }

    all_mappings
}

#[cfg(test)]
mod tests {
    use super::{GenerateOpts, generate_struct_defs};

    fn fixture_path(name: &str) -> String {
        format!("{}/test/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn generates_array_dims_for_packed_scalar_subvectors() {
        let file = fixture_path("packed_dims.sv");
        let files = vec![file];
        let tops = vec!["packed_dims_top".to_string()];
        let opts = GenerateOpts {
            files: &files,
            includes: &[],
            defines: &[],
            top_modules: &tops,
            param_overrides: &[],
            public_only: true,
            auto_map: true,
            manual_mappings: &[],
            root_prefix: "TOP",
        };

        let toml = generate_struct_defs(&opts).expect("generate struct defs");

        assert!(toml.contains("[structs.packed_dims_t]"));
        assert!(toml.contains("name = \"data_q\"\nwidth = 2\narray_size = 3\n"));
        assert!(toml.contains("name = \"state_q\"\nwidth = 2\n"));
        assert!(toml.contains("pattern = \"TOP.packed_dims_top.payload_q\""));
        assert!(toml.contains("struct_type = \"packed_dims_t\""));
        assert!(toml.contains("num_bits = 8"));
    }

    #[test]
    fn generates_array_dims_for_parameterized_packed_scalar_subvectors() {
        let file = fixture_path("packed_dims_param.sv");
        let files = vec![file];
        let tops = vec!["packed_dims_param_top".to_string()];
        let opts = GenerateOpts {
            files: &files,
            includes: &[],
            defines: &[],
            top_modules: &tops,
            param_overrides: &[],
            public_only: true,
            auto_map: true,
            manual_mappings: &[],
            root_prefix: "TOP",
        };

        let toml = generate_struct_defs(&opts).expect("generate struct defs");

        assert!(toml.contains("[structs.sequencer_probe_t]"));
        assert!(toml.contains("name = \"insn_queue_cnt_q\"\nwidth = 4\narray_size = 7\n"));
        assert!(toml.contains("name = \"insn_queue_done\"\nwidth = 7\n"));
        assert!(toml.contains("pattern = \"TOP.packed_dims_param_top.probe_q\""));
        assert!(toml.contains("struct_type = \"sequencer_probe_t\""));
        assert!(toml.contains("num_bits = 35"));
    }

    #[test]
    fn generates_mappings_for_plain_packed_scalar_signals() {
        let file = fixture_path("plain_signal_dims.sv");
        let files = vec![file];
        let tops = vec!["plain_signal_dims_top".to_string()];
        let opts = GenerateOpts {
            files: &files,
            includes: &[],
            defines: &[],
            top_modules: &tops,
            param_overrides: &[],
            public_only: false,
            auto_map: true,
            manual_mappings: &[],
            root_prefix: "TOP",
        };

        let toml = generate_struct_defs(&opts).expect("generate struct defs");

        assert!(toml.contains("[[mappings]]"));
        assert!(toml.contains("pattern = \"TOP.plain_signal_dims_top.data_q\""));
        assert!(toml.contains("width = 2"));
        assert!(toml.contains("num_bits = 6"));
        assert!(toml.contains("array_size = 3"));
    }
}
