//! Links clang's builtins runtime on macOS (rust-lang/rust#164915).
//!
//! ort 2.0.0-rc.14+ no longer links `libclang_rt.osx.a`, and fat LTO drops
//! std's `__isPlatformVersionAtLeast`, which ONNX Runtime's `CoreML` objects
//! need. `-fapple-link-rtlib` makes clang add the archive to the final link.
//! Delete this file once rust-lang/rust#164915 is fixed.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg=-fapple-link-rtlib");
    }
}
