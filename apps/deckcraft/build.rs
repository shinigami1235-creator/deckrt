//! Windows only: embed the app icon and version info (VERSIONINFO) into `deckcraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `DECKCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/deckcraft.ico");
    println!("cargo:rerun-if-env-changed=DECKCRAFT_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=DECKRT_BUILD_NUMBER");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/deckcraft.ico")
        .set("ProductName", "deckrt")
        .set("FileDescription", "deckrt presentations (a fork of DeckCraft)")
        .set("CompanyName", "deckrt")
        .set("LegalCopyright", "Copyright (c) the DeckCraft authors and the deckrt contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "deckrt.exe")
        .set("InternalName", "deckrt");
    // deckrt: each build's exe gets its own file version, <version>.<CI run number>, so the
    // installer always sees the newer exe as newer and Explorer's Details tab tells builds apart.
    let (major, minor, patch, build) = file_version();
    let dotted = format!("{major}.{minor}.{patch}.{build}");
    res.set_version_info(winresource::VersionInfo::FILEVERSION, (major << 48) | (minor << 32) | (patch << 16) | build).set("FileVersion", &dotted);
    if let Err(e) = res.compile() {
        if std::env::var_os("DECKCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=deckcraft.exe built without icon/version resources: {e}");
    }
}

/// The exe's file version: the crate version's three numbers and the CI build number
/// (`DECKRT_BUILD_NUMBER`, 0 for a local build), each held to 16 bits.
fn file_version() -> (u64, u64, u64, u64) {
    let v = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut n = v.split(['.', '-', '+']).map(|p| p.parse::<u64>().unwrap_or(0).min(0xFFFF));
    let (major, minor, patch) = (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0));
    let build = std::env::var("DECKRT_BUILD_NUMBER").ok().and_then(|b| b.trim().parse::<u64>().ok()).unwrap_or(0) % 0x10000;
    (major, minor, patch, build)
}
