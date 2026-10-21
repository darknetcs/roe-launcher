//! Embeds the Windows resources (icon, version information, manifest) into roe-launcher.exe.
//! Nothing happens for other targets. Works with MSVC (rc.exe) and MinGW (windres, also when cross-compiling).
//! The .rc is a template: the version from Cargo.toml and the resource folder are filled in here, because windres
//! passes preprocessor definitions through a shell that drops the quotes of a string macro.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=windows/roe-launcher.rc");
    println!("cargo:rerun-if-changed=windows/roe-launcher.manifest");
    println!("cargo:rerun-if-changed=windows/roe-launcher.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let dir = PathBuf::from(env("CARGO_MANIFEST_DIR")).join("windows");
    // forward slashes work for both compilers and need no escaping inside rc strings
    let dir = dir.display().to_string().replace('\\', "/");
    let template = std::fs::read_to_string("windows/roe-launcher.rc").expect("windows/roe-launcher.rc");
    let rc = template
        .replace("@DIR@", &dir)
        .replace("@MAJOR@", &env("CARGO_PKG_VERSION_MAJOR"))
        .replace("@MINOR@", &env("CARGO_PKG_VERSION_MINOR"))
        .replace("@PATCH@", &env("CARGO_PKG_VERSION_PATCH"))
        .replace("@VERSION@", &env("CARGO_PKG_VERSION"));
    let out = PathBuf::from(env("OUT_DIR")).join("roe-launcher.rc");
    std::fs::write(&out, rc).expect("writing the resource script");
    embed_resource::compile(&out, embed_resource::NONE).manifest_required().unwrap();
}
