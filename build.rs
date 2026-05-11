use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail, ensure};

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");

    let lib_dir = build_from_source()?;

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=pkl");
    println!("cargo:rustc-link-lib=z");

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CoreServices");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=pthread");
    }

    Ok(())
}

fn build_from_source() -> anyhow::Result<PathBuf> {
    let java_home = env::var("JAVA_HOME").context("JAVA_HOME must be set")?;
    let native_image = Path::new(&java_home).join("bin/native-image");
    if !native_image.exists() {
        bail!(
            "JAVA_HOME ({java_home}) does not contain bin/native-image. \
            Try installing a JVM from https://www.graalvm.org/downloads/."
        )
    }

    let src_dir = Path::new("./pkl");
    let gradlew = if cfg!(windows) {
        "./gradlew.bat"
    } else {
        "./gradlew"
    };

    let os = env::consts::OS;
    let arch = match env::consts::ARCH {
        "x86_64" => "amd64",
        other => other,
    };

    let build_dir = src_dir
        .join("libpkl/build/native-libs")
        .join(format!("{os}-{arch}"));

    let status = Command::new(gradlew)
        .args([":libpkl:assembleNative", "--no-daemon"])
        .env("JAVA_HOME", java_home)
        .current_dir(src_dir)
        .status()
        .context("Failed to execute pkl/gradlew")?;
    ensure!(status.success(), "Gradle :libpkl:assembleNative failed");
    ensure!(
        build_dir.join("libpkl.a").exists(),
        "libpkl.a not found in {}",
        build_dir.display()
    );

    Ok(build_dir)
}
