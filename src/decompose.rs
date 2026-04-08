use crate::config::{Config, FieldDef, StructDef, total_elements};
use surfer_translation_types::{
    SubFieldTranslationResult, TranslationResult, ValueKind, ValueRepr, VariableInfo,
};

/// Build the hierarchical `VariableInfo` tree for a struct type, with optional array wrapping.
pub fn build_variable_info(struct_name: &str, array_dims: &[u32], config: &Config) -> VariableInfo {
    let Some(struct_def) = config.structs.get(struct_name) else {
        return VariableInfo::Bits;
    };
    let elem_info = build_variable_info_for_struct(struct_def, config);
    wrap_in_array_dims(elem_info, array_dims)
}

/// Build `VariableInfo` for a plain scalar or enum leaf signal with optional array wrapping.
pub fn build_leaf_variable_info(width: u32, array_dims: &[u32]) -> VariableInfo {
    let elem_info = if width == 1 {
        VariableInfo::Bool
    } else {
        VariableInfo::Bits
    };
    wrap_in_array_dims(elem_info, array_dims)
}

/// Wrap a `VariableInfo` in nested array dimensions (leftmost = outermost).
fn wrap_in_array_dims(inner: VariableInfo, dims: &[u32]) -> VariableInfo {
    dims.iter()
        .rev()
        .fold(inner, |acc, &dim| VariableInfo::Compound {
            subfields: (0..dim).map(|i| (format!("[{i}]"), acc.clone())).collect(),
        })
}

fn build_variable_info_for_struct(struct_def: &StructDef, config: &Config) -> VariableInfo {
    VariableInfo::Compound {
        subfields: struct_def
            .fields
            .iter()
            .map(|f| {
                let elem_info = if let Some(ref st) = f.struct_type {
                    config
                        .structs
                        .get(st)
                        .map(|s| build_variable_info_for_struct(s, config))
                        .unwrap_or(VariableInfo::Bits)
                } else if f.width == Some(1) {
                    VariableInfo::Bool
                } else {
                    VariableInfo::Bits
                };
                let info = wrap_in_array_dims(elem_info, &f.array_dims);
                (f.name.clone(), info)
            })
            .collect(),
    }
}

/// Decompose a binary string into a structured `TranslationResult` according to a struct definition.
/// When `array_dims` is non-empty, the bits are split into nested array elements.
pub fn decompose(
    binary_digits: &str,
    struct_name: &str,
    array_dims: &[u32],
    config: &Config,
) -> TranslationResult {
    let Some(struct_def) = config.structs.get(struct_name) else {
        return TranslationResult {
            val: ValueRepr::String(format!("unknown struct: {struct_name}")),
            subfields: vec![],
            kind: ValueKind::Warn,
        };
    };

    if array_dims.is_empty() {
        decompose_struct(binary_digits, struct_def, config)
    } else {
        let elem_width = config.struct_total_width(struct_name) as usize;
        decompose_array(binary_digits, array_dims, elem_width, struct_def, config)
    }
}

/// Decompose a plain scalar signal with optional array dimensions.
pub fn decompose_scalar(binary_digits: &str, width: u32, array_dims: &[u32]) -> TranslationResult {
    let empty_config = Config {
        structs: std::collections::HashMap::new(),
        enums: std::collections::HashMap::new(),
        mappings: vec![],
    };
    let field = FieldDef {
        name: String::new(),
        width: Some(width),
        struct_type: None,
        enum_type: None,
        array_dims: array_dims.to_vec(),
    };
    if array_dims.is_empty() {
        decompose_leaf_element(binary_digits, &field, &empty_config)
    } else {
        decompose_leaf_array(
            binary_digits,
            array_dims,
            width as usize,
            &field,
            &empty_config,
        )
    }
}

/// Decompose a plain enum signal with optional array dimensions.
pub fn decompose_enum(
    binary_digits: &str,
    enum_name: &str,
    array_dims: &[u32],
    config: &Config,
) -> TranslationResult {
    let width = config.enums.get(enum_name).map_or(0, |e| e.width);
    let field = FieldDef {
        name: String::new(),
        width: None,
        struct_type: None,
        enum_type: Some(enum_name.to_string()),
        array_dims: array_dims.to_vec(),
    };
    if array_dims.is_empty() {
        decompose_leaf_element(binary_digits, &field, config)
    } else {
        decompose_leaf_array(binary_digits, array_dims, width as usize, &field, config)
    }
}

/// Recursively decompose multi-dimensional array dimensions.
/// `dims[0]` is the outermost dimension; each element spans `inner_count * elem_width` bits.
fn decompose_array(
    bits: &str,
    dims: &[u32],
    elem_width: usize,
    struct_def: &StructDef,
    config: &Config,
) -> TranslationResult {
    let dim = dims[0] as usize;
    let remaining_dims = &dims[1..];
    let inner_count = total_elements(remaining_dims) as usize;
    let chunk_width = inner_count * elem_width;

    let subfields = (0..dim)
        .map(|i| {
            let offset = i * chunk_width;
            let slice = safe_slice(bits, offset, chunk_width);
            let result = if remaining_dims.is_empty() {
                decompose_struct(slice, struct_def, config)
            } else {
                decompose_array(slice, remaining_dims, elem_width, struct_def, config)
            };
            SubFieldTranslationResult {
                name: format!("[{i}]"),
                result,
            }
        })
        .collect();

    TranslationResult {
        val: ValueRepr::Struct,
        subfields,
        kind: ValueKind::Normal,
    }
}

fn decompose_struct(
    binary_digits: &str,
    struct_def: &StructDef,
    config: &Config,
) -> TranslationResult {
    let mut offset = 0;
    let subfields = struct_def
        .fields
        .iter()
        .map(|field| {
            let width = config.field_width(field) as usize;
            let bits = safe_slice(binary_digits, offset, width);
            offset += width;
            SubFieldTranslationResult {
                name: field.name.clone(),
                result: decompose_field(bits, field, config),
            }
        })
        .collect();

    TranslationResult {
        val: ValueRepr::Struct,
        subfields,
        kind: ValueKind::Normal,
    }
}

fn decompose_field(bits: &str, field: &FieldDef, config: &Config) -> TranslationResult {
    if let Some(ref st) = field.struct_type {
        // Nested struct (possibly arrayed): recurse
        if let Some(s) = config.structs.get(st) {
            if !field.array_dims.is_empty() {
                let elem_width = config.struct_total_width(st) as usize;
                return decompose_array(bits, &field.array_dims, elem_width, s, config);
            }
            return decompose_struct(bits, s, config);
        }
    }

    // For enum and leaf fields with array dims, decompose as array of elements.
    if !field.array_dims.is_empty() {
        let elem_width = if let Some(ref et) = field.enum_type {
            config.enums.get(et).map_or(0, |e| e.width) as usize
        } else {
            field.width.unwrap_or(0) as usize
        };
        return decompose_leaf_array(bits, &field.array_dims, elem_width, field, config);
    }

    decompose_leaf_element(bits, field, config)
}

/// Decompose a single leaf element (enum or scalar bits).
fn decompose_leaf_element(bits: &str, field: &FieldDef, config: &Config) -> TranslationResult {
    if let Some(ref et) = field.enum_type
        && let Some(enum_def) = config.enums.get(et)
    {
        return if let Some(name) = enum_def.values.get(bits) {
            TranslationResult {
                val: ValueRepr::String(name.clone()),
                subfields: vec![],
                kind: ValueKind::Normal,
            }
        } else {
            TranslationResult {
                val: ValueRepr::String(format!("?({bits})")),
                subfields: vec![],
                kind: ValueKind::Warn,
            }
        };
    }

    // Plain bits for surfer's sub-translators
    let width = field.width.unwrap_or(bits.len() as u32);
    TranslationResult {
        val: ValueRepr::Bits(width, bits.to_string()),
        subfields: vec![],
        kind: if bits.contains('x') || bits.contains('z') {
            ValueKind::Undef
        } else {
            ValueKind::Normal
        },
    }
}

/// Recursively decompose multi-dimensional leaf (enum/scalar) array dimensions.
fn decompose_leaf_array(
    bits: &str,
    dims: &[u32],
    elem_width: usize,
    field: &FieldDef,
    config: &Config,
) -> TranslationResult {
    let dim = dims[0] as usize;
    let remaining_dims = &dims[1..];
    let inner_count = total_elements(remaining_dims) as usize;
    let chunk_width = inner_count * elem_width;

    let subfields = (0..dim)
        .map(|i| {
            let offset = i * chunk_width;
            let slice = safe_slice(bits, offset, chunk_width);
            let result = if remaining_dims.is_empty() {
                decompose_leaf_element(slice, field, config)
            } else {
                decompose_leaf_array(slice, remaining_dims, elem_width, field, config)
            };
            SubFieldTranslationResult {
                name: format!("[{i}]"),
                result,
            }
        })
        .collect();

    TranslationResult {
        val: ValueRepr::Struct,
        subfields,
        kind: ValueKind::Normal,
    }
}

/// Safely slice a string by char offset and length, padding with 'x' if out of bounds.
fn safe_slice(s: &str, offset: usize, len: usize) -> &str {
    let chars = s.len();
    if offset >= chars {
        return "";
    }
    let end = (offset + len).min(chars);
    &s[offset..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        let toml = r#"
[enums.burst_t]
width = 2
values = { "00" = "FIXED", "01" = "INCR", "10" = "WRAP" }

[structs.inner_t]
[[structs.inner_t.fields]]
name = "addr"
width = 8
[[structs.inner_t.fields]]
name = "burst"
enum_type = "burst_t"

[structs.outer_t]
[[structs.outer_t.fields]]
name = "chan"
struct_type = "inner_t"
[[structs.outer_t.fields]]
name = "valid"
width = 1
"#;
        Config::from_toml(toml).unwrap()
    }

    #[test]
    fn test_decompose_leaf() {
        let config = test_config();
        let result = decompose("11001101011", "outer_t", &[], &config);
        assert!(matches!(result.val, ValueRepr::Struct));
        assert_eq!(result.subfields.len(), 2);
        assert_eq!(result.subfields[0].name, "chan");
        assert_eq!(result.subfields[1].name, "valid");

        // chan is a nested struct
        let chan = &result.subfields[0].result;
        assert!(matches!(chan.val, ValueRepr::Struct));
        assert_eq!(chan.subfields.len(), 2);

        // addr = first 8 bits = "11001101"
        let addr = &chan.subfields[0].result;
        assert!(matches!(&addr.val, ValueRepr::Bits(8, s) if s == "11001101"));

        // burst = next 2 bits = "01" = INCR
        let burst = &chan.subfields[1].result;
        assert!(matches!(&burst.val, ValueRepr::String(s) if s == "INCR"));

        // valid = last bit = "1"
        let valid = &result.subfields[1].result;
        assert!(matches!(&valid.val, ValueRepr::Bits(1, s) if s == "1"));
    }

    #[test]
    fn test_decompose_unknown_enum_value() {
        let config = test_config();
        // burst = "11" which is not in the enum map
        let result = decompose("0000000011", "inner_t", &[], &config);
        let burst = &result.subfields[1].result;
        assert!(matches!(&burst.val, ValueRepr::String(s) if s == "?(11)"));
        assert!(matches!(burst.kind, ValueKind::Warn));
    }

    #[test]
    fn test_decompose_1d_array() {
        let config = test_config();
        // inner_t is 10 bits; 2-element array = 20 bits
        let bits = "11111111001010101010";
        let result = decompose(bits, "inner_t", &[2], &config);
        assert!(matches!(result.val, ValueRepr::Struct));
        assert_eq!(result.subfields.len(), 2);
        assert_eq!(result.subfields[0].name, "[0]");
        assert_eq!(result.subfields[1].name, "[1]");

        // [0].addr = "11111111"
        let elem0 = &result.subfields[0].result;
        assert_eq!(elem0.subfields[0].name, "addr");
        assert!(matches!(&elem0.subfields[0].result.val, ValueRepr::Bits(8, s) if s == "11111111"));

        // [1].addr = "10101010"
        let elem1 = &result.subfields[1].result;
        assert!(matches!(&elem1.subfields[0].result.val, ValueRepr::Bits(8, s) if s == "10101010"));
    }

    #[test]
    fn test_decompose_2d_array() {
        let config = test_config();
        // inner_t is 10 bits; 2x3 array = 60 bits
        // Lay out 6 elements: [0][0], [0][1], [0][2], [1][0], [1][1], [1][2]
        let e00 = "0000000000";
        let e01 = "1111111101"; // addr=11111111, burst=01(INCR)
        let e02 = "0101010110"; // addr=01010101, burst=10(WRAP)
        let e10 = "1010101000"; // addr=10101010, burst=00(FIXED)
        let e11 = "1100110001";
        let e12 = "0011001100";
        let bits = format!("{e00}{e01}{e02}{e10}{e11}{e12}");
        let result = decompose(&bits, "inner_t", &[2, 3], &config);

        // Top level: 2 elements
        assert_eq!(result.subfields.len(), 2);
        assert_eq!(result.subfields[0].name, "[0]");
        assert_eq!(result.subfields[1].name, "[1]");

        // [0] has 3 sub-elements
        let dim0 = &result.subfields[0].result;
        assert_eq!(dim0.subfields.len(), 3);
        assert_eq!(dim0.subfields[0].name, "[0]");
        assert_eq!(dim0.subfields[1].name, "[1]");
        assert_eq!(dim0.subfields[2].name, "[2]");

        // [0][1].burst = INCR
        let e01_result = &dim0.subfields[1].result;
        assert!(matches!(&e01_result.subfields[1].result.val, ValueRepr::String(s) if s == "INCR"));

        // [0][2].burst = WRAP
        let e02_result = &dim0.subfields[2].result;
        assert!(matches!(&e02_result.subfields[1].result.val, ValueRepr::String(s) if s == "WRAP"));

        // [1][0].burst = FIXED
        let dim1 = &result.subfields[1].result;
        let e10_result = &dim1.subfields[0].result;
        assert!(
            matches!(&e10_result.subfields[1].result.val, ValueRepr::String(s) if s == "FIXED")
        );
    }

    #[test]
    fn test_variable_info() {
        let config = test_config();
        let info = build_variable_info("outer_t", &[], &config);
        match info {
            VariableInfo::Compound { ref subfields } => {
                assert_eq!(subfields.len(), 2);
                assert_eq!(subfields[0].0, "chan");
                assert!(matches!(subfields[0].1, VariableInfo::Compound { .. }));
                assert_eq!(subfields[1].0, "valid");
                assert!(matches!(subfields[1].1, VariableInfo::Bool));
            }
            _ => panic!("expected Compound"),
        }
    }

    #[test]
    fn test_variable_info_2d_array() {
        let config = test_config();
        let info = build_variable_info("inner_t", &[2, 3], &config);
        // Top level: [0], [1]
        let VariableInfo::Compound { ref subfields } = info else {
            panic!("expected Compound");
        };
        assert_eq!(subfields.len(), 2);
        assert_eq!(subfields[0].0, "[0]");

        // [0]: [0], [1], [2]
        let VariableInfo::Compound {
            subfields: ref inner,
        } = subfields[0].1
        else {
            panic!("expected Compound");
        };
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[0].0, "[0]");

        // [0][0]: struct with addr, burst
        let VariableInfo::Compound {
            subfields: ref fields,
        } = inner[0].1
        else {
            panic!("expected Compound");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "addr");
        assert_eq!(fields[1].0, "burst");
    }

    #[test]
    fn test_field_level_2d_array() {
        let toml = r#"
[structs.elem_t]
[[structs.elem_t.fields]]
name = "val"
width = 4

[structs.container_t]
[[structs.container_t.fields]]
name = "matrix"
struct_type = "elem_t"
array_dims = [2, 3]
"#;
        let config = Config::from_toml(toml).unwrap();
        // elem_t is 4 bits, 2x3 = 24 bits total for the field
        assert_eq!(config.struct_total_width("container_t"), 24);

        // Decompose: 6 elements of 4 bits each, 24 bits total
        let bits = "000101001110010011110100";
        let result = decompose(bits, "container_t", &[], &config);
        let matrix = &result.subfields[0].result;

        // [0] has 3 elements
        assert_eq!(matrix.subfields.len(), 2);
        let row0 = &matrix.subfields[0].result;
        assert_eq!(row0.subfields.len(), 3);

        // [0][0].val = "0001"
        let e00 = &row0.subfields[0].result;
        assert!(matches!(&e00.subfields[0].result.val, ValueRepr::Bits(4, s) if s == "0001"));
    }

    #[test]
    fn test_scalar_1d_array_field() {
        // logic [2:0][3:0] data → 3 elements of 4-bit vectors, 12 bits total
        let toml = r#"
[structs.my_t]
[[structs.my_t.fields]]
name = "data"
width = 4
array_dims = [3]
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.struct_total_width("my_t"), 12);

        // 3 elements: "1010", "0011", "1111"
        let bits = "101000111111";
        let result = decompose(bits, "my_t", &[], &config);
        let data = &result.subfields[0].result;
        assert_eq!(data.subfields.len(), 3);
        assert_eq!(data.subfields[0].name, "[0]");
        assert!(matches!(&data.subfields[0].result.val, ValueRepr::Bits(4, s) if s == "1010"));
        assert!(matches!(&data.subfields[1].result.val, ValueRepr::Bits(4, s) if s == "0011"));
        assert!(matches!(&data.subfields[2].result.val, ValueRepr::Bits(4, s) if s == "1111"));
    }

    #[test]
    fn test_scalar_2d_array_field() {
        // logic [1:0][2:0][3:0] data → 2x3 array of 4-bit vectors, 24 bits total
        let toml = r#"
[structs.my_t]
[[structs.my_t.fields]]
name = "data"
width = 4
array_dims = [2, 3]
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.struct_total_width("my_t"), 24);

        // 6 elements in row-major: [0][0..2], [1][0..2]
        let bits = "000100100011010001010110";
        let result = decompose(bits, "my_t", &[], &config);
        let data = &result.subfields[0].result;

        // Outer: 2 rows
        assert_eq!(data.subfields.len(), 2);
        assert_eq!(data.subfields[0].name, "[0]");
        assert_eq!(data.subfields[1].name, "[1]");

        // [0]: 3 elements
        let row0 = &data.subfields[0].result;
        assert_eq!(row0.subfields.len(), 3);
        assert!(matches!(&row0.subfields[0].result.val, ValueRepr::Bits(4, s) if s == "0001"));
        assert!(matches!(&row0.subfields[1].result.val, ValueRepr::Bits(4, s) if s == "0010"));
        assert!(matches!(&row0.subfields[2].result.val, ValueRepr::Bits(4, s) if s == "0011"));

        // [1]: 3 elements
        let row1 = &data.subfields[1].result;
        assert!(matches!(&row1.subfields[0].result.val, ValueRepr::Bits(4, s) if s == "0100"));
        assert!(matches!(&row1.subfields[1].result.val, ValueRepr::Bits(4, s) if s == "0101"));
        assert!(matches!(&row1.subfields[2].result.val, ValueRepr::Bits(4, s) if s == "0110"));
    }

    #[test]
    fn test_enum_array_field() {
        let toml = r#"
[enums.state_t]
width = 2
values = { "00" = "IDLE", "01" = "RUN", "10" = "DONE" }

[structs.my_t]
[[structs.my_t.fields]]
name = "states"
enum_type = "state_t"
array_dims = [3]
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.struct_total_width("my_t"), 6);

        // 3 enum values: IDLE, RUN, DONE
        let bits = "000110";
        let result = decompose(bits, "my_t", &[], &config);
        let states = &result.subfields[0].result;
        assert_eq!(states.subfields.len(), 3);
        assert!(matches!(&states.subfields[0].result.val, ValueRepr::String(s) if s == "IDLE"));
        assert!(matches!(&states.subfields[1].result.val, ValueRepr::String(s) if s == "RUN"));
        assert!(matches!(&states.subfields[2].result.val, ValueRepr::String(s) if s == "DONE"));
    }

    #[test]
    fn test_variable_info_scalar_2d_array() {
        let toml = r#"
[structs.my_t]
[[structs.my_t.fields]]
name = "data"
width = 4
array_dims = [2, 3]
"#;
        let config = Config::from_toml(toml).unwrap();
        let info = build_variable_info("my_t", &[], &config);

        // my_t → data → [0],[1] → [0],[1],[2] → Bits
        let VariableInfo::Compound { ref subfields } = info else {
            panic!("expected Compound");
        };
        assert_eq!(subfields[0].0, "data");

        let VariableInfo::Compound {
            subfields: ref outer,
        } = subfields[0].1
        else {
            panic!("expected Compound");
        };
        assert_eq!(outer.len(), 2);

        let VariableInfo::Compound {
            subfields: ref inner,
        } = outer[0].1
        else {
            panic!("expected Compound");
        };
        assert_eq!(inner.len(), 3);
        assert!(matches!(inner[0].1, VariableInfo::Bits));
    }

    #[test]
    fn test_variable_info_plain_scalar_array() {
        let info = build_leaf_variable_info(4, &[3]);
        let VariableInfo::Compound { subfields } = info else {
            panic!("expected Compound");
        };
        assert_eq!(subfields.len(), 3);
        assert!(matches!(subfields[0].1, VariableInfo::Bits));
    }

    #[test]
    fn test_decompose_plain_scalar_array() {
        let result = decompose_scalar("101000111111", 4, &[3]);
        assert_eq!(result.subfields.len(), 3);
        assert!(matches!(&result.subfields[0].result.val, ValueRepr::Bits(4, s) if s == "1010"));
        assert!(matches!(&result.subfields[1].result.val, ValueRepr::Bits(4, s) if s == "0011"));
        assert!(matches!(&result.subfields[2].result.val, ValueRepr::Bits(4, s) if s == "1111"));
    }
}
