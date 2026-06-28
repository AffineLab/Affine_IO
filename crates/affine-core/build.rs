use std::path::Path;
use std::process::Command;

// Stamp a build version into AFFINE_BUILD_VERSION. Prefers `git describe`, so once
// a tag like `v1.2.3` is pushed the binary reports it (e.g. `v1.2.3` or
// `v1.2.3-4-gabc1234` between tags); with no tags it falls back to the short commit
// hash, and with no git at all to the Cargo package version.
fn main() {
    let version = git_describe().unwrap_or_else(|| {
        std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string())
    });
    println!("cargo:rustc-env=AFFINE_BUILD_VERSION={version}");

    // Refresh the stamp when HEAD or refs move (commit / checkout / tag).
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let git_dir = Path::new(&manifest).join("..").join("..").join(".git");
        for entry in ["HEAD", "packed-refs"] {
            let path = git_dir.join(entry);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    let version = version.trim();
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}
