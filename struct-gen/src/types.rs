//! JSON types matching the C++ reflect_types output.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct ReflectedData {
    pub structs: Vec<ReflectedStruct>,
    pub enums: Vec<ReflectedEnum>,
    #[serde(default)]
    pub signal_mappings: Vec<ReflectedSignalMapping>,
}

#[derive(Clone)]
pub struct ReflectedSignalMapping {
    pub path: String,
    pub type_name: String,
    pub width: u32,
    /// Array dimensions (empty = scalar, [N] = 1D, [M,N] = 2D, etc.).
    pub array_dims: Vec<u32>,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedStruct {
    pub name: String,
    pub fields: Vec<ReflectedField>,
}

#[derive(Clone)]
pub struct ReflectedField {
    pub name: String,
    pub width: u32,
    pub kind: String,
    pub type_name: String,
    /// Array dimensions (empty = scalar, [N] = 1D, [M,N] = 2D, etc.).
    pub array_dims: Vec<u32>,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedEnum {
    pub name: String,
    pub width: u32,
    pub values: Vec<ReflectedEnumValue>,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedEnumValue {
    pub name: String,
    #[allow(dead_code)]
    pub value: u64,
    pub binary: String,
}

/// Total number of elements across all dimensions (product of dims, 1 for scalar).
pub fn total_elements(dims: &[u32]) -> u32 {
    dims.iter().copied().product::<u32>().max(1)
}

// ── Backward-compatible deserialization ──────────────────────────────────────

/// Convert raw `array_dims` / `array_size` fields into a canonical dims vector.
fn dims_from_raw(array_dims: Option<Vec<u32>>, array_size: Option<u32>) -> Vec<u32> {
    if let Some(dims) = array_dims {
        dims
    } else if let Some(size) = array_size {
        if size > 1 { vec![size] } else { vec![] }
    } else {
        vec![]
    }
}

#[derive(Deserialize)]
struct ReflectedFieldRaw {
    name: String,
    width: u32,
    kind: String,
    type_name: String,
    #[serde(default)]
    array_size: Option<u32>,
    #[serde(default)]
    array_dims: Option<Vec<u32>>,
}

impl From<ReflectedFieldRaw> for ReflectedField {
    fn from(raw: ReflectedFieldRaw) -> Self {
        ReflectedField {
            name: raw.name,
            width: raw.width,
            kind: raw.kind,
            type_name: raw.type_name,
            array_dims: dims_from_raw(raw.array_dims, raw.array_size),
        }
    }
}

impl<'de> Deserialize<'de> for ReflectedField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ReflectedFieldRaw::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Deserialize)]
struct ReflectedSignalMappingRaw {
    path: String,
    type_name: String,
    width: u32,
    #[serde(default)]
    array_size: Option<u32>,
    #[serde(default)]
    array_dims: Option<Vec<u32>>,
}

impl From<ReflectedSignalMappingRaw> for ReflectedSignalMapping {
    fn from(raw: ReflectedSignalMappingRaw) -> Self {
        ReflectedSignalMapping {
            path: raw.path,
            type_name: raw.type_name,
            width: raw.width,
            array_dims: dims_from_raw(raw.array_dims, raw.array_size),
        }
    }
}

impl<'de> Deserialize<'de> for ReflectedSignalMapping {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ReflectedSignalMappingRaw::deserialize(deserializer).map(Into::into)
    }
}
