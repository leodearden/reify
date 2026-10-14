//! Tet-mesh readback out of the process-global gmsh model.
//!
//! A broken mesher's characteristic output is an EMPTY element buffer (see
//! [`crate::init::mesh_generate_with_recovery`]), so all three tet meshers read
//! their tets back through one shared, checked call,
//! [`read_tet_connectivity`], rather than each validating the buffer itself.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use reify_ir::{ElementOrderTag, GeometryError};

use crate::ffi;

/// Read this call's tetrahedra back out of gmsh, rejecting a buffer that
/// cannot be a real mesh: a length that is not a whole number of tets, or an
/// EMPTY buffer.
///
/// The emptiness half is the output-side twin of the empty-INPUT rejection in
/// [`crate::GmshKernel::mesh_to_volume`]: gmsh accepts the degenerate case and
/// yields a zero-tet mesh, which is never a useful caller outcome. Stating it
/// once, here, is what keeps that rejection uniform across the three tet
/// meshers rather than enforced in whichever of them was edited last. This
/// crate's fourth mesher, [`crate::mesh_profile_2d::mesh_plane_2d`], reads
/// back triangles and quads instead of tets, so it cannot share this call; it
/// carries the 2D form of the same rejection at its own readback.
///
/// Since every `mesh_generate` in this crate routes through
/// [`crate::init::mesh_generate_with_recovery`], no path measured today reaches the empty
/// case. It is the backstop for the ones not measured: a future gmsh version,
/// a mesher added later that forgets the recovery wrapper, an HXT that
/// reports `ierr=0` having produced nothing at all.
///
/// `caller` names the entry point in the error message — three of them share
/// this readback, and the buffer says nothing about where it came from.
pub(crate) fn read_tet_connectivity(
    caller: &str,
    element_order: ElementOrderTag,
) -> Result<Vec<u64>, GeometryError> {
    let (elem_type, _) = tet_element_spec(element_order);
    let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;
    verify_tet_readback(caller, &elem_node_tags, element_order)?;
    Ok(elem_node_tags)
}

/// The two numbers a tet's element order fixes: gmsh's element-type code for
/// [`ffi::get_elements_by_type`], and the nodes-per-element stride of the flat
/// buffer that call returns.
///
/// One table because for this readback they are one fact — gmsh numbers its
/// element types by shape AND order (4 = 4-node tet, 11 = 10-node tet), so a
/// site that asked gmsh for 10-node tets while validating against a 4-node
/// stride would mis-slice the connectivity into plausible nonsense.
///
/// The gmsh type code is local to this crate. The stride is NOT: it is a
/// fourth copy of P1→4 / P2→10, alongside `reify_ir::VolumeMesh::nodes_per_element`
/// (the same table over a wider element family) and the `match` in each of
/// [`crate::through_thickness`] and [`crate::fill_metrics`]. Collapsing those
/// belongs next to the `reify_ir` element-order type that owns the fact, not
/// here.
fn tet_element_spec(element_order: ElementOrderTag) -> (i32, usize) {
    match element_order {
        ElementOrderTag::P1 => (4, 4),
        ElementOrderTag::P2 => (11, 10),
    }
}

/// The half of [`read_tet_connectivity`] that the buffer alone decides — split
/// out so both rejections are reachable from a unit test with no live gmsh
/// model behind them.
fn verify_tet_readback(
    caller: &str,
    elem_node_tags: &[u64],
    element_order: ElementOrderTag,
) -> Result<(), GeometryError> {
    let (_, nodes_per_elem) = tet_element_spec(element_order);
    if !elem_node_tags.len().is_multiple_of(nodes_per_elem) {
        return Err(GeometryError::OperationFailed(format!(
            "{caller}: gmsh get_elements_by_type stride mismatch: \
             elem_node_tags.len()={} is not a multiple of {nodes_per_elem} \
             (expected {nodes_per_elem} nodes per {element_order:?} tet)",
            elem_node_tags.len(),
        )));
    }
    if elem_node_tags.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "{caller}: gmshModelMeshGenerate reported success but the model holds \
             no {element_order:?} tetrahedra — returning an empty VolumeMesh would \
             be a silent wrong answer. Known cause: a mesher left unusable by an \
             earlier failed mesh_generate in this process"
        )));
    }
    Ok(())
}

/// The decisions this module makes with no live gmsh model behind them: both
/// [`verify_tet_readback`] rejections over both element orders.
///
/// Neither is reachable from a measured production path — the empty-buffer
/// rejection for the reason [`read_tet_connectivity`] records. So without
/// these, a guard inverted or deleted outright would ship green.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_tet_buffer_is_rejected_and_names_its_caller() {
        let err = verify_tet_readback("some_mesher", &[], ElementOrderTag::P1)
            .expect_err("an empty element buffer is never a real mesh");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("no P1 tetrahedra"),
            "the empty case must say the model holds no tets of the requested order; got: {msg}"
        );
        assert!(
            msg.contains("some_mesher"),
            "three meshers share this check, so the message must name which one; got: {msg}"
        );
    }

    #[test]
    fn a_buffer_that_is_not_a_whole_number_of_tets_is_rejected() {
        let err = verify_tet_readback("some_mesher", &[1, 2, 3], ElementOrderTag::P1)
            .expect_err("3 node tags cannot be a whole number of 4-node tets");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("stride mismatch"),
            "a partial tet must be reported as a stride mismatch, not as an empty \
             readback; got: {msg}"
        );
    }

    #[test]
    fn the_stride_is_read_from_the_element_order() {
        verify_tet_readback("some_mesher", &[1, 2, 3, 4], ElementOrderTag::P1)
            .expect("four node tags are exactly one P1 tet");
        let err = verify_tet_readback("some_mesher", &[1, 2, 3, 4], ElementOrderTag::P2)
            .expect_err("a P2 tet is 10 nodes, so the same four tags are a partial one");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("stride mismatch") && msg.contains("multiple of 10"),
            "the P2 stride must come from the order, not from a P1 constant; got: {msg}"
        );
    }
}
