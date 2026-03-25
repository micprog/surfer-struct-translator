// Slang session management for surfer-struct-gen
// Based on bender-slang by Tim Fischer <fischeti@iis.ee.ethz.ch>

#include "slang_bridge.h"
#include "surfer-struct-gen/src/slang.rs.h"

using namespace slang;
using namespace slang::syntax;

using std::shared_ptr;
using std::string;
using std::string_view;

std::unique_ptr<SlangSession> new_slang_session() { return std::make_unique<SlangSession>(); }

SlangContext::SlangContext() : diagEngine(sourceManager), diagClient(std::make_shared<TextDiagnosticClient>()) {
    diagEngine.addClient(diagClient);
}

rust::String SlangContext::set_includes(const rust::Vec<rust::String>& incs) {
    for (const auto& inc : incs) {
        std::string incStr(inc.data(), inc.size());
        if (auto ec = sourceManager.addUserDirectories(incStr); ec) {
            return rust::String("Failed to add include directory '" + incStr + "': " + ec.message());
        }
    }
    return rust::String();
}

void SlangContext::set_defines(const rust::Vec<rust::String>& defs) {
    ppOptions.predefines.reserve(defs.size());
    for (const auto& def : defs) {
        ppOptions.predefines.emplace_back(def.data(), def.size());
    }
}

// Parses a list of source files and returns the resulting syntax trees.
// On failure, sets error_out and returns an empty vector.
std::vector<std::shared_ptr<SyntaxTree>> SlangContext::parse_files(
    const rust::Vec<rust::String>& paths, rust::String& error_out) {
    Bag options;
    options.set(ppOptions);

    std::vector<std::shared_ptr<SyntaxTree>> out;
    out.reserve(paths.size());

    for (const auto& path : paths) {
        string_view pathView(path.data(), path.size());
        auto result = SyntaxTree::fromFile(pathView, sourceManager, options);

        if (!result) {
            auto& err = result.error();
            error_out = rust::String("System Error loading '" + std::string(err.second) + "': " + err.first.message());
            return {};
        }

        auto tree = *result;
        diagClient->clear();
        diagEngine.clearIncludeStack();

        bool hasErrors = false;
        for (const auto& diag : tree->diagnostics()) {
            hasErrors |= diag.isError();
            diagEngine.issue(diag);
        }

        if (hasErrors) {
            std::string rendered = diagClient->getString();
            if (rendered.empty()) {
                rendered = "Failed to parse '" + std::string(pathView) + "'.";
            }
            error_out = rust::String(rendered);
            return {};
        }

        out.push_back(tree);
    }

    return out;
}

// Parses a group of files with the given include paths and preprocessor defines.
SlangResult SlangSession::parse_group(const rust::Vec<rust::String>& files, const rust::Vec<rust::String>& includes,
                               const rust::Vec<rust::String>& defines) {
    auto ctx = std::make_unique<SlangContext>();

    auto inc_err = ctx->set_includes(includes);
    if (!inc_err.empty()) {
        return SlangResult{rust::String(), std::move(inc_err)};
    }

    ctx->set_defines(defines);

    rust::String parse_err;
    auto parsed = ctx->parse_files(files, parse_err);
    if (!parse_err.empty()) {
        return SlangResult{rust::String(), std::move(parse_err)};
    }

    allTrees.reserve(allTrees.size() + parsed.size());
    for (const auto& tree : parsed) {
        allTrees.push_back(tree);
    }

    contexts.push_back(std::move(ctx));
    return SlangResult{rust::String(), rust::String()};
}

// Returns the number of syntax trees currently stored in the session.
std::size_t tree_count(const SlangSession& session) { return session.trees().size(); }
