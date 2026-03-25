//! Verilator-style file list (.f / flist) parsing.

use std::path::Path;

/// Parsed contents of a file list.
pub struct FlistContents {
    pub files: Vec<String>,
    pub includes: Vec<String>,
    pub defines: Vec<String>,
}

/// Parse a Verilator-style file list (.f / flist).
///
/// Supports:
/// - `+incdir+<path>` — include directory
/// - `+define+<name>` or `+define+<name>=<value>` — preprocessor define
/// - `// comment` and `# comment` — line comments
/// - Blank lines are skipped
/// - Everything else is treated as a source file path
pub fn parse_flist(path: &Path) -> Result<FlistContents, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read flist '{}': {e}", path.display()))?;

    let mut result = FlistContents {
        files: Vec::new(),
        includes: Vec::new(),
        defines: Vec::new(),
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        classify_arg(line, &mut result.files, &mut result.includes, &mut result.defines);
    }

    Ok(result)
}

/// Classify an argument that might be +incdir+, +define+, or a file path.
pub fn classify_arg(
    arg: &str,
    files: &mut Vec<String>,
    includes: &mut Vec<String>,
    defines: &mut Vec<String>,
) {
    if let Some(incdir) = arg.strip_prefix("+incdir+") {
        includes.push(incdir.to_string());
    } else if let Some(define) = arg.strip_prefix("+define+") {
        defines.push(define.to_string());
    } else if arg.starts_with('+') {
        eprintln!("Warning: ignoring unknown directive: {arg}");
    } else {
        files.push(arg.to_string());
    }
}
