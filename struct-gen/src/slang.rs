// Rust bindings for the Slang SystemVerilog parser.
// Based on bender-slang by Tim Fischer <fischeti@iis.ee.ethz.ch>

use cxx::UniquePtr;

#[cxx::bridge]
mod ffi {
    /// Result type for C++ functions that can fail.
    /// On success, `error` is empty; on failure, `error` contains the message.
    struct SlangResult {
        value: String,
        error: String,
    }

    unsafe extern "C++" {
        include!("surfer-struct-gen/cpp/slang_bridge.h");

        /// Opaque session that owns parse contexts and syntax trees.
        type SlangSession;

        fn new_slang_session() -> UniquePtr<SlangSession>;

        /// Parse a group of files. Returns empty error on success.
        fn parse_group(
            self: Pin<&mut SlangSession>,
            files: &Vec<String>,
            includes: &Vec<String>,
            defines: &Vec<String>,
        ) -> SlangResult;

        fn tree_count(session: &SlangSession) -> usize;

        /// Compiles all syntax trees in the session, visits the elaborated AST,
        /// and returns a JSON string describing all packed struct and enum type aliases.
        fn reflect_types(
            session: &SlangSession,
            public_only: bool,
            top_modules: &Vec<String>,
            param_overrides: &Vec<String>,
        ) -> SlangResult;
    }
}

/// Public owner for all parsed trees and parse contexts.
pub struct SlangSession {
    inner: UniquePtr<ffi::SlangSession>,
}

impl SlangSession {
    pub fn new() -> Self {
        Self {
            inner: ffi::new_slang_session(),
        }
    }

    /// Parses one source group with scoped include directories and defines.
    pub fn parse_group(
        &mut self,
        files: &[String],
        includes: &[String],
        defines: &[String],
    ) -> Result<(), String> {
        let files_vec = files.to_vec();
        let includes_vec = includes.to_vec();
        let defines_vec = defines.to_vec();

        let result = self
            .inner
            .pin_mut()
            .parse_group(&files_vec, &includes_vec, &defines_vec);

        if result.error.is_empty() {
            Ok(())
        } else {
            Err(result.error)
        }
    }

    /// Returns the total number of parsed syntax trees in the session.
    pub fn tree_count(&self) -> usize {
        ffi::tree_count(self.inner.as_ref().unwrap())
    }

    /// Compiles all parsed syntax trees and extracts packed struct and enum type definitions.
    ///
    /// Returns a JSON string with the extracted type information.
    pub fn reflect_types(
        &self,
        public_only: bool,
        top_modules: &[String],
        param_overrides: &[String],
    ) -> Result<String, String> {
        let tops = top_modules.to_vec();
        let params = param_overrides.to_vec();
        let result = ffi::reflect_types(self.inner.as_ref().unwrap(), public_only, &tops, &params);

        if result.error.is_empty() {
            Ok(result.value)
        } else {
            Err(result.error)
        }
    }
}
