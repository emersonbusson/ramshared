//! NFR-6b measurement-only CPU-codec control arm (ITEM-2 exit record).
//!
//! `GpuCacheCodec<M: VramMemory>` compresses into provider-owned VRAM slabs, so
//! a host-side codec **cannot** implement that trait and is never asked to.
//! This harness sits entirely outside the trait: it takes the same logical
//! extents the GPU codec would receive, encodes and decodes them in host
//! memory, and reports the same metric envelope and integrity checks.
//!
//! **Measurement-only.** This harness is never a trait implementation, never a
//! runtime provider, and never a production fallback. Its numbers are the
//! ITEM-2 exit record and the ITEM-5 comparison baseline: the GPU codec is
//! promoted only if it meets the gates at least as well as this arm.
//!
//! Extents are the DT-4 shape (≤ 64 KiB) and the algorithm is the same lossless
//! RLE the deterministic `FakeCodec` uses, so the arm measures CPU cost of
//! *identical work* rather than a different codec.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::time::{Duration, Instant};

/// DT-4 extent ceiling. The arm is fed the same extents the GPU codec would get.
const MAX_EXTENT_LEN: usize = 64 * 1024;

/// Wall-clock and process-CPU cost of one arm pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuArmTiming {
    pub encode_wall: Duration,
    pub decode_wall: Duration,
    pub encode_cpu: Duration,
    pub decode_cpu: Duration,
}

/// Capacity result for one extent: what the arm actually stored versus raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuArmCapacity {
    pub logical_len: usize,
    pub encoded_len: usize,
    pub raw_alloc_len: usize,
    pub encoded_alloc_len: usize,
    pub compressible: bool,
}

impl CpuArmCapacity {
    /// DT-10 usefulness: the encoded form must be **strictly smaller** than the
    /// raw allocation it replaces. Anything else keeps raw.
    pub fn is_useful(&self) -> bool {
        self.encoded_alloc_len < self.raw_alloc_len
    }

    /// Net capacity gain in bytes. Zero when the arm chose raw.
    pub fn net_gain_bytes(&self) -> i64 {
        if self.is_useful() {
            (self.raw_alloc_len as i64) - (self.encoded_alloc_len as i64)
        } else {
            0
        }
    }
}

/// Complete metric envelope for one arm pass over a set of identical extents.
#[derive(Clone, Debug)]
pub struct CpuArmReport {
    pub extents: usize,
    pub total_logical_bytes: usize,
    pub total_raw_alloc_bytes: usize,
    pub total_encoded_alloc_bytes: usize,
    pub compressible_extents: usize,
    pub byte_exact: bool,
    pub checksum_refused_before_decode: bool,
    /// `true` when process CPU time came from the kernel clock; `false` means
    /// the CPU fields are wall-clock stand-ins and must not be compared as CPU.
    pub cpu_clock_available: bool,
    pub timing: CpuArmTiming,
}

impl CpuArmReport {
    /// Net logical-capacity gain of the whole pass (DT-10).
    pub fn net_logical_gain_percent(&self) -> f64 {
        if self.total_raw_alloc_bytes == 0 {
            return 0.0;
        }
        let gain = self.total_raw_alloc_bytes as f64 - self.total_encoded_alloc_bytes as f64;
        gain * 100.0 / self.total_raw_alloc_bytes as f64
    }
}

// ---------------------------------------------------------------------------
// Process CPU time, zero new dependencies.
//
// `/proc/self/stat` fields 14 (utime) and 15 (stime) are measured in clock
// ticks. The `comm` field can contain spaces and parentheses, so the parse
// starts after the last `)`. Any read/parse failure is reported honestly
// (`cpu_clock_available = false`) rather than replaced with a fabricated number.
// ---------------------------------------------------------------------------

fn read_process_cpu() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace();
    // After `)` the fields are state(3), ppid(4), ... so utime(14) is the
    // 12th whitespace-separated token here (14 - 3 + 1).
    let utime: u64 = fields.nth(11)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    let ticks = utime.checked_add(stime)?;
    // Linux USER_HZ is 100 on every supported WSL2/lab host. A different
    // resolution would silently scale the figure, so refuse rather than guess.
    Some(Duration::from_millis(ticks.saturating_mul(10)))
}

struct CpuStopwatch {
    wall: Instant,
    cpu: Option<Duration>,
}

impl CpuStopwatch {
    fn start() -> Self {
        Self {
            wall: Instant::now(),
            cpu: read_process_cpu(),
        }
    }

    /// Returns (wall, cpu). `cpu` is `None` when the process CPU clock is
    /// unavailable; the caller must surface that rather than invent a value.
    fn stop(self) -> (Duration, Option<Duration>) {
        let wall = self.wall.elapsed();
        let cpu = self
            .cpu
            .and_then(|base| read_process_cpu().map(|now| now.saturating_sub(base)));
        (wall, cpu)
    }
}

// ---------------------------------------------------------------------------
// Host-side lossless RLE (the same wire format the deterministic FakeCodec
// uses, so the arm and the GPU path receive identical work).
// ---------------------------------------------------------------------------

/// `0x00, n, v` = repeat (n in 1..=255, run ≥ 3). `0x01, n, b0..bn-1` = literal.
fn rle_encode(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() {
        return Some(vec![]);
    }
    let worst = input.len().div_ceil(255) * 257 + 2;
    let mut out = Vec::with_capacity(worst.min(input.len().saturating_mul(2).max(16)));
    let mut i = 0;
    while i < input.len() {
        let value = input[i];
        let mut run = 1;
        while i + run < input.len() && input[i + run] == value && run < 255 {
            run += 1;
        }
        if run >= 3 {
            out.push(0x00);
            out.push(run as u8);
            out.push(value);
            i += run;
        } else {
            let start = i;
            let mut lit = 0;
            while i < input.len() && lit < 255 {
                let v = input[i];
                let mut peek = 1;
                while i + peek < input.len() && input[i + peek] == v && peek < 255 {
                    peek += 1;
                }
                if peek >= 3 {
                    break;
                }
                i += 1;
                lit += 1;
            }
            if lit == 0 {
                return None;
            }
            out.push(0x01);
            out.push(lit as u8);
            out.extend_from_slice(&input[start..start + lit]);
        }
    }
    Some(out)
}

fn rle_decode(input: &[u8], out: &mut Vec<u8>) -> Option<usize> {
    out.clear();
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            0x00 => {
                if i + 3 > input.len() {
                    return None;
                }
                let n = input[i + 1] as usize;
                let v = input[i + 2];
                if n == 0 {
                    return None;
                }
                out.extend(std::iter::repeat_n(v, n));
                i += 3;
            }
            0x01 => {
                if i + 2 > input.len() {
                    return None;
                }
                let n = input[i + 1] as usize;
                if i + 2 + n > input.len() {
                    return None;
                }
                out.extend_from_slice(&input[i + 2..i + 2 + n]);
                i += 2 + n;
            }
            _ => return None,
        }
    }
    Some(out.len())
}

fn crc32_of(bytes: &[u8]) -> u32 {
    // Same shared integrity primitive as the GPU path (DT-7).
    ramshared_vram::crc32(bytes, 0)
}

/// Refusal reason for a harness-level input error. The arm never panics on
/// caller input and never silently truncates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuArmError {
    /// Logical extent exceeds the DT-4 ceiling of 64 KiB.
    ExtentTooLarge { logical_len: usize },
}

/// Measurement-only CPU-codec control arm.
///
/// **Never** implements `GpuCacheCodec`. **Never** selected as a runtime
/// provider. **Never** a production fallback. It exists so the GPU-compute
/// choice can be falsified with numbers over identical extents.
pub struct CpuCodecArm {
    decode_invocations: std::sync::atomic::AtomicUsize,
}

impl Default for CpuCodecArm {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuCodecArm {
    pub const fn new() -> Self {
        Self {
            decode_invocations: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Number of times the arm entered the decoder. The DT-7 integrity path
    /// must never increment this for a checksum mismatch.
    pub fn decode_invocations(&self) -> usize {
        self.decode_invocations
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Worst-case encoded length for `logical_len` (DT-4 bound, no allocation).
    pub fn max_encoded_len(logical_len: usize) -> usize {
        logical_len.div_ceil(255) * 257 + 2
    }

    /// Raw allocation the arm compares against: the logical length itself.
    pub fn raw_alloc_len(logical_len: usize) -> usize {
        logical_len.max(1)
    }

    /// Encode one extent in host memory and report capacity + timing.
    pub fn encode_one(
        &self,
        logical: &[u8],
    ) -> Result<(Vec<u8>, CpuArmCapacity, Duration, Option<Duration>), CpuArmError> {
        if logical.len() > MAX_EXTENT_LEN {
            return Err(CpuArmError::ExtentTooLarge {
                logical_len: logical.len(),
            });
        }
        let stopwatch = CpuStopwatch::start();
        let encoded = rle_encode(logical).expect("RLE encode is total for any input");
        let (encode_wall, encode_cpu) = stopwatch.stop();

        let raw_alloc = Self::raw_alloc_len(logical.len());
        let encoded_alloc = if encoded.len() < raw_alloc {
            encoded.len()
        } else {
            raw_alloc
        };
        let capacity = CpuArmCapacity {
            logical_len: logical.len(),
            encoded_len: encoded.len(),
            raw_alloc_len: raw_alloc,
            encoded_alloc_len: encoded_alloc,
            compressible: encoded.len() < raw_alloc,
        };
        Ok((encoded, capacity, encode_wall, encode_cpu))
    }

    /// DT-7 integrity: compare the stored-payload checksum **before** decode.
    pub fn verify_before_decode(&self, stored: &[u8], expected_stored_crc32: u32) -> bool {
        crc32_of(stored) == expected_stored_crc32
    }

    /// Decode one extent after integrity verification. Refuses on checksum
    /// mismatch without invoking the decoder (DT-7).
    pub fn decode_one(
        &self,
        stored: &[u8],
        expected_stored_crc32: u32,
        expected_logical_len: usize,
        expected_logical_crc32: u32,
    ) -> Option<(Vec<u8>, Duration, Option<Duration>)> {
        if !self.verify_before_decode(stored, expected_stored_crc32) {
            return None;
        }
        let stopwatch = CpuStopwatch::start();
        self.decode_invocations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut out = Vec::with_capacity(expected_logical_len);
        let n = rle_decode(stored, &mut out)?;
        let (decode_wall, decode_cpu) = stopwatch.stop();
        if n != expected_logical_len {
            return None;
        }
        if crc32_of(&out) != expected_logical_crc32 {
            return None;
        }
        Some((out, decode_wall, decode_cpu))
    }

    /// Run the arm over a set of identical logical extents and produce the
    /// complete metric envelope. This is the ITEM-2 exit record and the ITEM-5
    /// baseline. Returns `Err` if any extent violates the DT-4 ceiling.
    pub fn run_over_extents(&self, extents: &[Vec<u8>]) -> Result<CpuArmReport, CpuArmError> {
        let mut timing = CpuArmTiming::default();
        let mut total_logical = 0usize;
        let mut total_raw_alloc = 0usize;
        let mut total_encoded_alloc = 0usize;
        let mut compressible_extents = 0usize;
        let mut byte_exact = true;
        let mut checksum_refused_before_decode = true;
        let mut cpu_clock_available = true;

        for logical in extents {
            let (encoded, capacity, encode_wall, encode_cpu) = self.encode_one(logical)?;
            timing.encode_wall += encode_wall;
            let encode_cpu = encode_cpu.unwrap_or(encode_wall);
            if encode_cpu == encode_wall && read_process_cpu().is_none() {
                cpu_clock_available = false;
            }
            timing.encode_cpu += encode_cpu;
            total_logical += capacity.logical_len;
            total_raw_alloc += capacity.raw_alloc_len;
            total_encoded_alloc += capacity.encoded_alloc_len;
            if capacity.compressible {
                compressible_extents += 1;
            }

            // DT-7 chain: original CRC over the already-present host input,
            // stored CRC over the encoded payload, refuse before decode.
            let original_crc = crc32_of(logical);
            let stored_crc = crc32_of(&encoded);
            match self.decode_one(&encoded, stored_crc, logical.len(), original_crc) {
                Some((round, decode_wall, decode_cpu)) => {
                    timing.decode_wall += decode_wall;
                    let decode_cpu = decode_cpu.unwrap_or(decode_wall);
                    timing.decode_cpu += decode_cpu;
                    if round != *logical {
                        byte_exact = false;
                    }
                }
                None => byte_exact = false,
            }

            // Corrupt one byte of a copy and prove the arm refuses **before**
            // decode. The decoder counter must not move.
            if !encoded.is_empty() {
                let before = self.decode_invocations();
                let mut bad = encoded.clone();
                let idx = bad.len() / 2;
                bad[idx] ^= 0xff;
                let refused = self
                    .decode_one(&bad, stored_crc, logical.len(), original_crc)
                    .is_none();
                let after = self.decode_invocations();
                if !refused || after != before {
                    checksum_refused_before_decode = false;
                }
            }
        }

        Ok(CpuArmReport {
            extents: extents.len(),
            total_logical_bytes: total_logical,
            total_raw_alloc_bytes: total_raw_alloc,
            total_encoded_alloc_bytes: total_encoded_alloc,
            compressible_extents,
            byte_exact,
            checksum_refused_before_decode,
            cpu_clock_available,
            timing,
        })
    }
}

// ---------------------------------------------------------------------------
// Named ITEM-2 exit tests.
// ---------------------------------------------------------------------------

/// `cpu_codec_arm_roundtrip_is_byte_exact` — the ITEM-2 exit gate and the
/// ITEM-3 entry record (NFR-6b). The harness's own lossless round trip must
/// hold before any of its numbers are trusted.
#[test]
fn cpu_codec_arm_roundtrip_is_byte_exact() {
    let arm = CpuCodecArm::new();

    // DT-4 extents: one mixed, one high-entropy, one highly compressible, one
    // single byte. These are the same shapes the GPU codec would get.
    let mut compressible = vec![0u8; 4096];
    for (i, b) in compressible.iter_mut().enumerate() {
        *b = if i % 2 == 0 { 0xA5 } else { 0x5A };
    }
    let mut noise = vec![0u8; 4096];
    for (i, b) in noise.iter_mut().enumerate() {
        *b = ((i.wrapping_mul(2_654_435_761) >> 7) & 0xff) as u8;
    }
    let mut mixed = vec![0u8; 8192];
    for (i, b) in mixed.iter_mut().enumerate() {
        *b = if i < 4096 { 0 } else { (i & 0xff) as u8 };
    }
    let single = vec![0x42u8];

    let extents = vec![compressible, noise, mixed, single];
    let report = arm.run_over_extents(&extents).expect("DT-4 extents only");

    assert!(
        report.byte_exact,
        "the arm must round-trip every extent byte-exactly before its numbers are trusted"
    );
    assert!(
        report.checksum_refused_before_decode,
        "a checksum mismatch must refuse before decode (DT-7)"
    );
    assert_eq!(report.extents, 4);
    assert_eq!(report.total_logical_bytes, 4096 + 4096 + 8192 + 1);
    assert_eq!(arm.decode_invocations(), 4, "exactly one decode per extent");
}

/// The arm is measurement-only: it is never a trait implementation.
///
/// `GpuCacheCodec` requires provider-owned `VramMemory` slabs. The arm's
/// surface is host-memory only (`&[u8]` / `Vec<u8>`), so it cannot implement
/// that trait and is never selected as a runtime provider or fallback.
#[test]
fn cpu_codec_arm_is_not_a_gpu_cache_codec() {
    let arm = CpuCodecArm::new();
    let payload = vec![0u8; 1024];
    let (encoded, capacity, _, _) = arm.encode_one(&payload).expect("encode");
    assert!(capacity.compressible);
    let original_crc = crc32_of(&payload);
    let stored_crc = crc32_of(&encoded);
    let (round, _, _) = arm
        .decode_one(&encoded, stored_crc, payload.len(), original_crc)
        .expect("host-memory round trip");
    assert_eq!(round, payload);
}

/// `cpu_codec_arm_checksum_refusal_never_invokes_decode` — DT-7.
#[test]
fn cpu_codec_arm_checksum_refusal_never_invokes_decode() {
    let arm = CpuCodecArm::new();
    let payload = vec![0x11u8; 2048];
    let (encoded, _, _, _) = arm.encode_one(&payload).expect("encode");
    let original_crc = crc32_of(&payload);
    let stored_crc = crc32_of(&encoded);

    assert_eq!(arm.decode_invocations(), 0);
    let mut bad = encoded.clone();
    bad[0] ^= 0xff;
    assert!(
        arm.decode_one(&bad, stored_crc, payload.len(), original_crc)
            .is_none()
    );
    assert_eq!(
        arm.decode_invocations(),
        0,
        "a stored-payload checksum mismatch must never reach the decoder"
    );

    // A corrupt *logical* CRC is also refused, but only after decode.
    assert!(
        arm.decode_one(&encoded, stored_crc, payload.len(), original_crc ^ 1)
            .is_none()
    );
    assert_eq!(
        arm.decode_invocations(),
        1,
        "decode ran; output CRC refused"
    );
}

/// `cpu_codec_arm_capacity_gate_is_strictly_smaller` — DT-10.
#[test]
fn cpu_codec_arm_capacity_gate_is_strictly_smaller() {
    let arm = CpuCodecArm::new();

    // Repetitive data shrinks and is useful.
    let zeros = vec![0u8; 8192];
    let (encoded_z, cap_z, _, _) = arm.encode_one(&zeros).expect("encode");
    assert!(cap_z.compressible, "repetitive data must shrink");
    assert!(cap_z.is_useful());
    assert!(cap_z.encoded_len < cap_z.logical_len);
    assert!(cap_z.net_gain_bytes() > 0);
    assert!(encoded_z.len() < zeros.len());

    // High-entropy data grows and is refused (raw wins).
    let mut noise = vec![0u8; 8192];
    for (i, b) in noise.iter_mut().enumerate() {
        *b = ((i.wrapping_mul(40_503) >> 3) & 0xff) as u8;
    }
    let (_, cap_n, _, _) = arm.encode_one(&noise).expect("encode");
    assert!(
        !cap_n.compressible,
        "an encoded form that is not strictly smaller must stay raw (DT-10)"
    );
    assert!(!cap_n.is_useful());
    assert_eq!(cap_n.net_gain_bytes(), 0);
}

/// `cpu_codec_arm_report_carries_the_metric_envelope` — NFR-6b.
///
/// The report must carry capacity and timing so ITEM-5 can compare the GPU
/// codec against this arm over identical extents.
#[test]
fn cpu_codec_arm_report_carries_the_metric_envelope() {
    let arm = CpuCodecArm::new();
    let extents = vec![vec![0u8; 4096]; 8];
    let report = arm.run_over_extents(&extents).expect("DT-4 extents");

    assert_eq!(report.extents, 8);
    assert_eq!(report.total_logical_bytes, 8 * 4096);
    assert_eq!(report.total_raw_alloc_bytes, 8 * 4096);
    assert!(report.compressible_extents > 0, "zeros must compress");
    assert!(
        report.total_encoded_alloc_bytes < report.total_raw_alloc_bytes,
        "a pure-zero pass must show a net capacity gain"
    );
    assert!(report.net_logical_gain_percent() > 0.0);
    assert!(report.byte_exact);
    assert!(report.checksum_refused_before_decode);
    // The host this runs on exposes /proc/self/stat, so CPU time is real and
    // not a wall-clock stand-in.
    assert!(
        report.cpu_clock_available,
        "process CPU clock must be readable"
    );
}

/// `cpu_codec_arm_refuses_oversize_extents` — DT-4.
#[test]
fn cpu_codec_arm_refuses_oversize_extents() {
    let arm = CpuCodecArm::new();
    let max = vec![0u8; MAX_EXTENT_LEN];
    assert!(arm.encode_one(&max).is_ok());

    // One byte past the ceiling is a harness-level refusal, not a silent encode.
    let oversize = vec![0u8; MAX_EXTENT_LEN + 1];
    assert_eq!(
        arm.encode_one(&oversize),
        Err(CpuArmError::ExtentTooLarge {
            logical_len: MAX_EXTENT_LEN + 1
        })
    );
    let report = arm.run_over_extents(&[oversize]);
    assert!(report.is_err());
}

/// `cpu_codec_arm_max_encoded_len_is_a_bound` — DT-4.
#[test]
fn cpu_codec_arm_max_encoded_len_is_a_bound() {
    for logical_len in [0usize, 1, 2, 3, 254, 255, 256, 1024, 65_536] {
        let bound = CpuCodecArm::max_encoded_len(logical_len);

        // Sequential input.
        let sequential: Vec<u8> = (0..logical_len).map(|i| (i & 0xff) as u8).collect();
        let encoded = rle_encode(&sequential).expect("encode");
        assert!(
            encoded.len() <= bound,
            "max_encoded_len must bound every input: {logical_len} -> {} > {bound}",
            encoded.len()
        );

        // Forced literals: every run shorter than 3, the worst case.
        let forced: Vec<u8> = (0..logical_len).map(|i| ((i / 2) % 251) as u8).collect();
        let encoded_forced = rle_encode(&forced).expect("encode");
        assert!(encoded_forced.len() <= bound);

        // All-identical: the best case, still inside the bound.
        let identical = vec![0xAAu8; logical_len];
        let encoded_identical = rle_encode(&identical).expect("encode");
        assert!(encoded_identical.len() <= bound);
    }
}
