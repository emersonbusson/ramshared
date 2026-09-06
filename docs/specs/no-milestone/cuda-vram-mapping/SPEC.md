# cuda-vram-mapping

Spec.

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/ffi.rs,crates/ramshared-cuda/src/vram_impl.rs --min 80 --report-json tmp/cuda-vram-mapping.json
```
