//! Tells the crate the target it is built for: the install proof downloads
//! the release's archive for the same machine.

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=FURCA_RELEASE_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
