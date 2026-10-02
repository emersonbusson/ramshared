# ramshared-ipc

Shared host-guest IPC protocol for the RamShared vsock control plane.

## Scope & Responsibility

`ramshared-ipc` implements the binary framed protocol and vsock transport abstraction for host-guest communication:
- **Binary Framing:** 24-byte `VsockFrameHeader` with magic `0x52414D53` ("RAMS"), versioned headers, `correlation_id` for request-response matching, and bounded payloads (1MB max).
- **Message Types:** 21 typed messages covering handshake, heartbeat, lease management, origin manifest, safe-mode gate, guardian health, VHDX lifecycle, telemetry, and shutdown.
- **HMAC Authentication:** HMAC-SHA256 handshake verification (manual implementation using `sha2`) for cryptographic origin authority minting.
- **vsock Transport:** AF_VSOCK (guest) / AF_HYPERV (host) stream socket abstraction with bounded connect and read timeouts.
- **Version Negotiation:** Protocol version 3 with backward-compatible version 2 support.

## Workspace Dependencies

- External crates: `serde` + `serde_json` for control message serialization, `sha2` for HMAC-SHA256.

## Safety Invariants

- **Bounded Payloads:** All frames capped at `MAX_PAYLOAD_LEN` (1MB); JSON control messages capped at `MAX_CONTROL_PAYLOAD` (4KB); manifest payloads capped at `MAX_MANIFEST_PAYLOAD` (64KB). Exceeding caps is rejected at deserialization.
- **Typed Errors:** Uses `FrameError` and `VsockError` with zero unchecked panics.
- **Constant-Time HMAC Comparison:** `verify_hmac` uses XOR-accumulate comparison to prevent timing side-channels.

## Testing

```bash
cargo test -p ramshared-ipc
```
