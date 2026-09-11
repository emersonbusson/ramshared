1. Modify `crates/ramshared-cuda/src/ffi.rs`:
   ```bash
   cat << 'EOF' > patch_ffi.js
   const fs = require('fs');
   let code = fs.readFileSync('crates/ramshared-cuda/src/ffi.rs', 'utf8');

   code = code.replace(
       'pub type FnMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> CuResult;',
       `pub type FnMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> CuResult;
   pub type FnDeviceCanAccessPeer = unsafe extern "C" fn(*mut c_int, CuDevice, CuDevice) -> CuResult;
   pub type FnMemcpyPeer = unsafe extern "C" fn(CuDevicePtr, CuContext, CuDevicePtr, CuContext, usize) -> CuResult;`
   );

   code = code.replace(
       'pub mem_get_info: FnMemGetInfo,',
       `pub mem_get_info: FnMemGetInfo,
       pub device_can_access_peer: FnDeviceCanAccessPeer,
       pub memcpy_peer: FnMemcpyPeer,`
   );

   fs.writeFileSync('crates/ramshared-cuda/src/ffi.rs', code);
   EOF
   node patch_ffi.js
   rm patch_ffi.js
   ```

2. Modify `crates/ramshared-cuda/src/driver.rs`:
   ```bash
   cat << 'EOF' > patch_driver.js
   const fs = require('fs');
   let code = fs.readFileSync('crates/ramshared-cuda/src/driver.rs', 'utf8');

   code = code.replace(
       'use core::ffi::{CStr, c_char, c_void};',
       'use core::ffi::{CStr, c_char, c_int, c_void};'
   );

   code = code.replace(
       'mem_get_info: load_sym(handle, c"cuMemGetInfo_v2")?,',
       `mem_get_info: load_sym(handle, c"cuMemGetInfo_v2")?,
                   device_can_access_peer: load_sym(handle, c"cuDeviceCanAccessPeer")?,
                   memcpy_peer: load_sym(handle, c"cuMemcpyPeer")?,`
   );

   code = code.replace(
       'pub fn device_count(&self) -> Result<i32, CudaError> {',
       `/// Queries if \`dev\` can directly access \`peer_dev\`'s memory.
       pub fn device_can_access_peer(&self, dev: &Device, peer_dev: &Device) -> Result<bool, CudaError> {
           let mut can_access: c_int = 0;
           // SAFETY: can_access points to a valid local memory location.
           let r = unsafe { (self.syms.device_can_access_peer)(&mut can_access, dev.raw, peer_dev.raw) };
           check(&self.syms, r, "cuDeviceCanAccessPeer")?;
           Ok(can_access != 0)
       }

       /// Returns the number of CUDA-capable devices visible to the system.
       pub fn device_count(&self) -> Result<i32, CudaError> {`
   );

   code = code.replace(
       'fn bounds(&self, off: usize, len: usize) -> Result<(), CudaError> {',
       `/// Copies \`len\` bytes directly from another device memory (\`src_mem\`) at \`src_off\`
       /// to this memory at \`dst_off\` (Device->Device, synchronous).
       pub fn memcpy_peer(
           &mut self,
           dst_off: usize,
           src_mem: &DeviceMem<'_, '_>,
           src_off: usize,
           len: usize,
       ) -> Result<(), CudaError> {
           self.bounds(dst_off, len)?;
           src_mem.bounds(src_off, len)?;

           let syms = &self.ctx.cuda.syms;
           // SAFETY: offsets and lengths validated by bounds(); pointers are within allocated regions.
           let r = unsafe {
               (syms.memcpy_peer)(
                   self.ptr + dst_off as u64,
                   self.ctx.raw,
                   src_mem.ptr + src_off as u64,
                   src_mem.ctx.raw,
                   len,
               )
           };
           check(syms, r, "cuMemcpyPeer")
       }

       fn bounds(&self, off: usize, len: usize) -> Result<(), CudaError> {`
   );

   fs.writeFileSync('crates/ramshared-cuda/src/driver.rs', code);
   EOF
   node patch_driver.js
   rm patch_driver.js
   ```

3. Modify `crates/ramshared-cuda/src/lib.rs`:
   ```bash
   cat << 'EOF' > patch_lib.js
   const fs = require('fs');
   let code = fs.readFileSync('crates/ramshared-cuda/src/lib.rs', 'utf8');

   code = code.replace(
       '// Known pattern at three offsets.',
       `// Test P2P if multiple devices exist
           if cuda.device_count().unwrap() >= 2 {
               let dev1 = cuda.device(1).unwrap();
               if cuda.device_can_access_peer(&dev, &dev1).unwrap() {
                   let ctx1 = cuda.create_context(&dev1).unwrap();
                   let mut mem1 = ctx1.alloc(size).unwrap();
                   mem1.zero().unwrap();

                   // mem was allocated on dev 0, mem1 on dev 1.
                   // Test Device -> Device copy
                   mem.write_at(0, b"p2ptest").unwrap();
                   mem1.memcpy_peer(0, &mem, 0, 7).unwrap();
                   let mut out_p2p = vec![0u8; 7];
                   mem1.read_at(0, &mut out_p2p).unwrap();
                   assert_eq!(out_p2p, b"p2ptest", "P2P roundtrip diverged");
               }
           }

           // Known pattern at three offsets.`
   );

   fs.writeFileSync('crates/ramshared-cuda/src/lib.rs', code);
   EOF
   node patch_lib.js
   rm patch_lib.js
   ```

4. Verify changes:
   ```bash
   git diff crates/ramshared-cuda/src/ffi.rs
   git diff crates/ramshared-cuda/src/driver.rs
   git diff crates/ramshared-cuda/src/lib.rs
   ```

5. Run linters:
   ```bash
   cargo clippy --all-targets
   ```

6. Run tests:
   ```bash
   cargo test -p ramshared-cuda
   ```

7. Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.
