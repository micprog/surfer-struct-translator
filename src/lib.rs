mod config;
mod decompose;

use std::sync::Mutex;

use extism_pdk::{FnResult, host_fn, plugin_fn};
use surfer_translation_types::{
    TranslationPreference, TranslationResult, ValueKind, ValueRepr, VariableInfo, VariableMeta,
    VariableValue,
};

pub use surfer_translation_types::plugin_types::TranslateParams;

use config::Config;

#[host_fn]
extern "ExtismHost" {
    pub fn read_file(filename: String) -> Vec<u8>;
    pub fn file_exists(filename: String) -> bool;
    pub fn translators_config_dir() -> Vec<u8>;
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

fn load_config() -> Option<Config> {
    let raw = unsafe { translators_config_dir() }.ok()?;
    let config_dir: Option<String> = serde_json::from_slice(&raw).ok()?;
    let config_dir = config_dir?;
    let path = format!("{}/struct_defs.toml", config_dir);

    let exists = unsafe { file_exists(path.clone()) }.ok()?;
    if !exists {
        return None;
    }

    let bytes = unsafe { read_file(path) }.ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Config::from_toml(&text).ok()
}

fn with_config<T>(f: impl FnOnce(&Config) -> T) -> Option<T> {
    let guard = CONFIG.lock().ok()?;
    guard.as_ref().map(f)
}

fn signal_full_path(variable: &VariableMeta<(), ()>) -> String {
    let mut path = variable.var.path.strs.join(".");
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(&variable.var.name);
    path
}

#[plugin_fn]
pub fn new() -> FnResult<()> {
    let config = load_config();
    if let Ok(mut guard) = CONFIG.lock() {
        *guard = config;
    }
    Ok(())
}

#[plugin_fn]
pub fn name() -> FnResult<String> {
    Ok("Struct Decomposer".to_string())
}

#[plugin_fn]
pub fn translates(variable: VariableMeta<(), ()>) -> FnResult<TranslationPreference> {
    let full_path = signal_full_path(&variable);
    let pref = with_config(|config| {
        config
            .find_mapping(&full_path, variable.num_bits)
            .map(|_| TranslationPreference::Prefer)
            .unwrap_or(TranslationPreference::No)
    })
    .unwrap_or(TranslationPreference::No);
    Ok(pref)
}

#[plugin_fn]
pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
    let full_path = signal_full_path(&variable);
    let info = with_config(|config| {
        config
            .find_mapping(&full_path, variable.num_bits)
            .map(|struct_name| decompose::build_variable_info(struct_name, config))
            .unwrap_or(VariableInfo::Bits)
    })
    .unwrap_or(VariableInfo::Bits);
    Ok(info)
}

#[plugin_fn]
pub fn translate(
    TranslateParams { variable, value }: TranslateParams,
) -> FnResult<TranslationResult> {
    let full_path = signal_full_path(&variable);

    let result = with_config(|config| {
        let Some(struct_name) = config.find_mapping(&full_path, variable.num_bits) else {
            return TranslationResult {
                val: ValueRepr::String("no mapping".to_string()),
                subfields: vec![],
                kind: ValueKind::Warn,
            };
        };

        let binary_digits = match &value {
            VariableValue::BigUint(big_uint) => {
                let raw = format!("{big_uint:b}");
                let num_bits = variable.num_bits.unwrap_or_default() as usize;
                let padding = "0".repeat(num_bits.saturating_sub(raw.len()));
                format!("{padding}{raw}")
            }
            VariableValue::String(v) => v.clone(),
        };

        decompose::decompose(&binary_digits, struct_name, config)
    });

    Ok(result.unwrap_or_else(|| TranslationResult {
        val: ValueRepr::String("no config loaded".to_string()),
        subfields: vec![],
        kind: ValueKind::Warn,
    }))
}
