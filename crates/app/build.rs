use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=../../data/icons");

    let mut blueprints: Vec<PathBuf> = std::fs::read_dir("data/ui")
        .expect("data/ui exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "blp"))
        .collect();
    blueprints.sort();

    let status = Command::new("blueprint-compiler")
        .arg("batch-compile")
        .arg(out_dir.join("ui"))
        .arg("data/ui")
        .args(&blueprints)
        .status()
        .expect("blueprint-compiler is needed to build the UI (sudo dnf install blueprint-compiler)");
    assert!(status.success(), "blueprint-compiler failed");

    // The app icon also lives in the resource, so the window shows it before installation.
    let icons_dir = PathBuf::from("../../data");
    glib_build_tools::compile_resources(
        &[out_dir.as_path(), "data".as_ref(), icons_dir.as_path()],
        "data/resources.gresource.xml",
        "permafrost.gresource",
    );
}
