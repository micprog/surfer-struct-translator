// Build script for surfer-struct-gen
// Based on bender-slang build.rs by Tim Fischer <fischeti@iis.ee.ethz.ch>

fn main() {
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let build_profile = std::env::var("PROFILE").unwrap();

    let is_wasi = target_arch == "wasm32" && target_os == "wasi";

    let cmake_profile = match (target_env.as_str(), build_profile.as_str()) {
        ("msvc", _) => "RelWithDebInfo",
        _ if is_wasi => "Release",
        (_, "debug") => "Debug",
        _ => "Release",
    };

    // Create the configuration builder
    let mut slang_lib = cmake::Config::new(".");

    // Defines for the C++ bridge build (cxx_build).
    // These must match slang's own compile definitions to avoid ABI mismatches.
    let mut bridge_defines: Vec<(&str, &str)> = vec![("SLANG_BOOST_SINGLE_HEADER", "1")];

    let common_cxx_flags: Vec<&str> = if is_wasi {
        // WASI build: no threads, no mimalloc, no exceptions.
        let wasi_sdk = std::env::var("WASI_SDK_PATH")
            .expect("WASI_SDK_PATH must be set for wasm32-wasip1 builds (set in .cargo/config.toml or environment)");

        let toolchain_file = format!("{wasi_sdk}/share/cmake/wasi-sdk-p1.cmake");

        // Configure cmake with WASI toolchain. slang's CMakeLists.txt handles
        // the WASI-specific settings (fno-exceptions, signal emulation, etc.)
        // when CMAKE_SYSTEM_NAME=WASI (set by the toolchain file).
        slang_lib
            .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
            .define("WASI_SDK_PREFIX", &wasi_sdk)
            .define("SLANG_USE_THREADS", "OFF")
            .define("SLANG_USE_MIMALLOC", "OFF");

        // Bridge defines must match what slang was built with.
        bridge_defines.push(("SLANG_USE_MIMALLOC", "0"));
        bridge_defines.push(("SLANG_USE_THREADS", "0"));

        vec!["-std=c++20", "-fno-exceptions"]

        // Note: CC_wasm32_wasip1, CXX_wasm32_wasip1, AR_wasm32_wasip1 must be set
        // in .cargo/config.toml or the environment so that all crate build scripts
        // (including cxx's own) use the wasi-sdk compilers.
    } else {
        // Native build: threads and mimalloc enabled.
        bridge_defines.push(("SLANG_USE_MIMALLOC", "1"));
        bridge_defines.push(("SLANG_USE_THREADS", "1"));

        if build_profile == "debug" && (target_env != "msvc") {
            bridge_defines.push(("SLANG_DEBUG", "1"));
            bridge_defines.push(("SLANG_ASSERT_ENABLED", "1"));
        }

        if target_env == "msvc" {
            vec!["/std:c++20", "/EHsc", "/utf-8"]
        } else {
            vec!["-std=c++20"]
        }
    };

    // Apply cmake configuration for Slang library
    slang_lib
        .define("SLANG_INCLUDE_TESTS", "OFF")
        .define("SLANG_INCLUDE_TOOLS", "OFF")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .define("CMAKE_DISABLE_FIND_PACKAGE_fmt", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_mimalloc", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Boost", "ON")
        .profile(cmake_profile);

    // For native builds, pass defines as cxxflags to cmake so they apply to
    // both slang and any cmake-built code. For WASI, slang's cmake handles
    // its own defines via the SLANG_USE_* options; we only need to ensure
    // the bridge build gets the right defines.
    if !is_wasi {
        for (def, value) in bridge_defines.iter() {
            slang_lib.define(def, *value);
            slang_lib.cxxflag(format!("-D{}={}", def, value));
        }
    }
    for flag in common_cxx_flags.iter() {
        slang_lib.cxxflag(flag);
    }

    // Build the slang library
    let dst = slang_lib.build();
    let slang_lib_dir = dst.join("build/_deps/slang-build/lib");
    let slang_include_dir = dst.join("build/_deps/slang-src/include");
    let slang_generated_include_dir = dst.join("build/_deps/slang-build/source");
    let fmt_include_dir = dst.join("build/_deps/fmt-src/include");

    // Configure Linker to find Slang static library
    println!("cargo:rustc-link-search=native={}", slang_lib_dir.display());
    println!("cargo:rustc-link-lib=static=svlang");

    if is_wasi {
        // WASI: only link fmt (no mimalloc, no threads)
        println!("cargo:rustc-link-lib=static=fmt");
    } else {
        let (fmt_lib, mimalloc_lib) = match (target_env.as_str(), build_profile.as_str()) {
            ("msvc", _) => ("fmt", "mimalloc"),
            (_, "debug") => ("fmtd", "mimalloc-debug"),
            _ => ("fmt", "mimalloc"),
        };

        println!("cargo:rustc-link-lib=static={fmt_lib}");
        println!("cargo:rustc-link-lib=static={mimalloc_lib}");

        if target_os == "windows" {
            println!("cargo:rustc-link-lib=advapi32");
        }
    }

    // Compile the C++ Bridge
    let mut bridge_build = cxx_build::bridge("src/slang.rs");
    bridge_build
        .file("cpp/session.cpp")
        .file("cpp/reflect.cpp")
        .flag_if_supported("-std=c++20")
        .include(&slang_include_dir)
        .include(&slang_generated_include_dir)
        .include(dst.join("slang-external"))
        .include(&fmt_include_dir);

    for (def, value) in bridge_defines.iter() {
        bridge_build.define(def, *value);
    }
    for flag in common_cxx_flags.iter() {
        bridge_build.flag(flag);
    }

    bridge_build.compile("slang-bridge");

    println!("cargo:rerun-if-changed=src/slang.rs");
    println!("cargo:rerun-if-changed=cpp/slang_bridge.h");
    println!("cargo:rerun-if-changed=cpp/session.cpp");
    println!("cargo:rerun-if-changed=cpp/reflect.cpp");
}
