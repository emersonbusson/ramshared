# Finding

The instructed task was to audit the dxgkrnl ABI struct alignment and collision against WSL2 headers.
The C struct definition of `d3dkmthandle` in the WSL2 headers is:

```c
struct d3dkmthandle {
	union {
		struct {
			__u32 instance	:  6;
			__u32 index	: 24;
			__u32 unique	: 2;
		};
		__u32 v;
	};
};
```

This struct relies on 32-bit bitfields and a `__u32` inside a union, resulting in exactly a 4-byte (`u32`) struct size. The C tests and kernel headers verify that `d3dkmthandle` is 4 bytes. Thus, `adapter_handle` in `AdapterInfo`, `adapter` in `QueryVideoMemoryInfo`, and `adapter_handle` in `CloseAdapter` are exactly 4-byte integers in C. The current Rust representation in `crates/ramshared-dxg/src/lib.rs` simply using `u32` is completely accurate. `QueryVideoMemoryInfo` is 56 bytes in C and 56 bytes in Rust, and the ioctl values match (`0xc038470a`).

No safe code change is possible or needed because the current Rust code is structurally correct. It correctly aligns to the Windows and WSL2 ABI (d3dkmthk.h) for `D3DKMT_HANDLE`. Modifying the sizes to pad `D3DKMT_HANDLE` to 8 bytes would incorrectly bloat the structs and invalidate the ioctl numbers (e.g., from `0xc038470a` to `0xc040470a`), which breaks the dxgkrnl API.

Since the premise of an ABI mismatch is a false positive and safe code changes cannot be made, this task is documented as a `FINDING_ONLY`.
