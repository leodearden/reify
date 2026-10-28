//! The real `prj/` design files that test gates compile, named by
//! repo-relative path and read from the running tree via
//! [`crate::workspace_root::workspace_root`].
//!
//! Dependency-free (`std` only): `crates/reify-audit` carries this crate as a
//! NORMAL dependency (see this crate's `Cargo.toml`).

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{DEV_CAPSTAN, PRINTER_RI, read_design_file};

    /// A design-file path absent from the real tree, written only into the
    /// re-exec fixture workspace.
    const PROBE_REL: &str = "prj/__design_files_probe__/probe.ri";
    const PROBE_TEXT: &str = "// design_files re-exec probe\n";

    /// A design-file path absent from every tree.
    const ABSENT_REL: &str = "prj/__no_such_design__/absent.ri";

    /// Every named design file is a repo-relative `.ri` path that reads
    /// non-empty from the running tree. A relative path matters because
    /// `Path::join` discards its base for an absolute one, silently bypassing
    /// the workspace root.
    #[test]
    fn every_named_design_file_is_repo_relative_and_reads_non_empty() {
        for (name, rel) in [("PRINTER_RI", PRINTER_RI), ("DEV_CAPSTAN", DEV_CAPSTAN)] {
            assert!(
                Path::new(rel).is_relative(),
                "{name} ({rel}) must be repo-relative, not absolute"
            );
            assert!(
                rel.ends_with(".ri"),
                "{name} ({rel}) must name a `.ri` design file"
            );
            assert!(
                !read_design_file(rel).trim().is_empty(),
                "{name} ({rel}) read as empty — if the design file moved, update {name}"
            );
        }
    }

    #[test]
    fn read_design_file_follows_the_runtime_workspace_root_child() {
        crate::workspace_root::reexec::in_child(|_root| {
            assert_eq!(read_design_file(PROBE_REL), PROBE_TEXT);
        });
    }

    /// Re-execs with the runtime `CARGO_MANIFEST_DIR` inside a fixture tree
    /// holding a probe design file the real tree lacks, so a compile-time-baked
    /// root cannot pass.
    #[test]
    fn read_design_file_follows_the_runtime_workspace_root() {
        use crate::workspace_root::reexec::{Launch, fixture_workspace, run_child};

        let fixture = fixture_workspace("design-files-runtime-root-");
        let probe = fixture.path().join(PROBE_REL);
        let probe_dir = probe.parent().expect("PROBE_REL has a parent directory");
        std::fs::create_dir_all(probe_dir)
            .unwrap_or_else(|e| panic!("create {}: {e}", probe_dir.display()));
        std::fs::write(&probe, PROBE_TEXT)
            .unwrap_or_else(|e| panic!("write {}: {e}", probe.display()));

        run_child(
            "design_files::tests::read_design_file_follows_the_runtime_workspace_root_child",
            fixture.path(),
            Launch::ManifestDirEnv,
        );
    }

    /// An absent design file fails loudly, naming the absolute path tried. A
    /// reader that degraded to an empty source would let a gate compiling it
    /// pass vacuously.
    #[test]
    fn read_design_file_names_the_resolved_path_when_the_file_is_absent() {
        let payload = std::panic::catch_unwind(|| read_design_file(ABSENT_REL))
            .expect_err("reading an absent design file must panic");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .expect("panic payload is a string");

        let resolved = crate::workspace_root::workspace_root()
            .join(ABSENT_REL)
            .display()
            .to_string();
        assert!(
            message.contains(&resolved),
            "panic message lacks the resolved path {resolved}: {message}"
        );
    }
}
