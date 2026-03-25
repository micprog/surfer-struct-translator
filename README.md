# Surfer Struct Translator

A [Surfer](https://surfer-project.org/) plugin that decomposes packed SystemVerilog structs and enums in waveform viewers.

Verilator (and other simulators) flatten packed structs into wide bit vectors when writing VCD/FST waveforms. This plugin restores the original struct hierarchy inside Surfer, showing named fields, nested structs, enums with symbolic values, and arrays.

## Building

The plugin compiles to WebAssembly (wasm32-wasip1) and requires [wasi-sdk](https://github.com/WebAssembly/wasi-sdk) for the C++ slang library.

### Prerequisites

- Rust toolchain with the `wasm32-wasip1` target: `rustup target add wasm32-wasip1`
- [wasi-sdk](https://github.com/WebAssembly/wasi-sdk/releases) (tested with v32)

### Configuration

Update `.cargo/config.toml` to point to your wasi-sdk installation. The default paths assume `~/tools/wasi-sdk-32.0-arm64-macos/`.

### Build the WASM plugin

```sh
cargo build --release
```

The plugin binary is at `target/wasm32-wasip1/release/surfer_struct_translator.wasm`.

### Build the CLI tool (optional)

The `surfer-struct-gen` CLI can generate a `struct_defs.toml` configuration file from SystemVerilog sources without using the plugin's on-the-fly parsing:

```sh
cargo build -p surfer-struct-gen --release --target <your-host-triple>
```

## Installation

Copy the `.wasm` file into the `.surfer/translators/` directory next to your waveform file (or into Surfer's global config directory):

```sh
mkdir -p .surfer/translators
cp target/wasm32-wasip1/release/surfer_struct_translator.wasm .surfer/translators/
```

Then create a configuration file in the same directory (see below).

## Configuration

The plugin supports three configuration modes, checked in order:

### Mode 1: On-the-fly parsing from SystemVerilog sources

Create `.surfer/translators/struct_config.toml`:

```toml
[sources]
flist = ["path/to/sources.f"]
top_modules = ["my_top_module"]
param_overrides = ["WIDTH=64"]
auto_map = true
```

The plugin will parse the SystemVerilog sources at startup using [slang](https://github.com/MikePopoloski/slang) (compiled into the WASM binary) and automatically generate struct definitions and signal mappings.

| Field              | Description                                              | Default |
|--------------------|----------------------------------------------------------|---------|
| `flist`            | Verilator-style file lists (`.f` files)                  | `[]`    |
| `files`            | Individual source files                                  | `[]`    |
| `includes`         | Include directories                                      | `[]`    |
| `defines`          | Preprocessor defines                                     | `[]`    |
| `top_modules`      | Top module(s) for elaboration                            | `[]`    |
| `param_overrides`  | Parameter overrides (e.g. `"NrLanes=4"`)                 | `[]`    |
| `public_only`      | Only extract types marked `/* public */`                  | `false` |
| `auto_map`         | Auto-generate signal-to-struct mappings from the design   | `true`  |
| `mappings`         | Manual mappings (`"pattern=struct_type"`)                 | `[]`    |

### Mode 2: Pre-generated definitions file

Create `.surfer/translators/struct_config.toml`:

```toml
struct_defs_file = "struct_defs.toml"
```

Generate the definitions file using the CLI tool:

```sh
surfer-struct-gen -f sources.f --top my_top --auto-map -o .surfer/translators/struct_defs.toml
```

### Mode 3: Direct definitions file (backward compatible)

Place a `struct_defs.toml` directly in `.surfer/translators/`. No `struct_config.toml` is needed.

### Definitions file format

The `struct_defs.toml` file contains enum definitions, struct definitions, and signal-to-struct mappings:

```toml
[enums.burst_t]
width = 2
values = { "00" = "FIXED", "01" = "INCR", "10" = "WRAP" }

[structs.aw_chan_t]
[[structs.aw_chan_t.fields]]
name = "id"
width = 5
[[structs.aw_chan_t.fields]]
name = "burst"
enum_type = "burst_t"

[structs.req_t]
[[structs.req_t.fields]]
name = "aw"
struct_type = "aw_chan_t"
[[structs.req_t.fields]]
name = "aw_valid"
width = 1

[[mappings]]
pattern = "TOP.dut.axi_req_o"
struct_type = "req_t"
num_bits = 112
```

Fields are listed MSB-first (Verilator packing order). Each field can be a plain bit vector (`width`), a nested struct (`struct_type`), or an enum (`enum_type`). Array fields use `array_size` to specify the element count.

## License

Apache-2.0 — see [LICENSE](LICENSE) for details.
