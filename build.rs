use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some(OsStr::new("windows")) {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let stage = out.join("icon-stage");
    fs::create_dir_all(&stage).expect("icon stage");
    fs::copy("assets/icon.ico", stage.join("icon.ico")).expect("icon copy");
    fs::write(stage.join("icon.rc"), "1 ICON \"icon.ico\"\n").expect("icon rc");

    let object = out.join("icon.o");
    let status = Command::new("windres")
        .current_dir(&stage)
        .arg("-i")
        .arg("icon.rc")
        .arg("-o")
        .arg(&object)
        .status()
        .expect("windres");

    assert!(status.success(), "windres failed");

    println!("cargo:rustc-link-arg={}", object.display());
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
