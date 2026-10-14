//! The compiler as a library, for WebAssembly: the browser playground (`src/wasm.rs`, built by
//! `tools/build_wasm.py`). On every other target this library is empty and the command line
//! (`src/main.rs`) is the compiler.
//!
//! The WebAssembly build is the command line's own source with its C-ABI exports: what needs an
//! operating system (the C compiler, files, processes, threads) is still compiled but never
//! called, so its code is unused there.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports, unused_variables))]

#[cfg(target_arch = "wasm32")]
include!("main.rs");
