# gmsh 4.15.2: PostView background field + multi-threaded meshing

**Task #7706 | 2026-10-01 | supersedes the threading rationale of task #7447**

`crates/reify-kernel-gmsh/src/refine_volume.rs` installs a `PostView` background mesh-size
field, then meshes with the caller's resolved `General.NumThreads`. That is safe on libgmsh
4.15.2 only because `BackgroundFieldGuard::install` probes the view once
(`ffi::view_probe`) before meshing. This note holds the evidence. Code comments point here
rather than restating it.

---

## a. Symptom (task #7447)

With a `PostView` background field installed, `gmshModelMeshGenerate(3)` blocks forever: no
error, no timeout. CPU time stays frozen, RSS stays flat, and every thread sits in
`futex_do_wait`. It hangs on a valid unit cube too, so it is not confined to the mesher's
failure path.

#7447 measured `tests/mesher_poison_recovery.rs` at `c1cabbb73c`, varying only
`General.NumThreads`:

| `General.NumThreads` | outcome                  |
|----------------------|--------------------------|
| 1                    | 5 passed, 17.58 s        |
| 2                    | 5 passed, 24.37 s        |
| 8                    | HANG, SIGKILLed at 100 s |
| 32 (= nproc)         | HANG, SIGKILLed at 100 s |

`Mesh.MaxNumThreads3D = 1` with `General.NumThreads = 32` still hangs.

The same sweep on a clean process (`tests/refine_volume_tests.rs`, unit cube, uniform field):
8 passes in ~4 s; 16, 24 and 32 hang.

#7447 responded by pinning `General.NumThreads = 1` unconditionally in
`refine_volume_with_size_field`. #7706 removed that pin.

## b. Root cause

gmsh 4.15.2, `src/post/PViewData.cpp`, `PViewData::searchScalar` (and likewise
`searchVector` and `searchTensor`) builds the view's lookup octree lazily:

```cpp
if(!_octree) {
#pragma omp barrier
#pragma omp single
  { _octree = new OctreePost(this); }
}
```

That barrier is ORPHANED: it binds to whatever OpenMP team is running when it is first
reached.

1D and 2D meshing evaluate the background field from inside worksharing loops over curves
and surfaces: `src/mesh/Generator.cpp:431` (1D) and `:590` (2D), both
`#pragma omp parallel for schedule(dynamic) num_threads(nthreads)`. The field is evaluated
through `PostViewField::operator()` → `PViewData::searchScalarClosest` →
`PViewData::searchScalar`.

The mechanism: when `General.NumThreads` exceeds the number of curves, the threads that got
no iteration go straight to the loop's implicit end barrier. The threads that did get one
reach the orphaned octree barrier. The two barriers never line up, so the whole team
deadlocks.

This also explains why `Mesh.MaxNumThreads3D = 1` cannot help: the first field evaluation
happens in 1D, long before the 3D stage.

## c. Measured threshold = classified curve count

Measured with the Python reproducer in section f: no warm-up, libgmsh 4.15.2, 32-core host at
loadavg ~100, each run under `timeout 25`–`30`.

- **Unit cube**: 14 curves and 8 surfaces after `classify_surfaces(PI/12, 1, 1, PI/12, 0)`.
  - 2, 8, 12, 13 and 14 threads: OK.
  - 15, 16 and 32 threads: HANG.
- **Open triangle**: 3 curves.
  - 2 and 3 threads: fail promptly with `HXT 3D mesh failed`, the error
    `mesher_poison_recovery` expects.
  - 4 threads (2 of 2 runs), 8, 16 and 32: HANG.

This supersedes #7447's hypothesis that `mesher_poison_recovery`'s lower threshold came from
gmsh having been through a `finalize`/`initialize` recovery cycle. That binary hangs at 8
because its poisoning input, the open triangle, has only 3 curves. The "fixture-dependent
threshold" is each fixture's curve count.

## d. Fix and measurement

Probe the view once, on the calling thread, after `gmshViewAddListData` and before
`gmshModelMeshGenerate`. `gmsh::view::probe` reaches `searchScalarClosest` →
`searchScalar`, so ANY point builds the octree, whether the probe hits or misses.

Outside a parallel region, an orphaned `barrier` / `single` binds to a team of one and is a
no-op. 4.15.2 deletes `_octree` only in `~PViewData`, so the octree built here is the one the
mesher threads later find. The closest-node kd-tree in `findClosestNode` is guarded by
`omp critical` (a mutex, not a barrier), so it needs no warm-up.

**Reproducer, warm.** 0 hangs in 40 runs: cube and triangle at 2, 3, 8, 16 and 32 threads,
1 run plus 4 repeats each. The cube meshed every time. The triangle failed promptly with
`HXT 3D mesh failed`.

**Rust path, task #7706, same host.** With the warm-up in place and the pin lifted:

- `refine_volume_tests`: 8 passed in 4 of 4 runs, binary wall 12.06 / 23.29 / 13.34 /
  12.51 s.
- `mesher_poison_recovery`: 5 passed in 4 of 4 runs, binary wall 46.02 / 52.27 / 45.14 /
  33.31 s.

Both binaries' refine calls ask for a literal `threads: Some(32)`.

**Mutation, not committed.** I deleted the `ffi::view_probe(...)` line from
`BackgroundFieldGuard::install`, rebuilt, and ran:

- `timeout 180 cargo test -p reify-kernel-gmsh --test refine_volume_tests refine_returns_a_mesh_when_the_caller_asks_for_many_threads`
  → **exit 124**, elapsed 180.09 s.
- `timeout 180 cargo test -p reify-kernel-gmsh --test mesher_poison_recovery a_failed_sibling_mesher_leaves_mesh_to_volume_usable`
  → **exit 124**, elapsed 180.05 s.

After I restored the line, both binaries were green again (22.12 s and 33.90 s). So the
warm-up is what keeps those guards from hanging; nothing else in the change does.

Production output is unchanged. Both production callers
(`reify-eval/src/compute_targets/elastic_static.rs`) pass `deterministic: true`, which
resolves to one thread.

## e. Upstream status (observed 2026-10-01)

- No gmsh release after 4.15.2 exists on PyPI or conda-forge.
- The gmsh-git source snapshot on gmsh.info (Last-Modified 2026-09-29, CMake version 5.0.0)
  replaced the barrier/single with `PViewData::_getOctree()` under
  `#pragma omp critical(PViewDataOctree)`.

On such a libgmsh the probe is redundant but harmless, so no version gate is needed.

## f. Reproducer

Use it to re-measure after a libgmsh bump. Take `gmsh.py` from the matching source tarball's
`api/` dir, and put a `lib/libgmsh.so.4.15` symlink beside it. Run:

```sh
LD_LIBRARY_PATH=/opt/reify-deps/lib timeout 30 python3 repro.py <threads> cube|tri cold|warm
```

`cold` reproduces the hang. `warm` applies the fix.

```python
import math, sys
import gmsh
nthreads = int(sys.argv[1]); fixture = sys.argv[2]; warm = sys.argv[3] == "warm"
V = [(0,0,0),(1,0,0),(1,1,0),(0,1,0),(0,0,1),(1,0,1),(1,1,1),(0,1,1)]
if fixture == "cube":
    verts = V
    tris = [0,2,1,0,3,2, 4,5,6,4,6,7, 0,1,5,0,5,4, 3,7,6,3,6,2, 0,4,7,0,7,3, 1,2,6,1,6,5]
else:
    verts = [(0,0,0),(1,0,0),(0,1,0)]; tris = [0,1,2]
TETS = [0,1,2,6, 0,1,5,6, 0,3,2,6, 0,3,7,6, 0,4,5,6, 0,4,7,6]
gmsh.initialize()
gmsh.option.setNumber("General.Terminal", 0)
gmsh.option.setNumber("General.NumThreads", nthreads)
gmsh.option.setNumber("Mesh.ElementOrder", 1)
gmsh.option.setNumber("Mesh.Algorithm3D", 10)
gmsh.model.add("repro")
s = gmsh.model.addDiscreteEntity(2)
gmsh.model.mesh.addNodes(2, s, list(range(1, len(verts)+1)), [c for v in verts for c in v])
gmsh.model.mesh.addElementsByType(s, 2, list(range(1, len(tris)//3+1)), [i+1 for i in tris])
gmsh.model.mesh.classifySurfaces(math.pi/12, True, True, math.pi/12, False)
gmsh.model.mesh.createGeometry()
surfs = [t for _, t in gmsh.model.getEntities(2)]
gmsh.model.geo.addVolume([gmsh.model.geo.addSurfaceLoop(surfs)])
gmsh.model.geo.synchronize()
data = []
for k in range(6):
    tet = TETS[4*k:4*k+4]
    for axis in range(3): data += [V[i][axis] for i in tet]
    data += [0.5]*4
view = gmsh.view.add("bgm"); gmsh.view.addListData(view, "SS", 6, data)
f = gmsh.model.mesh.field.add("PostView")
gmsh.model.mesh.field.setNumber(f, "ViewTag", view)
gmsh.model.mesh.field.setAsBackgroundMesh(f)
for k, v in [("Mesh.MeshSizeFromPoints",0),("Mesh.MeshSizeFromCurvature",0),("Mesh.MeshSizeExtendFromBoundary",0),("Mesh.MeshSizeMin",0),("Mesh.MeshSizeMax",0.5)]:
    gmsh.option.setNumber(k, v)
if warm: gmsh.view.probe(view, 0.5, 0.5, 0.5)
try:
    gmsh.model.mesh.generate(3)
    print("OK tets=", sum(len(t) for t in gmsh.model.mesh.getElements(3)[1]), flush=True)
except Exception as e:
    print("ERR", e, flush=True)
gmsh.finalize()
```
