fn main() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_arch == "wasm32" && target_os == "wasi" {
        let wasi_sdk = std::env::var("WASI_SDK_PATH")
            .expect("WASI_SDK_PATH must be set for wasm32-wasip1 builds");
        let sysroot_lib = format!("{wasi_sdk}/share/wasi-sysroot/lib/wasm32-wasip1");

        // Find crt1-reactor.o from the Rust sysroot (needed for WASI reactor model).
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
        let sysroot = std::process::Command::new(&rustc)
            .arg("--print")
            .arg("sysroot")
            .output()
            .expect("failed to run rustc --print sysroot");
        let sysroot = String::from_utf8(sysroot.stdout).unwrap();
        let sysroot = sysroot.trim();
        let crt1_reactor =
            format!("{sysroot}/lib/rustlib/wasm32-wasip1/lib/self-contained/crt1-reactor.o");

        // Emit link search path for wasi-sysroot libraries (libc++abi, etc.).
        println!("cargo:rustc-link-search=native={sysroot_lib}");

        // Link the reactor CRT object to get proper WASI reactor semantics
        // (prevents __wasm_call_dtors from being called after each export).
        println!("cargo:rustc-link-arg={crt1_reactor}");

        // Link C++ runtime libraries needed by slang.
        println!("cargo:rustc-link-arg=-lc++abi");
        println!("cargo:rustc-link-arg=-lwasi-emulated-signal");

        // Allow undefined imports (host functions provided by Extism).
        println!("cargo:rustc-link-arg=--import-undefined");
    }
}
