//! Triangle counts read back from exported mesh-artifact bytes (binary STL and
//! 3MF), which `cmd_build` prints as `Triangles: N` after an `-o` mesh export.

/// Parses the exported triangle count directly from a binary STL byte
/// buffer's fixed-width header field: 80-byte header + `u32` triangle count
/// (bytes 80..84, little-endian) + 50 bytes/triangle. `write_stl_binary`
/// (reify-ir/src/geometry.rs) is the sole `Stl` writer in every kernel
/// (occt/manifold) — including multi-body compound exports — so this header
/// field is authoritative for any bytes produced by
/// `engine.build(_, ExportFormat::Stl)`. Returns `0` if `data` is shorter
/// than 84 bytes (defensive; a real binary-STL export always has the full
/// header). See [`threemf_triangle_count`] for the `ExportFormat::ThreeMF`
/// counterpart and `triangle_count_tests` below for coverage.
pub(crate) fn stl_triangle_count(data: &[u8]) -> usize {
    match data.get(80..84) {
        Some(count_bytes) => {
            u32::from_le_bytes(count_bytes.try_into().expect("slice is exactly 4 bytes")) as usize
        }
        None => 0,
    }
}

/// Parses the exported triangle count directly from `ThreeMF` bytes by
/// counting `<triangle ` element occurrences in the raw archive. `write_3mf`
/// (reify-ir/src/geometry.rs) pins every ZIP part to
/// `CompressionMethod::Stored`, specifically so the `3D/3dmodel.model` XML
/// appears literally in `data` — its doc comment sanctions substring-counting
/// `<triangle ` on raw bytes "without a zip reader", and
/// `write_3mf_box_produces_valid_3mf_package` pins the identical
/// `.matches("<triangle ").count()` technique against the unzipped XML.
///
/// Reading straight from `data` (rather than a fresh `tessellate_realizations`
/// walk) keeps this authoritative for whatever was actually written: the OCCT
/// kernel's `ThreeMF` export re-tessellates at its own hardcoded
/// `DEFAULT_STL_TESSELLATION_TOLERANCE` (0.1), not
/// `Engine::DEFAULT_TESSELLATION_TOLERANCE` (0.0001), so a tessellation
/// re-walk is not guaranteed to agree with what was actually exported. See
/// `triangle_count_tests` below for coverage.
///
/// This is a package-wide total, not a single-mesh count: if `data` ever
/// contained more than one `<mesh>` part, this would sum `<triangle ` across
/// all of them (and across any other archive section where the substring
/// happened to appear). Every export this CLI drives today writes exactly one
/// mesh part, so `Triangles: N` is the exported mesh's count in practice; this
/// is an acceptable semantic for a diagnostic print, not a correctness bug.
pub(crate) fn threemf_triangle_count(data: &[u8]) -> usize {
    const NEEDLE: &[u8] = b"<triangle ";
    data.windows(NEEDLE.len()).filter(|w| *w == NEEDLE).count()
}

#[cfg(test)]
mod triangle_count_tests {
    use super::{stl_triangle_count, threemf_triangle_count};
    use reify_ir::{Mesh, ThreeMfOptions, write_3mf, write_stl_binary};

    /// A well-formed binary-STL byte buffer with `count` baked into bytes
    /// 80..84 (little-endian `u32`), per the layout `stl_triangle_count`
    /// parses. The per-triangle payload is irrelevant to the function under
    /// test, so it is omitted entirely — only the 84-byte header is built.
    fn stl_bytes_with_count(count: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 84];
        bytes[80..84].copy_from_slice(&count.to_le_bytes());
        bytes
    }

    /// A minimal valid `Mesh` with `triangle_count` triangles, all reusing
    /// the same 3 vertices. Neither `write_stl_binary` nor `write_3mf`
    /// validate geometry (manifoldness, winding, area) — only buffer-length
    /// and index-bounds — so a degenerate repeated triangle is sufficient to
    /// drive both real writers.
    ///
    /// Each triangle's index triple is a distinct cyclic rotation of `0,1,2`
    /// (`0,1,2` / `1,2,0` / `2,0,1`, repeating), rather than every triangle
    /// sharing the literal triple `[0,1,2]`. The three vertices themselves
    /// still repeat (still degenerate/zero-area — irrelevant to the byte-count
    /// this test drives), but distinct index triples mean no two triangles are
    /// byte-identical records, so a hypothetical future dedup pass in either
    /// writer would visibly change the reported count instead of silently
    /// collapsing both writers in lockstep — keeping the parity assertion
    /// robust to writer-internals changes.
    fn repeated_triangle_mesh(triangle_count: usize) -> Mesh {
        let mut indices = Vec::with_capacity(triangle_count * 3);
        for i in 0..triangle_count {
            let r = i % 3;
            indices.push(r as u32);
            indices.push(((r + 1) % 3) as u32);
            indices.push(((r + 2) % 3) as u32);
        }
        Mesh { vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], indices, normals: None }
    }

    /// Normal case: reads the count straight out of the header field.
    #[test]
    fn stl_triangle_count_reads_header_field() {
        assert_eq!(stl_triangle_count(&stl_bytes_with_count(7)), 7);
    }

    /// Defensive case: a buffer shorter than the 84-byte header (e.g. an
    /// empty/degenerate export) must report `0`, not panic —
    /// `data.get(80..84)` returns `None` rather than indexing out of bounds.
    #[test]
    fn stl_triangle_count_defensive_on_short_buffer() {
        assert_eq!(stl_triangle_count(&[]), 0);
        assert_eq!(stl_triangle_count(&[0u8; 10]), 0);
    }

    /// Normal case: counts `<triangle ` elements in a real `write_3mf` output.
    #[test]
    fn threemf_triangle_count_reads_real_archive() {
        let mesh = repeated_triangle_mesh(7);
        let mut buf = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut buf).expect("write_3mf should succeed");
        assert_eq!(threemf_triangle_count(&buf), 7);
    }

    /// An empty buffer (no `<triangle ` element at all) reports `0`, not a
    /// panic.
    #[test]
    fn threemf_triangle_count_empty_is_zero() {
        assert_eq!(threemf_triangle_count(&[]), 0);
    }

    /// Cross-writer parity: `stl_triangle_count` and `threemf_triangle_count`
    /// must report the same count when reading the REAL bytes
    /// `reify_ir::write_stl_binary` / `reify_ir::write_3mf` emit for the SAME
    /// mesh. Deriving both from one shared `Mesh` via the actual production
    /// writers — rather than two independently hand-built buffers — is what
    /// makes this a genuine dual-source check instead of a tautology.
    #[test]
    fn triangle_count_derivations_agree_for_same_mesh_via_real_writers() {
        let triangle_count = 11;
        let mesh = repeated_triangle_mesh(triangle_count);

        let mut stl_bytes = Vec::new();
        write_stl_binary(&mesh, &mut stl_bytes).expect("write_stl_binary should succeed");

        let mut mf_bytes = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut mf_bytes)
            .expect("write_3mf should succeed");

        let stl = stl_triangle_count(&stl_bytes);
        let mf = threemf_triangle_count(&mf_bytes);
        assert_eq!(stl, triangle_count);
        assert_eq!(mf, triangle_count);
        assert_eq!(
            stl, mf,
            "stl_triangle_count and threemf_triangle_count must report the \
             same count when reading the real write_stl_binary/write_3mf \
             output for the same mesh"
        );
    }
}
