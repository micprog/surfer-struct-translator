use crate::config::{Config, FieldDef, StructDef};
use surfer_translation_types::{
    SubFieldTranslationResult, TranslationResult, ValueKind, ValueRepr, VariableInfo,
};

/// Build the hierarchical `VariableInfo` tree for a struct type.
pub fn build_variable_info(struct_name: &str, config: &Config) -> VariableInfo {
    let Some(struct_def) = config.structs.get(struct_name) else {
        return VariableInfo::Bits;
    };
    build_variable_info_for_struct(struct_def, config)
}

fn build_variable_info_for_struct(struct_def: &StructDef, config: &Config) -> VariableInfo {
    VariableInfo::Compound {
        subfields: struct_def
            .fields
            .iter()
            .map(|f| {
                let info = if let Some(ref st) = f.struct_type {
                    config
                        .structs
                        .get(st)
                        .map(|s| build_variable_info_for_struct(s, config))
                        .unwrap_or(VariableInfo::Bits)
                } else {
                    // Leaf fields (plain bits or enums) are represented as Bits
                    // so surfer can apply sub-translators to them.
                    VariableInfo::Bits
                };
                (f.name.clone(), info)
            })
            .collect(),
    }
}

/// Decompose a binary string into a structured `TranslationResult` according to a struct definition.
pub fn decompose(binary_digits: &str, struct_name: &str, config: &Config) -> TranslationResult {
    let Some(struct_def) = config.structs.get(struct_name) else {
        return TranslationResult {
            val: ValueRepr::String(format!("unknown struct: {struct_name}")),
            subfields: vec![],
            kind: ValueKind::Warn,
        };
    };
    decompose_struct(binary_digits, struct_def, config)
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
        // Nested struct: recurse
        if let Some(s) = config.structs.get(st) {
            return decompose_struct(bits, s, config);
        }
    }

    if let Some(ref et) = field.enum_type {
        // Enum field: look up the bit pattern
        if let Some(enum_def) = config.enums.get(et) {
            return if let Some(name) = enum_def.values.get(bits) {
                TranslationResult {
                    val: ValueRepr::String(name.clone()),
                    subfields: vec![],
                    kind: ValueKind::Normal,
                }
            } else {
                // Unknown enum value — show raw bits with warning
                TranslationResult {
                    val: ValueRepr::String(format!("?({bits})")),
                    subfields: vec![],
                    kind: ValueKind::Warn,
                }
            };
        }
    }

    // Leaf field: return as raw bits for surfer's sub-translators
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
        let result = decompose("11001101011", "outer_t", &config);
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
        let result = decompose("0000000011", "inner_t", &config);
        let burst = &result.subfields[1].result;
        assert!(matches!(&burst.val, ValueRepr::String(s) if s == "?(11)"));
        assert!(matches!(burst.kind, ValueKind::Warn));
    }

    #[test]
    fn test_variable_info() {
        let config = test_config();
        let info = build_variable_info("outer_t", &config);
        match info {
            VariableInfo::Compound { ref subfields } => {
                assert_eq!(subfields.len(), 2);
                assert_eq!(subfields[0].0, "chan");
                assert!(matches!(
                    subfields[0].1,
                    VariableInfo::Compound { .. }
                ));
                assert_eq!(subfields[1].0, "valid");
                assert!(matches!(subfields[1].1, VariableInfo::Bits));
            }
            _ => panic!("expected Compound"),
        }
    }
}
