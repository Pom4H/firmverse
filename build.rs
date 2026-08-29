use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(mut command: Command, label: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to start {label}: {error}"));
    assert!(status.success(), "{label} failed with status {status}");
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(firmverse_saturn_native)");
    println!("cargo:rerun-if-changed=src/controller/saturn_bridge.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdrt.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdrt.h");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdsun.c");
    println!("cargo:rerun-if-changed=third_party/fbd-runtime/fbdsun.h");

    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("wasm32") || target.contains("msvc") {
        // Browser and MSVC will get the exact same upstream runtime through a
        // dedicated package/backend. Never substitute a second interpreter.
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let sources = [
        "src/controller/saturn_bridge.c",
        "third_party/fbd-runtime/fbdrt.c",
        "third_party/fbd-runtime/fbdsun.c",
    ];
    let mut objects = Vec::new();

    for source in sources {
        let stem = PathBuf::from(source)
            .file_stem()
            .expect("C source stem")
            .to_string_lossy()
            .into_owned();
        let object = out.join(format!("{stem}.o"));
        let mut command = Command::new(&cc);
        command.args([
            "-std=c11",
            "-O2",
            "-Uunix",
            "-Wno-implicit-fallthrough",
            "-Ithird_party/fbd-runtime",
            "-c",
            source,
            "-o",
        ]);
        command.arg(&object);
        run(command, source);
        objects.push(object);
    }

    let archive = out.join("libfirmverse_saturn_fbd.a");
    let mut command = Command::new(&ar);
    command.arg("crs").arg(&archive);
    for object in &objects {
        command.arg(object);
    }
    run(command, "Saturn FBD archive");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=firmverse_saturn_fbd");
    if target.contains("linux") || target.contains("darwin") {
        println!("cargo:rustc-link-lib=m");
    }
    println!("cargo:rustc-cfg=firmverse_saturn_native");
}
