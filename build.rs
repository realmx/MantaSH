//! Embed the application icon in the Windows desktop executable.
fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/mantash.rc");
    println!("cargo:rerun-if-changed=assets/mantash.ico");
    #[cfg(feature = "desktop")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GPUI loads icon resource 1 from the executable; the manifest remains
        // supplied by GPUI's windows-manifest feature.
        embed_resource::compile_for(
            "packaging/windows/mantash.rc",
            ["mantash"],
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("compile the MantaSH Windows application icon");
    }
}
