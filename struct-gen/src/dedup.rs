//! Deduplication of parameterized struct and enum types.

use std::collections::HashMap;

use crate::types::{ReflectedEnum, ReflectedField, ReflectedStruct};

/// A deduplicated struct with a unique TOML key name.
pub struct UniqueStruct {
    /// The TOML key name (may include _<width> suffix for disambiguation).
    pub key: String,
    /// The original SV type name.
    pub sv_name: String,
    pub fields: Vec<ReflectedField>,
    pub total_width: u32,
}

/// A deduplicated enum with a unique TOML key name.
pub struct UniqueEnum {
    pub key: String,
    pub inner: ReflectedEnum,
}

/// Compute a signature for a struct's fields to detect different parameterizations.
fn field_signature(fields: &[ReflectedField]) -> String {
    fields
        .iter()
        .map(|f| format!("{}:{}", f.name, f.width))
        .collect::<Vec<_>>()
        .join(",")
}

/// Deduplicate structs: same (name, field_signature) → keep one copy.
/// When the same name appears with different field signatures, suffix with _<total_width>.
pub fn deduplicate_structs(raw: &[ReflectedStruct]) -> Vec<UniqueStruct> {
    let mut by_name: HashMap<&str, Vec<(String, &ReflectedStruct)>> = HashMap::new();
    for s in raw {
        let sig = field_signature(&s.fields);
        by_name.entry(&s.name).or_default().push((sig, s));
    }

    let mut result = Vec::new();

    for (name, entries) in &by_name {
        // Deduplicate by signature.
        let mut unique_sigs: Vec<(String, &ReflectedStruct)> = Vec::new();
        for (sig, s) in entries {
            if !unique_sigs
                .iter()
                .any(|(existing_sig, _)| existing_sig == sig)
            {
                unique_sigs.push((sig.clone(), s));
            }
        }

        if unique_sigs.len() == 1 {
            let s = unique_sigs[0].1;
            let total: u32 = s.fields.iter().map(|f| f.width).sum();
            result.push(UniqueStruct {
                key: name.to_string(),
                sv_name: name.to_string(),
                fields: s.fields.clone(),
                total_width: total,
            });
        } else {
            for (_, s) in &unique_sigs {
                let total: u32 = s.fields.iter().map(|f| f.width).sum();
                result.push(UniqueStruct {
                    key: format!("{name}_{total}"),
                    sv_name: name.to_string(),
                    fields: s.fields.clone(),
                    total_width: total,
                });
            }
        }
    }

    result.sort_by(|a, b| a.key.cmp(&b.key));
    result
}

/// Deduplicate enums: same (name, width, values) → keep one copy.
/// When same name appears with different widths, suffix with _<width>.
pub fn deduplicate_enums(raw: &[ReflectedEnum]) -> Vec<UniqueEnum> {
    let mut by_name: HashMap<&str, Vec<&ReflectedEnum>> = HashMap::new();
    let mut seen: HashMap<(&str, u32), ()> = HashMap::new();
    for e in raw {
        if seen.insert((&e.name, e.width), ()).is_none() {
            by_name.entry(&e.name).or_default().push(e);
        }
    }

    let mut result = Vec::new();
    for (name, variants) in &by_name {
        if variants.len() == 1 {
            result.push(UniqueEnum {
                key: name.to_string(),
                inner: variants[0].clone(),
            });
        } else {
            for v in variants {
                result.push(UniqueEnum {
                    key: format!("{name}_{}", v.width),
                    inner: (*v).clone(),
                });
            }
        }
    }
    result.sort_by(|a, b| a.key.cmp(&b.key));
    result
}
