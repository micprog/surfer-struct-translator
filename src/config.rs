use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub structs: HashMap<String, StructDef>,
    #[serde(default)]
    pub enums: HashMap<String, EnumDef>,
    #[serde(default)]
    pub mappings: Vec<Mapping>,
}

#[derive(Deserialize, Clone)]
pub struct StructDef {
    pub fields: Vec<FieldDef>,
}

#[derive(Clone)]
pub struct FieldDef {
    pub name: String,
    /// Bit width for leaf fields.
    pub width: Option<u32>,
    /// Reference to a nested struct type (mutually exclusive with width/enum_type).
    pub struct_type: Option<String>,
    /// Reference to an enum type (mutually exclusive with width/struct_type).
    pub enum_type: Option<String>,
    /// Dimensions for packed array fields (empty = scalar, [N] = 1D, [M,N] = 2D, etc.).
    pub array_dims: Vec<u32>,
}

/// Raw deserialization helper that accepts both `array_size` and `array_dims`.
#[derive(Deserialize)]
struct FieldDefRaw {
    name: String,
    width: Option<u32>,
    struct_type: Option<String>,
    enum_type: Option<String>,
    #[serde(default)]
    array_size: Option<u32>,
    #[serde(default)]
    array_dims: Option<Vec<u32>>,
}

impl From<FieldDefRaw> for FieldDef {
    fn from(raw: FieldDefRaw) -> Self {
        let array_dims = dims_from_raw(raw.array_dims, raw.array_size);
        FieldDef {
            name: raw.name,
            width: raw.width,
            struct_type: raw.struct_type,
            enum_type: raw.enum_type,
            array_dims,
        }
    }
}

impl<'de> Deserialize<'de> for FieldDef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        FieldDefRaw::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Deserialize, Clone)]
pub struct EnumDef {
    /// Bit width of the enum.
    pub width: u32,
    /// Mapping from binary bit pattern to display name.
    pub values: HashMap<String, String>,
}

#[derive(Clone)]
pub struct Mapping {
    /// Glob pattern matched against the full signal path (supports `*`).
    pub pattern: String,
    /// Name of the struct type to decompose into.
    pub struct_type: String,
    /// Optional: only match if the signal's bit width equals this value.
    pub num_bits: Option<u32>,
    /// Dimensions for array mappings (empty = not an array).
    pub array_dims: Vec<u32>,
}

/// Raw deserialization helper for Mapping.
#[derive(Deserialize)]
struct MappingRaw {
    pattern: String,
    struct_type: String,
    num_bits: Option<u32>,
    #[serde(default)]
    array_size: Option<u32>,
    #[serde(default)]
    array_dims: Option<Vec<u32>>,
}

impl From<MappingRaw> for Mapping {
    fn from(raw: MappingRaw) -> Self {
        let array_dims = dims_from_raw(raw.array_dims, raw.array_size);
        Mapping {
            pattern: raw.pattern,
            struct_type: raw.struct_type,
            num_bits: raw.num_bits,
            array_dims,
        }
    }
}

impl<'de> Deserialize<'de> for Mapping {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        MappingRaw::deserialize(deserializer).map(Into::into)
    }
}

/// Convert raw `array_dims` / `array_size` fields into a canonical dims vector.
/// `array_dims` takes precedence. `array_size` of 1 is treated as scalar (empty).
fn dims_from_raw(array_dims: Option<Vec<u32>>, array_size: Option<u32>) -> Vec<u32> {
    if let Some(dims) = array_dims {
        dims
    } else if let Some(size) = array_size {
        if size > 1 { vec![size] } else { vec![] }
    } else {
        vec![]
    }
}

/// Total number of elements across all dimensions (product of dims, 1 for scalar).
pub fn total_elements(dims: &[u32]) -> u32 {
    dims.iter().copied().product::<u32>().max(1)
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Resolve the bit width of a field, recursing into struct/enum references.
    /// For array fields, returns element_width * total_elements.
    pub fn field_width(&self, field: &FieldDef) -> u32 {
        let elem_width = if let Some(w) = field.width {
            w
        } else if let Some(ref st) = field.struct_type {
            self.struct_total_width(st)
        } else if let Some(ref et) = field.enum_type {
            self.enums.get(et).map_or(0, |e| e.width)
        } else {
            0
        };
        elem_width * total_elements(&field.array_dims)
    }

    /// Compute the total bit width of a struct type.
    pub fn struct_total_width(&self, name: &str) -> u32 {
        self.structs
            .get(name)
            .map(|s| s.fields.iter().map(|f| self.field_width(f)).sum())
            .unwrap_or(0)
    }

    /// Find the struct type and array dims that match a given signal path and bit width.
    pub fn find_mapping(&self, full_path: &str, num_bits: Option<u32>) -> Option<(&str, &[u32])> {
        self.mappings.iter().find_map(|m| {
            if let Some(required) = m.num_bits
                && num_bits != Some(required)
            {
                return None;
            }
            if glob_match(&m.pattern, full_path) {
                Some((m.struct_type.as_str(), m.array_dims.as_slice()))
            } else {
                None
            }
        })
    }
}

/// Simple glob matching supporting `*` as a wildcard for any sequence of characters.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return pattern == text;
    }

    let mut pos = 0;

    // First part must match the start
    if let Some(first) = parts.first()
        && !first.is_empty()
    {
        if !text.starts_with(first) {
            return false;
        }
        pos = first.len();
    }

    // Last part must match the end
    if let Some(last) = parts.last()
        && !last.is_empty()
        && !text[pos..].ends_with(last)
    {
        return false;
    }

    // Middle parts must appear in order
    for part in &parts[1..parts.len().saturating_sub(1)] {
        if part.is_empty() {
            continue;
        }
        if let Some(idx) = text[pos..].find(part) {
            pos += idx + part.len();
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*foo*", "abcfoodef"));
        assert!(glob_match("*foo*", "foo"));
        assert!(glob_match("*foo", "barfoo"));
        assert!(glob_match("foo*", "foobar"));
        assert!(glob_match("foo", "foo"));
        assert!(!glob_match("foo", "bar"));
        assert!(!glob_match("*foo*", "bar"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.dram_req", "TOP.dut.dram_req"));
        assert!(!glob_match("*.dram_req", "TOP.dut.dram_req.extra"));
    }

    #[test]
    fn test_config_parse() {
        let toml = r#"
[enums.burst_t]
width = 2
values = { "00" = "FIXED", "01" = "INCR", "10" = "WRAP" }

[structs.chan_t]
[[structs.chan_t.fields]]
name = "id"
width = 4
[[structs.chan_t.fields]]
name = "burst"
enum_type = "burst_t"

[structs.req_t]
[[structs.req_t.fields]]
name = "aw"
struct_type = "chan_t"
[[structs.req_t.fields]]
name = "valid"
width = 1

[[mappings]]
pattern = "*_req*"
struct_type = "req_t"
num_bits = 7
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.struct_total_width("chan_t"), 6); // 4 + 2
        assert_eq!(config.struct_total_width("req_t"), 7); // 6 + 1
        assert_eq!(
            config.find_mapping("TOP.dut.axi_req_o", Some(7)),
            Some(("req_t", [].as_slice()))
        );
        assert_eq!(config.find_mapping("TOP.dut.axi_req_o", Some(8)), None);
    }

    #[test]
    fn test_config_parse_array_size_compat() {
        let toml = r#"
[structs.inner_t]
[[structs.inner_t.fields]]
name = "data"
width = 8

[structs.outer_t]
[[structs.outer_t.fields]]
name = "items"
struct_type = "inner_t"
array_size = 4

[[mappings]]
pattern = "*.sig"
struct_type = "outer_t"
array_size = 2
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.structs["outer_t"].fields[0].array_dims, vec![4]);
        assert_eq!(config.mappings[0].array_dims, vec![2]);
        assert_eq!(config.struct_total_width("outer_t"), 32); // 8 * 4
    }

    #[test]
    fn test_config_parse_array_dims() {
        let toml = r#"
[structs.inner_t]
[[structs.inner_t.fields]]
name = "data"
width = 8

[structs.outer_t]
[[structs.outer_t.fields]]
name = "matrix"
struct_type = "inner_t"
array_dims = [2, 3]

[[mappings]]
pattern = "*.sig"
struct_type = "outer_t"
array_dims = [4, 5]
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.structs["outer_t"].fields[0].array_dims, vec![2, 3]);
        assert_eq!(config.mappings[0].array_dims, vec![4, 5]);
        assert_eq!(config.struct_total_width("outer_t"), 48); // 8 * 2 * 3
    }

    #[test]
    fn test_ara_config_widths() {
        let toml = include_str!("../struct_defs.toml");
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.struct_total_width("aw_chan_t"), 105);
        assert_eq!(config.struct_total_width("w_chan_t_146"), 146);
        assert_eq!(config.struct_total_width("w_chan_t_74"), 74);
        assert_eq!(config.struct_total_width("b_chan_t_8"), 8);
        assert_eq!(config.struct_total_width("ar_chan_t"), 99);
        assert_eq!(config.struct_total_width("r_chan_t_137"), 137);
        assert_eq!(config.struct_total_width("axi_req_t"), 355);
        assert_eq!(config.struct_total_width("axi_resp_t"), 150);
    }
}
