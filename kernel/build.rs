//! FabricOS Kernel Build Script
//!
//! Links pre-compiled V8 static libraries when available.
//! The C/C++ compilation (libv8.a, libopenlibm.a, libv8_shim.a) is handled
//! by the external build pipeline (scripts/build_v8.sh). This build script
//! tells rustc where to find and link those artifacts.

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let build_dir = format!("{}/../build", manifest_dir);

    // Link V8 static library (produced by scripts/build_v8.sh)
    // Only link if the build directory exists (allows kernel to compile without V8)
    if std::path::Path::new(&format!("{}/v8/libv8.a", build_dir)).exists() {
        println!("cargo:rustc-link-search=native={}/v8", build_dir);
        println!("cargo:rustc-link-lib=static=v8");
        println!("cargo:rustc-cfg=v8_linked");
    }

    // Link openlibm (IEEE 754 math for JS engine)
    if std::path::Path::new(&format!("{}/openlibm/libopenlibm.a", build_dir)).exists() {
        println!("cargo:rustc-link-search=native={}/openlibm", build_dir);
        println!("cargo:rustc-link-lib=static=openlibm");
    }

    // Link V8 shim layer (libc stubs, C++ runtime, platform backend)
    if std::path::Path::new(&format!("{}/shim/libv8_shim.a", build_dir)).exists() {
        println!("cargo:rustc-link-search=native={}/shim", build_dir);
        println!("cargo:rustc-link-lib=static=v8_shim");
    }

    // Rebuild if any static libraries change
    println!("cargo:rerun-if-changed={}/v8/libv8.a", build_dir);
    println!("cargo:rerun-if-changed={}/openlibm/libopenlibm.a", build_dir);
    println!("cargo:rerun-if-changed={}/shim/libv8_shim.a", build_dir);
}
