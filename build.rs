// Embeds the demo game (../DEMO/EDGAME.DAT) into the player when the crate is
// built inside the EDPACK repository, so `edpack` without arguments plays it.
fn main() {
    let demo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../DEMO/EDGAME.DAT");
    println!("cargo::rustc-check-cfg=cfg(has_demo)");
    println!("cargo::rerun-if-changed={}", demo.display());
    if demo.is_file() {
        println!("cargo::rustc-cfg=has_demo");
        println!("cargo::rustc-env=EDPACK_DEMO={}", demo.canonicalize().unwrap().display());
    }
}
