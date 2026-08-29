use std::env;

fn main() {
    println!("cargo:rerun-if-changed=src/controller/saturn_bridge.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdrt.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdrt.h");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdsun.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdsun.h");

    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("wasm32") {
        // Browser support uses the same controller metadata today. The exact C
        // runtime will be linked as a WASM package in the Studio slice rather
        // than silently replacing it with a second interpreter here.
        return;
    }

    cc::Build::new()
        .file("src/controller/saturn_bridge.c")
        .file("third_party/fbd-runtime/fbdrt.c")
        .file("third_party/fbd-runtime/fbdsun.c")
        .include("third_party/fbd-runtime")
        .flag_if_supported("-std=c11")
        .flag_if_supported("-Wno-implicit-fallthrough")
        .compile("firmverse_saturn_fbd");

    if target.contains("linux") || target.contains("darwin") {
        println!("cargo:rustc-link-lib=m");
    }
}
