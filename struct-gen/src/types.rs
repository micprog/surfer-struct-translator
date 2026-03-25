//! JSON types matching the C++ reflect_types output.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct ReflectedData {
    pub structs: Vec<ReflectedStruct>,
    pub enums: Vec<ReflectedEnum>,
    #[serde(default)]
    pub signal_mappings: Vec<ReflectedSignalMapping>,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedSignalMapping {
    pub path: String,
    pub type_name: String,
    #[allow(dead_code)]
    pub width: u32,
    #[serde(default = "default_one")]
    pub array_size: u32,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedStruct {
    pub name: String,
    pub fields: Vec<ReflectedField>,
}

#[derive(Deserialize, Clone)]
pub struct ReflectedField {
    pub name: String,
    pub width: u32,
    pub kind: String,
    pub type_name: String,
    #[serde(default = "default_one")]
    pub array_size: u32,
}

fn default_one() -> u32 {
    1
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
