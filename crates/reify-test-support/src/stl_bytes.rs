//! Read back what `reify_ir::write_stl_binary` wrote.
//!
//! Binary STL is 80 header bytes, a `u32` little-endian triangle count at
//! 80..84, then a 50-byte record per triangle: a 12-byte facet normal, 9 f32
//! vertex coordinates, and a 2-byte attribute count.
//!
//! This module is deliberately NOT glob-exported from `lib.rs` (see the note
//! there); readers write `use reify_test_support::stl_bytes::binary_stl_aabb;`.

/// The per-axis AABB of every vertex in a binary STL payload, plus its
/// triangle count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StlAabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub triangles: usize,
}

impl StlAabb {
    /// `max - min` per axis.
    pub fn extent(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

/// Measure a binary STL payload, asserting its byte length agrees with its
/// declared triangle count.
///
/// An extent is the right invariant to measure: it survives everything that
/// legitimately varies — the tessellator's triangle count, facet ordering, and
/// where the body is centred — while remaining a strictly physical quantity.
/// Facet normals are skipped rather than checked: they are dimensionless unit
/// directions, so they carry no size or unit signal.
pub fn binary_stl_aabb(bytes: &[u8]) -> StlAabb {
    assert!(
        bytes.len() >= 84,
        "a binary STL must carry at least an 80-byte header and a u32 count, got {} bytes",
        bytes.len()
    );
    let n_triangles = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(
        bytes.len(),
        84 + 50 * n_triangles,
        "binary STL byte length must equal 84 + 50*count"
    );

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for tri in 0..n_triangles {
        for v in 0..3usize {
            let vbase = 84 + tri * 50 + 12 + v * 12;
            for axis in 0..3usize {
                let raw: [u8; 4] = bytes[vbase + axis * 4..vbase + axis * 4 + 4]
                    .try_into()
                    .unwrap();
                let coord = f32::from_le_bytes(raw);
                min[axis] = min[axis].min(coord);
                max[axis] = max[axis].max(coord);
            }
        }
    }
    StlAabb {
        min,
        max,
        triangles: n_triangles,
    }
}
