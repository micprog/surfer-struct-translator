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

#[derive(Deserialize, Clone)]
pub struct FieldDef {
    pub name: String,
    /// Bit width for leaf fields.
    pub width: Option<u32>,
    /// Reference to a nested struct type (mutually exclusive with width/enum_type).
    pub struct_type: Option<String>,
    /// Reference to an enum type (mutually exclusive with width/struct_type).
    pub enum_type: Option<String>,
    /// Number of elements for packed array fields (default 1).
    #[serde(default = "default_one")]
    pub array_size: u32,
}

fn default_one() -> u32 {
    1
}

#[derive(Deserialize, Clone)]
pub struct EnumDef {
    /// Bit width of the enum.
    pub width: u32,
    /// Mapping from binary bit pattern to display name.
    pub values: HashMap<String, String>,
}

#[derive(Deserialize, Clone)]
pub struct Mapping {
    /// Glob pattern matched against the full signal path (supports `*`).
    pub pattern: String,
    /// Name of the struct type to decompose into.
    pub struct_type: String,
    /// Optional: only match if the signal's bit width equals this value.
    pub num_bits: Option<u32>,
    /// Number of array elements (default 1 = not an array).
    #[serde(default = "default_one")]
    pub array_size: u32,
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Resolve the bit width of a field, recursing into struct/enum references.
    /// For array fields, returns element_width * array_size.
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
        elem_width * field.array_size
    }

    /// Compute the total bit width of a struct type.
    pub fn struct_total_width(&self, name: &str) -> u32 {
        self.structs
            .get(name)
            .map(|s| s.fields.iter().map(|f| self.field_width(f)).sum())
            .unwrap_or(0)
    }

    /// Find the struct type and array size that matches a given signal path and bit width.
    pub fn find_mapping(&self, full_path: &str, num_bits: Option<u32>) -> Option<(&str, u32)> {
        self.mappings.iter().find_map(|m| {
            if let Some(required) = m.num_bits
                && num_bits != Some(required)
            {
                return None;
            }
            if glob_match(&m.pattern, full_path) {
                Some((m.struct_type.as_str(), m.array_size))
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
            Some(("req_t", 1))
        );
        assert_eq!(config.find_mapping("TOP.dut.axi_req_o", Some(8)), None);
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
