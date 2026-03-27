mod config;
mod decompose;
mod generate;
mod meta_config;

use std::sync::Mutex;

use extism_pdk::{FnResult, host_fn, plugin_fn};
use surfer_translation_types::{
    TranslationPreference, TranslationResult, ValueKind, ValueRepr, VariableInfo, VariableMeta,
    VariableValue,
};

pub use surfer_translation_types::plugin_types::TranslateParams;

use config::Config;
use meta_config::MetaConfig;

#[host_fn]
extern "ExtismHost" {
    pub fn read_file(filename: String) -> Vec<u8>;
    pub fn file_exists(filename: String) -> bool;
    pub fn translators_config_dir() -> Vec<u8>;
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

fn read_file_text(path: &str) -> Option<String> {
    let bytes = unsafe { read_file(path.to_string()) }.ok()?;
    String::from_utf8(bytes).ok()
}

fn file_exists_check(path: &str) -> bool {
    unsafe { file_exists(path.to_string()) }.unwrap_or(false)
}

fn get_config_dir() -> Option<String> {
    let raw = unsafe { translators_config_dir() }.ok()?;
    let config_dir: Option<String> = serde_json::from_slice(&raw).ok()?;
    config_dir
}

/// Load plugin configuration with three-tier fallback:
///
/// 1. `struct_config.toml` with `struct_defs_file` → load that TOML file
/// 2. `struct_config.toml` with `[sources]` → parse SV sources with slang
/// 3. `struct_defs.toml` directly → backward compatible
fn load_config() -> Option<Config> {
    let config_dir = get_config_dir()?;

    // Try struct_config.toml first (new format).
    let meta_path = format!("{config_dir}/struct_config.toml");
    if file_exists_check(&meta_path)
        && let Some(text) = read_file_text(&meta_path)
    {
        match toml::from_str::<MetaConfig>(&text) {
            Ok(meta) => {
                // Mode 1: Pre-generated definitions file.
                if let Some(ref defs_file) = meta.struct_defs_file {
                    let defs_path = if std::path::Path::new(defs_file).is_absolute() {
                        defs_file.clone()
                    } else {
                        format!("{config_dir}/{defs_file}")
                    };
                    if let Some(defs_text) = read_file_text(&defs_path) {
                        return Config::from_toml(&defs_text).ok();
                    }
                }

                // Mode 2: Parse SystemVerilog sources on the fly.
                if let Some(ref sources) = meta.sources {
                    match generate::generate_from_sources(sources, &config_dir) {
                        Ok(config) => return Some(config),
                        Err(e) => {
                            // Log error but don't crash — fall through to fallback.
                            extism_pdk::log!(
                                extism_pdk::LogLevel::Error,
                                "Failed to generate struct defs from sources: {e}"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                extism_pdk::log!(
                    extism_pdk::LogLevel::Error,
                    "Failed to parse struct_config.toml: {e}"
                );
            }
        }
    }

    // Mode 3: Fallback to struct_defs.toml (backward compatible).
    let path = format!("{config_dir}/struct_defs.toml");
    if file_exists_check(&path) {
        let text = read_file_text(&path)?;
        return Config::from_toml(&text).ok();
    }

    None
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
            .map(|(struct_name, array_size)| {
                decompose::build_variable_info(struct_name, array_size, config)
            })
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
        let Some((struct_name, array_size)) = config.find_mapping(&full_path, variable.num_bits)
        else {
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

        decompose::decompose(&binary_digits, struct_name, array_size, config)
    });

    Ok(result.unwrap_or_else(|| TranslationResult {
        val: ValueRepr::String("no config loaded".to_string()),
        subfields: vec![],
        kind: ValueKind::Warn,
    }))
}
