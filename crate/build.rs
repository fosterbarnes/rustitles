#[cfg(windows)]
extern crate embed_resource;

#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=rustitles.rc");
    println!("cargo:rerun-if-changed=resources/rustitles_icon.ico");
    embed_resource::compile("rustitles.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();

    // Use the Windows GUI subsystem.
    println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
    println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
}

#[cfg(not(windows))]
fn main() {
    // No special configuration on Linux.
    println!("cargo:rerun-if-changed=build.rs");
}
