# step-assembly-import — substrate evidence (G3)

Produced 2026-09-27 against system OCCT 7.8 (`/usr/include/opencascade`,
`/usr/lib/x86_64-linux-gnu/libTK*.so.7.8`), outside the Rust workspace.
These files prove the kernel-side premises of `../step-assembly-import.md` §3.1.
They are evidence, not build inputs: nothing compiles or reads them.

| File | What it proves |
|---|---|
| `writer.cpp` | Writes `test_assembly.step` through XDE: root `Container`; 4 occurrences of one shared product `CornerCasting` (instance names `CornerCasting-1..4`); sub-assembly `SideWall` holding `Panel` + `Rail`; multi-body product `Weldment` (2 disjoint solids). Millimetre coordinates. |
| `reader.cpp` | Reads it back with `STEPCAFControl_Reader` (`SetNameMode(true)`) and prints the product tree, instance names, locations, shared-product tallies, solid counts. `--static-unit=M` sets `Interface_Static::SetCVal("xstep.cascade.unit","M")` before `ReadFile`. |
| `plain_reader.cpp` | The plain `STEPControl_Reader` → `TransferRoots` → `OneShape` route (the original #4289 plan): one anonymous compound of 8 solids, no names. |
| `test_assembly.step` | The multi-product fixture; task α copies it into the kernel's test fixtures. |
| `reader_output_default_units.txt`, `reader_output_metres.txt`, `plain_reader_out.txt` | Captured output of the runs above. |

Minimal link set for the reader:

```
g++ -std=c++17 -I/usr/include/opencascade reader.cpp -o reader \
  -L/usr/lib/x86_64-linux-gnu -lTKXCAF -lTKLCAF -lTKDESTEP -lTKXSBase \
  -lTKBRep -lTKTopAlgo -lTKMath -lTKernel -Wl,-rpath,/usr/lib/x86_64-linux-gnu
```

Relative to `crates/reify-kernel-occt/build.rs`, only `TKXCAF` and `TKLCAF` are new.
`STEPControl_Reader::SetSystemLengthUnit` had no observable effect in any value tried;
the global `xstep.cascade.unit` static is the call that converts to metres.
