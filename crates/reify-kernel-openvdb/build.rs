// Placeholder build script — replaced by real OpenVDB detection in step-2.
// Pre-step-2: declare has_openvdb as a known cfg so rustc doesn't warn.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_openvdb)");
}
