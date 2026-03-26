// Slang bridge for surfer-struct-gen
// Based on bender-slang by Tim Fischer <fischeti@iis.ee.ethz.ch>

#ifndef SLANG_BRIDGE_H
#define SLANG_BRIDGE_H

#include "rust/cxx.h"
#include "slang/diagnostics/DiagnosticEngine.h"
#include "slang/diagnostics/TextDiagnosticClient.h"
#include "slang/parsing/Preprocessor.h"
#include "slang/syntax/SyntaxTree.h"
#include "slang/text/SourceManager.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

/// Result type shared with Rust via cxx bridge.
/// On success, `error` is empty; on failure, `error` contains the message.
struct SlangResult;

class SlangContext {
  public:
    SlangContext();

    /// Returns empty string on success, error message on failure.
    rust::String set_includes(const rust::Vec<rust::String>& includes);
    void set_defines(const rust::Vec<rust::String>& defines);

    /// Returns parsed trees, or sets error_out on failure.
    std::vector<std::shared_ptr<slang::syntax::SyntaxTree>> parse_files(
        const rust::Vec<rust::String>& paths, rust::String& error_out);

  private:
    slang::SourceManager sourceManager;
    slang::parsing::PreprocessorOptions ppOptions;
    slang::DiagnosticEngine diagEngine;
    std::shared_ptr<slang::TextDiagnosticClient> diagClient;
};

class SlangSession {
  public:
    SlangResult parse_group(const rust::Vec<rust::String>& files, const rust::Vec<rust::String>& includes,
                     const rust::Vec<rust::String>& defines);

    const std::vector<std::shared_ptr<slang::syntax::SyntaxTree>>& trees() const { return allTrees; }

  private:
    std::vector<std::unique_ptr<SlangContext>> contexts;
    std::vector<std::shared_ptr<slang::syntax::SyntaxTree>> allTrees;
};

std::unique_ptr<SlangSession> new_slang_session();

std::size_t tree_count(const SlangSession& session);

/// Compiles all syntax trees in the session, visits the elaborated AST,
/// and returns a JSON string describing all packed struct and enum type aliases.
/// If public_only is true, only types annotated with /* public */ are included.
/// If top_modules is non-empty, those modules are set as the design's top level.
SlangResult reflect_types(const SlangSession& session, bool public_only, const rust::Vec<rust::String>& top_modules, const rust::Vec<rust::String>& param_overrides, const rust::String& root_prefix);

#endif // SLANG_BRIDGE_H
