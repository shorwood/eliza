/// Enable embedded Scalar when Nix supplies the pinned JavaScript asset.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(embedded_scalar)");
    println!("cargo:rerun-if-env-changed=ELIZA_SCALAR_JS");
    // Plain Cargo builds retain Aide's bundled viewer.
    if std::env::var_os("ELIZA_SCALAR_JS").is_none() {
        return;
    }
    println!("cargo:rustc-cfg=embedded_scalar");
}
