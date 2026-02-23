#[cfg(windows)]
extern crate winres;

#[cfg(windows)]
extern crate embed_resource;

#[cfg(windows)]
fn main() {
    println!("cargo:warning=Build script is running on Windows");
    
    // Check if icon file exists
    let icon_path = "resources/rustitles_icon.ico";
    if !std::path::Path::new(icon_path).exists() {
        println!("cargo:warning=Icon file does not exist!");
        std::process::exit(1);
    }
    
    // Use manual resource compilation
    let rc_file = "rustitles.rc";
    let res_file = "rustitles.res";
    
    // Compile the resource file
    let output = std::process::Command::new("rc")
        .args(&["/fo", res_file, rc_file])
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                println!("cargo:warning=Resource compilation successful");
                // Link the resource file
                println!("cargo:rustc-link-arg={}", res_file);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("cargo:warning=Resource compilation failed: {}", stderr);
                // Fallback to winres
                println!("cargo:warning=Falling back to winres...");
                fallback_to_winres(icon_path);
            }
        }
        Err(e) => {
            println!("cargo:warning=Failed to run rc compiler: {}", e);
            // Fallback to winres
            println!("cargo:warning=Falling back to winres...");
            fallback_to_winres(icon_path);
        }
    }
    
    // Explicitly set the Windows subsystem to prevent console window
    println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
    println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
}

#[cfg(windows)]
fn fallback_to_winres(icon_path: &str) {
    extern crate winres;
    let mut res = winres::WindowsResource::new();
    res.set_icon(icon_path);
    res.set("SubSystem", "Windows");
    res.set("FileDescription", "Rustitles - Subtitle Downloader");
    res.set("ProductName", "Rustitles");
    
    match res.compile() {
        Ok(_) => println!("cargo:warning=Icon embedded successfully with winres fallback"),
        Err(e) => {
            println!("cargo:warning=Both methods failed! winres error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    println!("cargo:warning=Build script is running on Linux");
    println!("cargo:rerun-if-changed=build.rs");
}

#[cfg(target_os = "macos")]
fn main() {
    println!("cargo:warning=Build script is running on macOS");
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=10.13");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/rustitles_icon.png");
}