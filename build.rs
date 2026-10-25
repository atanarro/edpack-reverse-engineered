// Embeds a game into the player when EDPACK_DEMO_DAT names an EDGAME.DAT at
// build time, so `edpack` without arguments plays it.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_demo)");
    println!("cargo::rerun-if-env-changed=EDPACK_DEMO_DAT");
    let Some(demo) = std::env::var_os("EDPACK_DEMO_DAT").map(std::path::PathBuf::from) else { return };
    println!("cargo::rerun-if-changed={}", demo.display());
    if demo.is_file() {
        println!("cargo::rustc-cfg=has_demo");
        println!("cargo::rustc-env=EDPACK_DEMO={}", demo.canonicalize().unwrap().display());
    }
}
