// Type reflection for surfer-struct-gen
// Based on bender-slang by Michael Rogenmoser

#include "slang_bridge.h"
#include "surfer-struct-gen/src/slang.rs.h"

#include "slang/ast/ASTVisitor.h"
#include "slang/ast/Compilation.h"
#include "slang/ast/Scope.h"
#include "slang/ast/symbols/InstanceSymbols.h"
#include "slang/ast/symbols/PortSymbols.h"
#include "slang/ast/symbols/VariableSymbols.h"
#include "slang/ast/types/AllTypes.h"
#include "slang/syntax/SyntaxVisitor.h"

#include <ranges>
#include <set>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

using namespace slang;
using namespace slang::ast;

// Escape a string for JSON output.
static std::string json_escape(std::string_view s) {
    std::string out;
    out.reserve(s.size());
    for (char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default: out += c; break;
        }
    }
    return out;
}

// Format an integer value as a binary string of the given width.
static std::string to_binary_string(uint64_t value, size_t width) {
    std::string result(width, '0');
    for (size_t i = 0; i < width && i < 64; i++) {
        if (value & (1ULL << i))
            result[width - 1 - i] = '1';
    }
    return result;
}

// Check if a type alias has a /* public */ directive on its semicolon token.
// Reuses the same pattern as SvTypeReflector in slang-reflect.
class PublicChecker : public slang::syntax::SyntaxVisitor<PublicChecker> {
public:
    explicit PublicChecker(const slang::parsing::TokenKind tokenKind) :
        tokenKind(tokenKind) {}

    void visitToken(const slang::parsing::Token token) {
        if (token.kind == tokenKind) {
            auto blockComments = token.trivia() | std::views::filter([](auto& v) {
                                     return v.kind == slang::parsing::TriviaKind::BlockComment;
                                 });
            for (auto& blockComment : blockComments) {
                auto text = blockComment.getRawText();
                if (text == "/* public */" || text == "/*verilator public*/" || text == "/* verilator public */") {
                    isPublic = true;
                }
            }
        }
    }

    bool operator()() { return std::exchange(isPublic, false); }

private:
    bool isPublic{false};
    slang::parsing::TokenKind tokenKind;
};

// Unwrap a type through aliases and packed arrays to find the innermost
// element type.  Returns the element type and the array dimensions
// (empty when the type is not an array).
//
// Dimensions are reported when:
// - The innermost element is a struct or enum (array of structs/enums).
// - There are 2+ nested PackedArrayType layers (multi-dimensional bit vector,
//   e.g. logic [2:0][3:0]).
//
// A single PackedArrayType layer (e.g. logic [7:0]) is treated as a plain
// bit vector with no dimensions.
static std::pair<const Type*, std::vector<size_t>> unwrap_to_element(const Type& type) {
    const auto& canonical = type.getCanonicalType();
    if (canonical.kind == SymbolKind::PackedArrayType) {
        const auto& arr = canonical.as<PackedArrayType>();
        auto [inner, innerDims] = unwrap_to_element(arr.elementType);

        if (inner->isStruct() || inner->isEnum()) {
            // Array of structs/enums: collect all dimensions.
            innerDims.insert(innerDims.begin(), static_cast<size_t>(arr.range.width()));
            return {inner, innerDims};
        }

        // For scalars: if the immediate element is itself a packed array,
        // this is a multi-dimensional bit vector (e.g. logic [M:0][N:0]).
        // Report the outer dimensions; the innermost layer is the element width.
        if (arr.elementType.getCanonicalType().kind == SymbolKind::PackedArrayType) {
            innerDims.insert(innerDims.begin(), static_cast<size_t>(arr.range.width()));
            return {inner, innerDims};
        }
    }
    return {&canonical, {}};
}

// Get the type alias name for a field, resolving through aliases.
// Also unwraps packed arrays to find the underlying struct/enum alias.
static std::string_view field_type_name(const Type& type) {
    if (type.isAlias()) {
        const auto& alias = type.as<TypeAliasType>();
        return alias.name;
    }
    const auto& canonical = type.getCanonicalType();
    if (canonical.isStruct() || canonical.isEnum()) {
        return canonical.name;
    }
    // Unwrap packed arrays to find the element's alias name.
    if (canonical.kind == SymbolKind::PackedArrayType) {
        const auto& arr = canonical.as<PackedArrayType>();
        return field_type_name(arr.elementType);
    }
    return "";
}

// Determine the kind of a field's type, unwrapping packed arrays.
static std::string field_kind(const Type& type) {
    auto [elem, _] = unwrap_to_element(type);
    if (elem->isStruct()) return "struct";
    if (elem->isEnum()) return "enum";
    return "scalar";
}

// Serialize a dimension vector as a JSON array string, e.g. "[2,3,4]".
static std::string dims_to_json(const std::vector<size_t>& dims) {
    std::ostringstream s;
    s << "[";
    for (size_t i = 0; i < dims.size(); i++) {
        if (i > 0) s << ",";
        s << dims[i];
    }
    s << "]";
    return s.str();
}

SlangResult reflect_types(const SlangSession& session, bool public_only, const rust::Vec<rust::String>& top_modules, const rust::Vec<rust::String>& param_overrides, const rust::String& root_prefix) {
    // Create a compilation from all parsed syntax trees.
    CompilationOptions options;
    // Tolerate unknown modules so partial designs can still be reflected.
    options.flags |= CompilationFlags::IgnoreUnknownModules;
    for (const auto& top : top_modules) {
        options.topModules.emplace(std::string_view(top.data(), top.size()));
    }
    for (const auto& p : param_overrides) {
        options.paramOverrides.emplace_back(std::string(p.data(), p.size()));
    }

    auto compilation = std::make_unique<Compilation>(options);
    for (const auto& tree : session.trees()) {
        compilation->addSyntaxTree(tree);
    }

    // Force elaboration.
    compilation->getRoot();

    // Collected types.
    struct FieldInfo {
        std::string name;
        size_t width;               // total bit width (element_width * product(dims))
        std::string kind;           // "scalar", "struct", "enum"
        std::string type_name;      // name of referenced struct/enum type, empty for scalar
        std::vector<size_t> dims;   // array dimensions (empty = scalar)
    };
    struct StructInfo {
        std::string name;
        std::vector<FieldInfo> fields;
    };
    struct EnumValueInfo {
        std::string name;
        uint64_t value;
    };
    struct EnumInfo {
        std::string name;
        size_t width;
        std::vector<EnumValueInfo> values;
    };

    std::vector<StructInfo> structs;
    std::vector<EnumInfo> enums;

    // Map from canonical packed struct type to the alias name, so we can
    // resolve the type name even when a signal's type isn't an alias.
    std::unordered_map<const Type*, std::string> canonical_to_alias;

    // Visitor for checking /* public */ annotations on semicolons.
    static auto publicChecker = PublicChecker(parsing::TokenKind::Semicolon);

    // Visit all TypeAliasType nodes in the compilation.
    compilation->getRoot().visit(makeVisitor([&](auto&, const TypeAliasType& type) {
        // Optionally filter to only /* public */ annotated types.
        if (public_only) {
            if (!type.getSyntax())
                return;
            type.getSyntax()->visit(publicChecker);
            if (!publicChecker())
                return;
        }

        if (type.isStruct()) {
            const auto& canonical = type.getCanonicalType();

            // Only handle packed structs (these are the ones flattened to bit vectors).
            if (canonical.kind != SymbolKind::PackedStructType)
                return;

            StructInfo info;
            info.name = std::string(type.name);

            for (const auto& member : canonical.as<Scope>().members()) {
                const auto& variable = member.as<VariableSymbol>();
                auto [elem, dims] = unwrap_to_element(variable.getType());
                FieldInfo field;
                field.name = std::string(variable.name);
                field.width = variable.getType().getBitstreamWidth();
                field.kind = field_kind(variable.getType());
                field.type_name = std::string(field_type_name(variable.getType()));
                field.dims = dims;
                info.fields.push_back(std::move(field));
            }

            // Slang iterates fields in declaration order, which is MSB-first
            // for packed structs. This matches Verilator's packing order.
            canonical_to_alias.emplace(&canonical, std::string(type.name));
            structs.push_back(std::move(info));
        }
        else if (type.isEnum()) {
            EnumInfo info;
            info.name = std::string(type.name);
            info.width = type.getBitstreamWidth();

            for (const auto& member : type.getCanonicalType().as<Scope>().members()) {
                const auto& enumMember = member.as<EnumValueSymbol>();
                EnumValueInfo val;
                val.name = std::string(enumMember.name);
                val.value = *enumMember.getValue().integer().getRawPtr();
                info.values.push_back(std::move(val));
            }

            enums.push_back(std::move(info));
        }
    }));

    // Track canonical types already extracted so we don't re-extract duplicates.
    std::set<const Type*> extracted_types;

    // Recursively extract struct (and nested struct) definitions from an
    // elaborated packed struct type.  This supplements the TypeAliasType pass
    // which may have wrong field widths when parameters were unresolved.
    std::function<void(const Type& elem, const std::string& name)> extract_struct_from_elaborated;
    extract_struct_from_elaborated = [&](const Type& elem, const std::string& name) {
        const auto& canonical = elem.getCanonicalType();
        if (canonical.kind != SymbolKind::PackedStructType)
            return;
        if (!extracted_types.insert(&canonical).second)
            return;  // Already extracted this exact canonical type.

        StructInfo info;
        info.name = name;

        for (const auto& member : canonical.as<Scope>().members()) {
            const auto& variable = member.as<VariableSymbol>();
            auto [fieldElem, dims] = unwrap_to_element(variable.getType());
            FieldInfo field;
            field.name = std::string(variable.name);
            field.width = variable.getType().getBitstreamWidth();
            field.kind = field_kind(variable.getType());
            field.type_name = std::string(field_type_name(variable.getType()));
            field.dims = dims;
            // Recursively extract nested struct types.
            if (fieldElem->kind == SymbolKind::PackedStructType) {
                auto nested_name = std::string(field_type_name(variable.getType()));
                if (nested_name.empty()) {
                    auto it2 = canonical_to_alias.find(fieldElem);
                    if (it2 != canonical_to_alias.end()) {
                        nested_name = it2->second;
                    } else {
                        // Anonymous nested struct — synthesize name from
                        // parent struct name + field name.
                        nested_name = "__anon_" + name + "_" + std::string(variable.name);
                        canonical_to_alias.emplace(fieldElem, nested_name);
                    }
                }
                // Update the field's type_name so the translator can
                // reference the (possibly synthesized) struct definition.
                field.type_name = nested_name;
                extract_struct_from_elaborated(*fieldElem, nested_name);
            }

            info.fields.push_back(std::move(field));
        }

        canonical_to_alias.emplace(&canonical, name);
        structs.push_back(std::move(info));
    };

    // Collect per-signal mappings from the elaborated hierarchy.
    // For each variable, net, or port whose type is a packed struct, record
    // the full hierarchical path and the type alias name.  This lets the
    // generator emit exact, per-signal mappings.
    struct SignalTypeMapping {
        std::string path;               // full hierarchical path, e.g. "top.dut.apb_req_o"
        std::string type_name;          // type alias, e.g. "apb_req_t"
        size_t width;                   // total bit width
        std::vector<size_t> dims;       // array dimensions (empty = scalar)
    };
    std::vector<SignalTypeMapping> signal_mappings;
    std::set<std::string> seen_paths;

    // Replace the leading "$root." that slang prepends with the actual root
    // scope prefix from the waveform file (e.g. "TOP.").  Falls back to "TOP."
    // when no prefix is provided.
    std::string rprefix = root_prefix.empty()
        ? std::string("TOP.")
        : std::string(root_prefix.data(), root_prefix.size()) + ".";
    auto strip_root = [&rprefix](const std::string& path) -> std::string {
        constexpr std::string_view slang_prefix = "$root.";
        if (path.substr(0, slang_prefix.size()) == slang_prefix)
            return rprefix + path.substr(slang_prefix.size());
        return rprefix + path;
    };

    auto collect_signal = [&](const Symbol& sym, const Type& type) {
        // Unwrap packed arrays to find the underlying packed struct.
        auto [elem, dims] = unwrap_to_element(type);
        if (elem->kind != SymbolKind::PackedStructType)
            return;

        // Resolve the struct type alias name.
        // Prefer the name from canonical_to_alias (which matches the struct
        // definition name from the TypeAliasType pass) over field_type_name
        // (which may return a secondary alias like "apb_req_t" when the struct
        // was defined as "uart_apb_req_t").
        std::string tname;
        auto it = canonical_to_alias.find(elem);
        if (it != canonical_to_alias.end()) {
            tname = it->second;
        } else {
            tname = std::string(field_type_name(type));
            if (tname.empty()) {
                // Anonymous packed struct (inline `struct packed { ... } sig;`)
                // — synthesize a name from the signal name so it can still be
                // decomposed.  Register in canonical_to_alias so that other
                // variables sharing the same anonymous type reuse this name.
                tname = "__anon_" + std::string(sym.name);
                canonical_to_alias.emplace(elem, tname);
            }
        }

        // Extract the struct definition from this elaborated signal's type.
        // This gives us correctly-resolved field widths even when the
        // TypeAliasType pass saw unresolved parameters.
        extract_struct_from_elaborated(*elem, tname);

        auto path = strip_root(sym.getHierarchicalPath());
        if (seen_paths.insert(path).second) {
            signal_mappings.push_back({path, tname, static_cast<size_t>(type.getBitstreamWidth()), dims});
        }
    };

    // Pass 1: collect internal variables and nets via the normal scope-member visitor.
    compilation->getRoot().visit(makeVisitor(
        [&](auto&, const VariableSymbol& var) {
            // Skip struct fields — only collect module-level signals.
            const auto* parent = var.getParentScope();
            if (parent && parent->asSymbol().kind == SymbolKind::PackedStructType)
                return;
            collect_signal(var, var.getType());
        },
        [&](auto&, const NetSymbol& net) {
            collect_signal(net, net.getType());
        }
    ));

    // Pass 2: explicitly walk instance port lists.  PortSymbols live in
    // InstanceBodySymbol::getPortList(), which is separate from the scope's
    // member list that the visitor iterates, so they are missed by pass 1.
    compilation->getRoot().visit(makeVisitor(
        [&](auto& visitor, const InstanceBodySymbol& body) {
            for (const auto* sym : body.getPortList()) {
                if (sym->kind == SymbolKind::Port) {
                    const auto& port = sym->as<PortSymbol>();
                    collect_signal(port, port.getType());
                }
            }
            // Continue recursing into the body's children so we visit
            // nested instances' port lists too.
            visitor.visitDefault(body);
        }
    ));

    // Serialize to JSON.
    std::ostringstream json;
    json << "{\"structs\":[";
    for (size_t si = 0; si < structs.size(); si++) {
        if (si > 0) json << ",";
        const auto& s = structs[si];
        json << "{\"name\":\"" << json_escape(s.name) << "\",\"fields\":[";
        for (size_t fi = 0; fi < s.fields.size(); fi++) {
            if (fi > 0) json << ",";
            const auto& f = s.fields[fi];
            json << "{\"name\":\"" << json_escape(f.name)
                 << "\",\"width\":" << f.width
                 << ",\"kind\":\"" << f.kind
                 << "\",\"type_name\":\"" << json_escape(f.type_name)
                 << "\",\"array_dims\":" << dims_to_json(f.dims) << "}";
        }
        json << "]}";
    }
    json << "],\"enums\":[";
    for (size_t ei = 0; ei < enums.size(); ei++) {
        if (ei > 0) json << ",";
        const auto& e = enums[ei];
        json << "{\"name\":\"" << json_escape(e.name) << "\",\"width\":" << e.width << ",\"values\":[";
        for (size_t vi = 0; vi < e.values.size(); vi++) {
            if (vi > 0) json << ",";
            const auto& v = e.values[vi];
            json << "{\"name\":\"" << json_escape(v.name)
                 << "\",\"value\":" << v.value
                 << ",\"binary\":\"" << to_binary_string(v.value, e.width) << "\"}";
        }
        json << "]}";
    }
    json << "],\"signal_mappings\":[";
    for (size_t mi = 0; mi < signal_mappings.size(); mi++) {
        if (mi > 0) json << ",";
        const auto& m = signal_mappings[mi];
        json << "{\"path\":\"" << json_escape(m.path)
             << "\",\"type_name\":\"" << json_escape(m.type_name)
             << "\",\"width\":" << m.width
             << ",\"array_dims\":" << dims_to_json(m.dims) << "}";
    }
    json << "]}";

    return SlangResult{rust::String(json.str()), rust::String()};
}
