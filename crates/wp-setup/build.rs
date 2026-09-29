//! Puts the manifest into the Windows installer.
//!
//! Windows looks at a program with "setup" or "install" in its name, finds
//! no manifest saying otherwise, and asks for an administrator before it
//! will start it — for an installer that installs for one person and needs
//! none. The manifest says so; it has to be inside the program, as a
//! resource, and the resource compiler that comes with the linker is what
//! puts it there. Nothing is built for any other target.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=setup.manifest");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("setup.manifest");
    // Resource 1 of type 24: the manifest a program is started with.
    let script = out.join("setup.rc");
    let named = manifest.display().to_string().replace('\\', "/");
    if std::fs::write(&script, format!("1 24 \"{named}\"\n")).is_err() {
        println!("cargo:warning=could not write the resource script");
        return;
    }
    let object = out.join("setup-manifest.o");
    let windres =
        std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".to_owned());
    let made = Command::new(&windres)
        .arg("--input")
        .arg(&script)
        .arg("--output-format=coff")
        .arg("--output")
        .arg(&object)
        .status();
    match made {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", object.display());
        }
        _ => println!(
            "cargo:warning={windres} did not compile the manifest; the installer will ask for an administrator"
        ),
    }
}
