// Link against the system-installed libslvs (from libslvs1-dev package).
fn main() {
    println!("cargo:rustc-link-lib=slvs");
}
