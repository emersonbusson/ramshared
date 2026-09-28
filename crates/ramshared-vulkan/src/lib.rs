//! `ramshared-vulkan` — Vulkan backend of `VramProvider` (RF-G2).
//!
//! Second implementation of the `ramshared_vram::VramProvider` trait (the first one, CUDA, remains intact),
//! unlocking "any GPU" support + a native Linux host where ublk+VRAM and eviction-under-load run e2e.
//!
//! **Complete IMPL (RF-V1..V3):** `open` initializes the loader, instance, physical device, logical device,
//! transfer queue, and staging buffer (`HOST_VISIBLE|HOST_COHERENT`). `impl VramProvider` covers `alloc`
//! (`DEVICE_LOCAL`) and `mem_info`. `impl VramMemory` covers `read_at`/`write_at` (staging +
//! `vkCmdCopyBuffer` + `VkFence`) and `zero` (`vkCmdFillBuffer`). According to
//! `docs/vulkan-backend/SPEC.md` (DT-1..DT-10).
//!
//! Validated via software rendering (lavapipe/llvmpipe) without a GPU — all unsafe blocks (FFI `ash`) are isolated here
//! with `// SAFETY:` for each block; the trait boundary is safe. `mem_info` uses `VK_EXT_memory_budget`
//! when present; otherwise, it falls back to DT-10 (largest `DEVICE_LOCAL` heap − sum allocated).

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use ash::vk;
use ramshared_vram::{
    GpuAdapterIdentity, GpuBudgetSnapshot, GpuBudgetSource, VramError, VramMemory, VramProvider,
    format_luid,
};

/// Single staging buffer per provider (no alloc on hot path, DT-8): 1 MiB. Larger I/O is sliced.
const STAGING_BYTES: u64 = 1 << 20;

fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}

/// Selects a transfer queue family (prefers explicit `TRANSFER`; falls back to `GRAPHICS`/`COMPUTE`, which imply transfer per spec). Returns the family index.
fn pick_transfer_family(instance: &ash::Instance, phys: vk::PhysicalDevice) -> Option<u32> {
    // SAFETY: `phys` was enumerated from `instance`; the query only reads properties.
    let fams = unsafe { instance.get_physical_device_queue_family_properties(phys) };
    fams.iter()
        .position(|f| f.queue_flags.contains(vk::QueueFlags::TRANSFER))
        .or_else(|| {
            fams.iter().position(|f| {
                f.queue_flags
                    .intersects(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            })
        })
        .map(|i| i as u32)
}

/// Index of the first memory type that satisfies `type_bits` (bitmask of `MemoryRequirements`) and contains
/// all `want` flags. `None` if none fit.
fn pick_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    want: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0 && props.memory_types[i as usize].property_flags.contains(want)
    })
}

fn largest_device_local_heap_index(props: &vk::PhysicalDeviceMemoryProperties) -> Option<u32> {
    (0..props.memory_heap_count)
        .filter(|&index| {
            props.memory_heaps[index as usize]
                .flags
                .contains(vk::MemoryHeapFlags::DEVICE_LOCAL)
        })
        .max_by_key(|&index| props.memory_heaps[index as usize].size)
}

fn pick_memory_type_on_heap(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    want: vk::MemoryPropertyFlags,
    heap_index: u32,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&index| {
        (type_bits & (1 << index)) != 0
            && props.memory_types[index as usize].heap_index == heap_index
            && props.memory_types[index as usize]
                .property_flags
                .contains(want)
    })
}

fn rounded_buffer_size(bytes: usize) -> Result<u64, VramError> {
    u64::try_from(bytes)
        .ok()
        .and_then(|requested| requested.max(1).checked_add(3))
        .map(|rounded| rounded & !3)
        .ok_or_else(|| VramError::Provider("requested Vulkan allocation size overflow".into()))
}

/// Logical device resources created in `open` (loaded into `VulkanProvider` on success).
struct DeviceBits {
    device: ash::Device,
    queue: vk::Queue,
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_mapped: *mut u8,
}

/// RAII guard for the `goto out_err` (kernel idiom) in device creation: on error (any `?`),
/// destroys the already created resources in reverse order **and** the device. On success, `disarm()` prevents
/// cleanup and the handles are passed to the `VulkanProvider`.
struct ResGuard {
    device: ash::Device,
    cmd_pool: Option<vk::CommandPool>,
    fence: Option<vk::Fence>,
    staging_buffer: Option<vk::Buffer>,
    staging_memory: Option<vk::DeviceMemory>,
    mapped: bool,
    armed: bool,
}

impl ResGuard {
    fn new(device: ash::Device) -> Self {
        Self {
            device,
            cmd_pool: None,
            fence: None,
            staging_buffer: None,
            staging_memory: None,
            mapped: false,
            armed: true,
        }
    }
}

impl Drop for ResGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // SAFETY: all Some handles were created from self.device in this flow and are destroyed
        // exactly once (in reverse order of allocation). device_wait_idle guarantees nothing is
        // in-flight before freeing.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(m) = self.staging_memory {
                if self.mapped {
                    self.device.unmap_memory(m);
                }
                self.device.free_memory(m, None);
            }
            if let Some(b) = self.staging_buffer {
                self.device.destroy_buffer(b, None);
            }
            if let Some(f) = self.fence {
                self.device.destroy_fence(f, None);
            }
            if let Some(p) = self.cmd_pool {
                self.device.destroy_command_pool(p, None);
            }
            self.device.destroy_device(None);
        }
    }
}

/// Vulkan Provider (thread-affine — create/use in the same thread, same as CUDA context;
/// the queue is externally synchronized, DT-7). Reuses 1 staging buffer + 1 cmd buffer + 1 fence.
pub struct VulkanProvider {
    instance: ash::Instance,
    _entry: ash::Entry, // keeps the loader alive as long as the instance exists
    phys: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_mapped: *mut u8,
    allocated: AtomicU64, // Σ bytes allocated via `alloc` (fallback of `mem_info`, DT-10)
    name: String,
    adapter: Option<GpuAdapterIdentity>,
    memory_budget_extension: bool,
}

impl VulkanProvider {
    /// Returns the number of physical devices visible to the Vulkan loader.
    pub fn device_count() -> Result<u32, VramError> {
        // SAFETY: entry owns the loaded Vulkan loader for the instance lifetime below.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: ci and app remain valid for the duration of the call.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;
        // SAFETY: instance is valid and the query only enumerates device handles.
        let result = unsafe { instance.enumerate_physical_devices() }
            .map(|devices| devices.len().min(u32::MAX as usize) as u32)
            .map_err(|e| vk_err("enumerate_physical_devices", e));
        // SAFETY: instance was created above and is destroyed exactly once.
        unsafe { instance.destroy_instance(None) };
        result
    }

    /// Loads the Vulkan loader, creates an instance, selects the physical device (prefers `DISCRETE_GPU`;
    /// otherwise the ordinal), and sets up logical device + transfer queue + staging. RF-V1.
    pub fn open(ordinal: u32) -> Result<Self, VramError> {
        Self::open_with_selection(ordinal, false)
    }

    /// Opens exactly the enumerated physical-device ordinal, without discrete-GPU preference
    /// or clamping. This is used when comparing adapters across backends.
    pub fn open_exact(ordinal: u32) -> Result<Self, VramError> {
        Self::open_with_selection(ordinal, true)
    }

    fn open_with_selection(ordinal: u32, exact: bool) -> Result<Self, VramError> {
        // SAFETY: loads libvulkan.so.1 via libloading; symbols remain valid as long as `entry` lives.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `ci`/`app` valid during call; `None` = default allocator.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;

        // From this point on, any error must destroy the instance (goto out_err idiom).
        match Self::after_instance(&instance, ordinal, exact) {
            Ok((phys, name, adapter, memory_budget_extension, bits)) => Ok(Self {
                instance,
                _entry: entry,
                phys,
                device: bits.device,
                queue: bits.queue,
                cmd_pool: bits.cmd_pool,
                cmd_buf: bits.cmd_buf,
                fence: bits.fence,
                staging_buffer: bits.staging_buffer,
                staging_memory: bits.staging_memory,
                staging_mapped: bits.staging_mapped,
                allocated: AtomicU64::new(0),
                name,
                adapter,
                memory_budget_extension,
            }),
            Err(e) => {
                // SAFETY: `instance` created above and destroyed exactly once here.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    /// Device selection + name + creation of device resources (with its own cleanup on error).
    fn after_instance(
        instance: &ash::Instance,
        ordinal: u32,
        exact: bool,
    ) -> Result<
        (
            vk::PhysicalDevice,
            String,
            Option<GpuAdapterIdentity>,
            bool,
            DeviceBits,
        ),
        VramError,
    > {
        // SAFETY: `instance` valid.
        let pdevs = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| vk_err("enumerate_physical_devices", e))?;
        if pdevs.is_empty() {
            return Err(VramError::Provider("no Vulkan physical device".into()));
        }
        let phys = if exact {
            pdevs.get(ordinal as usize).copied().ok_or_else(|| {
                VramError::Provider(format!(
                    "Vulkan physical-device ordinal {ordinal} is out of range ({} devices)",
                    pdevs.len()
                ))
            })?
        } else {
            // Legacy/default open prefers a discrete GPU; cross-backend selection uses open_exact.
            let discrete = pdevs.iter().copied().find(|&p| {
                // SAFETY: `p` is a valid handle enumerated from `instance`.
                unsafe { instance.get_physical_device_properties(p) }.device_type
                    == vk::PhysicalDeviceType::DISCRETE_GPU
            });
            discrete.unwrap_or_else(|| pdevs[(ordinal as usize).min(pdevs.len() - 1)])
        };
        // SAFETY: `phys` valid; `device_name` is a fixed-size NUL-terminated C-string.
        let props = unsafe { instance.get_physical_device_properties(phys) };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let mut id_props = vk::PhysicalDeviceIDProperties::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id_props);
        // SAFETY: physical device was enumerated from this instance; properties2 is initialized.
        unsafe { instance.get_physical_device_properties2(phys, &mut props2) };
        let adapter_key = id_props.device_uuid;
        let uuid_key = adapter_key.iter().any(|byte| *byte != 0).then(|| {
            adapter_key
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        });
        let luid = (id_props.device_luid_valid == vk::TRUE)
            .then(|| format_luid(id_props.device_luid))
            .flatten();
        let adapter = match (uuid_key, luid) {
            (Some(key), luid) => Some(GpuAdapterIdentity {
                backend: "vulkan".into(),
                key,
                luid,
            }),
            (None, Some(luid)) => Some(GpuAdapterIdentity {
                backend: "vulkan".into(),
                key: format!("luid:{luid}"),
                luid: Some(luid),
            }),
            (None, None) => None,
        };
        // SAFETY: `phys` was enumerated from `instance`; this is a read-only capability query.
        let extensions = unsafe { instance.enumerate_device_extension_properties(phys) }
            .map_err(|e| vk_err("enumerate_device_extension_properties", e))?;
        let memory_budget_extension = extensions.iter().any(|extension| {
            // SAFETY: Vulkan extension_name is fixed-size and null-terminated by the driver.
            (unsafe { CStr::from_ptr(extension.extension_name.as_ptr()) })
                == c"VK_EXT_memory_budget"
        });
        let qf = pick_transfer_family(instance, phys)
            .ok_or_else(|| VramError::Provider("sem queue family de transfer".into()))?;
        let bits = create_device_resources(instance, phys, qf, memory_budget_extension)?;
        Ok((phys, name, adapter, memory_budget_extension, bits))
    }

    /// Name of the selected device (e.g., \"NVIDIA GeForce RTX 2060\" or \"llvmpipe\" in software).
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Size of the largest `DEVICE_LOCAL` heap, which is also the heap used by allocations.
    pub fn device_local_total(&self) -> u64 {
        // SAFETY: `phys` valid.
        let mp = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        largest_device_local_heap_index(&mp)
            .map(|index| mp.memory_heaps[index as usize].size)
            .unwrap_or(0)
    }

    /// Records + submits + waits for 1 command on the transfer queue (synchronous, DT-5).
    /// `record` writes to the reused `cmd_buf`; after `wait`, the fence is reset.
    /// Single-threaded (DT-7): no races on shared cmd_buf/fence/staging.
    fn submit_wait<F>(&self, record: F) -> Result<(), VramError>
    where
        F: FnOnce(&ash::Device, vk::CommandBuffer),
    {
        let dev = &self.device;
        let cmd = self.cmd_buf;
        // SAFETY: `cmd` came from the `cmd_pool` of this provider; reset before rewriting;
        // single-threaded usage. The recording calls inside `record` have their own `// SAFETY:`.
        unsafe {
            dev.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| vk_err("reset_command_buffer", e))?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            dev.begin_command_buffer(cmd, &begin)
                .map_err(|e| vk_err("begin_command_buffer", e))?;
            record(dev, cmd);
            dev.end_command_buffer(cmd)
                .map_err(|e| vk_err("end_command_buffer", e))?;
            let cmds = [cmd];
            let submits = [vk::SubmitInfo::default().command_buffers(&cmds)];
            dev.queue_submit(self.queue, &submits, self.fence)
                .map_err(|e| vk_err("queue_submit", e))?;
            let fences = [self.fence];
            dev.wait_for_fences(&fences, true, u64::MAX)
                .map_err(|e| vk_err("wait_for_fences", e))?;
            dev.reset_fences(&fences)
                .map_err(|e| vk_err("reset_fences", e))?;
        }
        Ok(())
    }
}

/// Creates logical device + queue + cmd pool/buffer + fence + mapped staging buffer, with RAII cleanup on error.
fn create_device_resources(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    qf: u32,
    memory_budget_extension: bool,
) -> Result<DeviceBits, VramError> {
    let prio = [1.0f32];
    let qci = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(qf)
        .queue_priorities(&prio)];
    let enabled_extensions = memory_budget_extension
        .then_some(c"VK_EXT_memory_budget".as_ptr())
        .into_iter()
        .collect::<Vec<_>>();
    let dci = vk::DeviceCreateInfo::default()
        .queue_create_infos(&qci)
        .enabled_extension_names(&enabled_extensions);
    // SAFETY: `dci`/`qci`/`prio` valid during call; `phys` enumerated from `instance`. Before
    // device creation, there are no resources to clean up (returns directly on failure).
    let device = unsafe { instance.create_device(phys, &dci, None) }
        .map_err(|e| vk_err("create_device", e))?;

    // From here on, every `?` is covered by `guard` (destroys children + device on error).
    let mut guard = ResGuard::new(device);

    // SAFETY: `guard.device`/`qf` valid.
    let queue = unsafe { guard.device.get_device_queue(qf, 0) };

    let pool_ci = vk::CommandPoolCreateInfo::default()
        .queue_family_index(qf)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    // SAFETY: device + pool_ci valid.
    let cmd_pool = unsafe { guard.device.create_command_pool(&pool_ci, None) }
        .map_err(|e| vk_err("create_command_pool", e))?;
    guard.cmd_pool = Some(cmd_pool);

    let cb_ai = vk::CommandBufferAllocateInfo::default()
        .command_pool(cmd_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: device + cb_ai valid; the cmd buffer(s) are freed together with the pool.
    let cbs = unsafe { guard.device.allocate_command_buffers(&cb_ai) }
        .map_err(|e| vk_err("allocate_command_buffers", e))?;
    let cmd_buf = cbs
        .first()
        .copied()
        .ok_or_else(|| VramError::Provider("allocate_command_buffers returned empty".into()))?;

    // SAFETY: device valid.
    let fence = unsafe {
        guard
            .device
            .create_fence(&vk::FenceCreateInfo::default(), None)
    }
    .map_err(|e| vk_err("create_fence", e))?;
    guard.fence = Some(fence);

    let buf_ci = vk::BufferCreateInfo::default()
        .size(STAGING_BYTES)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: device + buf_ci valid.
    let staging_buffer = unsafe { guard.device.create_buffer(&buf_ci, None) }
        .map_err(|e| vk_err("create_buffer(staging)", e))?;
    guard.staging_buffer = Some(staging_buffer);

    // SAFETY: buffer valid.
    let req = unsafe { guard.device.get_buffer_memory_requirements(staging_buffer) };
    // SAFETY: phys valid.
    let mprops = unsafe { instance.get_physical_device_memory_properties(phys) };
    let mt = pick_memory_type(
        &mprops,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or_else(|| {
        VramError::Provider("sem memory type HOST_VISIBLE|COHERENT p/ staging".into())
    })?;
    let mai = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(mt);
    // SAFETY: device + mai valid.
    let staging_memory = unsafe { guard.device.allocate_memory(&mai, None) }
        .map_err(|e| vk_err("allocate_memory(staging)", e))?;
    guard.staging_memory = Some(staging_memory);

    // SAFETY: buffer + memory valid; offset 0 satisfies the alignment of `req`.
    unsafe {
        guard
            .device
            .bind_buffer_memory(staging_buffer, staging_memory, 0)
    }
    .map_err(|e| vk_err("bind_buffer_memory(staging)", e))?;

    // SAFETY: newly allocated HOST_VISIBLE memory; maps the entire range.
    let raw = unsafe {
        guard.device.map_memory(
            staging_memory,
            0,
            STAGING_BYTES,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|e| vk_err("map_memory(staging)", e))?;
    guard.mapped = true;
    let staging_mapped = raw.cast::<u8>();

    // Success: disarms the guard and extracts the handles (the device is cloned — lightweight handle from ash;
    // the actual destroy is done in Drop of VulkanProvider).
    guard.armed = false;
    Ok(DeviceBits {
        device: guard.device.clone(),
        queue,
        cmd_pool,
        cmd_buf,
        fence,
        staging_buffer,
        staging_memory,
        staging_mapped,
    })
}

impl VramProvider for VulkanProvider {
    // GAT: memory borrows &self (same as CUDA's DeviceMem) -> thread affinity without Arc.
    type Mem<'p>
        = VulkanMem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        // Rounds buffer size to a multiple of 4 (requirement for vkCmdFillBuffer with WHOLE_SIZE
        // in zero); the logical len remains `bytes`.
        let buf_size = rounded_buffer_size(bytes)?;
        let buf_ci = vk::BufferCreateInfo::default()
            .size(buf_size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: device + buf_ci valid.
        let buffer = unsafe { self.device.create_buffer(&buf_ci, None) }
            .map_err(|e| vk_err("create_buffer", e))?;

        // SAFETY: buffer valid.
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        // SAFETY: phys valid.
        let mprops = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        let Some(heap_index) = largest_device_local_heap_index(&mprops) else {
            // SAFETY: buffer was created above and is destroyed before returning.
            unsafe { self.device.destroy_buffer(buffer, None) };
            return Err(VramError::Provider(
                "no DEVICE_LOCAL memory heap available for the buffer".into(),
            ));
        };
        let mt = match pick_memory_type_on_heap(
            &mprops,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            heap_index,
        ) {
            Some(i) => i,
            None => {
                // SAFETY: buffer created above; destroyed before returning (no leak).
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(VramError::Provider(
                    "no DEVICE_LOCAL memory type for the buffer".into(),
                ));
            }
        };
        let mai = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(mt);
        // SAFETY: device + mai valid.
        let memory = match unsafe { self.device.allocate_memory(&mai, None) } {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: buffer created above; destroyed on error.
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(vk_err("allocate_memory", e));
            }
        };
        // SAFETY: buffer + memory valid; offset 0.
        if let Err(e) = unsafe { self.device.bind_buffer_memory(buffer, memory, 0) } {
            // SAFETY: buffer + memory created above; freed in reverse order on error.
            unsafe {
                self.device.free_memory(memory, None);
                self.device.destroy_buffer(buffer, None);
            }
            return Err(vk_err("bind_buffer_memory", e));
        }
        self.allocated.fetch_add(bytes as u64, Ordering::Relaxed);
        Ok(VulkanMem {
            provider: self,
            buffer,
            memory,
            len: bytes,
        })
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        let budget = self.budget_snapshot()?;
        let total = budget.total_bytes.unwrap_or(0);
        Ok((budget.available_bytes().min(total), total))
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        if self.memory_budget_extension {
            let mut budget_props = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
            let mut memory_props =
                vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget_props);
            // SAFETY: physical device is valid and both output structures are initialized.
            unsafe {
                self.instance
                    .get_physical_device_memory_properties2(self.phys, &mut memory_props)
            };
            let props = memory_props.memory_properties;
            if let Some(index) = largest_device_local_heap_index(&props) {
                let index = index as usize;
                let budget_bytes = budget_props.heap_budget[index];
                if budget_bytes > 0 {
                    return Ok(GpuBudgetSnapshot {
                        adapter: self.adapter.clone(),
                        total_bytes: Some(props.memory_heaps[index].size),
                        budget_bytes,
                        used_bytes: budget_props.heap_usage[index],
                        source: GpuBudgetSource::DriverReported,
                        sampled_at: Instant::now(),
                    });
                }
            }
        }

        let total_bytes = self.device_local_total();
        let used_bytes = self.allocated.load(Ordering::Relaxed);
        Ok(GpuBudgetSnapshot {
            adapter: self.adapter.clone(),
            total_bytes: Some(total_bytes),
            budget_bytes: total_bytes,
            used_bytes,
            source: GpuBudgetSource::ProviderLocalEstimate,
            sampled_at: Instant::now(),
        })
    }
}

impl Drop for VulkanProvider {
    fn drop(&mut self) {
        // SAFETY: resources created in open, destroyed once in reverse order of allocation. All
        // VulkanMem have already dropped (borrowing &self), so staging/queue are idle;
        // device_wait_idle still guarantees quiescence. _entry/instance drop later (fields).
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.unmap_memory(self.staging_memory);
            self.device.free_memory(self.staging_memory, None);
            self.device.destroy_buffer(self.staging_buffer, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Vulkan VRAM region (GAT: borrows `&'p VulkanProvider`). RAII: `Drop` frees buffer+memory.
pub struct VulkanMem<'p> {
    provider: &'p VulkanProvider,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    len: usize,
}

impl VulkanMem<'_> {
    /// `off + len <= self.len`, otherwise `OutOfRange` (mirrors CUDA's bounds check).
    fn check_bounds(&self, off: u64, len: usize) -> Result<(), VramError> {
        match off.checked_add(len as u64) {
            Some(end) if end <= self.len as u64 => Ok(()),
            _ => Err(VramError::OutOfRange {
                off,
                len: len as u64,
                size: self.len as u64,
            }),
        }
    }
}

impl VramMemory for VulkanMem<'_> {
    fn len(&self) -> usize {
        self.len
    }

    fn zero(&mut self) -> Result<(), VramError> {
        let buffer = self.buffer;
        self.provider.submit_wait(|dev, cmd| {
            // SAFETY: `cmd` in recording; `buffer` of this provider; `WHOLE_SIZE` zeroes the
            // entire buffer (allocated as a multiple of 4 to satisfy `vkCmdFillBuffer`).
            unsafe { dev.cmd_fill_buffer(cmd, buffer, 0, vk::WHOLE_SIZE, 0) };
        })
    }

    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
        self.check_bounds(off, dst.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < dst.len() {
            let chunk = (dst.len() - done).min(STAGING_BYTES as usize);
            let src_off = off + done as u64;
            // GPU: copies `[src_off, src_off + chunk)` from the `DEVICE_LOCAL` buffer -> staging.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(src_off)
                    .dst_offset(0)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider; `chunk <= STAGING_BYTES` and bounds-checked on the buffer.
                unsafe { dev.cmd_copy_buffer(cmd, buffer, p.staging_buffer, &region) };
            })?;
            // Host: staging.mapped -> dst[done..].
            // SAFETY: `staging_mapped` has `STAGING_BYTES` bytes (`HOST_VISIBLE|HOST_COHERENT`, no flush);
            // `chunk <= STAGING_BYTES`; `dst[done..done+chunk]` is valid (slice bounds).
            unsafe {
                std::ptr::copy_nonoverlapping(p.staging_mapped, dst.as_mut_ptr().add(done), chunk)
            };
            done += chunk;
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
        self.check_bounds(off, src.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < src.len() {
            let chunk = (src.len() - done).min(STAGING_BYTES as usize);
            // Host: src[done..] -> staging.mapped.
            // SAFETY: `staging_mapped` has `STAGING_BYTES` bytes; `chunk <= STAGING_BYTES`;
            // `src[done..done+chunk]` is valid (slice bounds). `HOST_COHERENT`: no flush.
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(done), p.staging_mapped, chunk)
            };
            let dst_off = off + done as u64;
            // GPU: copies staging -> `[dst_off, dst_off + chunk)` on the `DEVICE_LOCAL` buffer.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(0)
                    .dst_offset(dst_off)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider; `chunk <= STAGING_BYTES` and bounds-checked on the buffer.
                unsafe { dev.cmd_copy_buffer(cmd, p.staging_buffer, buffer, &region) };
            })?;
            done += chunk;
        }
        Ok(())
    }
}

impl Drop for VulkanMem<'_> {
    fn drop(&mut self) {
        // SAFETY: buffer+memory created in `alloc` of this provider; destroyed once in reverse
        // order. The device remains alive (borrowing `&'p provider`).
        unsafe {
            self.provider.device.destroy_buffer(self.buffer, None);
            self.provider.device.free_memory(self.memory, None);
        }
        self.provider
            .allocated
            .fetch_sub(self.len as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn open_enumerates_device_and_heap() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        assert!(VulkanProvider::device_count().expect("counts Vulkan devices") > 0);
        assert!(!p.device_name().is_empty(), "device has a name");
        let total = p.device_local_total();
        let budget = p.budget_snapshot().expect("budget snapshot");
        assert_eq!(budget.total_bytes, Some(total));
        assert!(budget.available_bytes() <= budget.budget_bytes);
        if budget.source == GpuBudgetSource::DriverReported {
            assert!(
                budget.can_admit(0),
                "driver budget needs a stable adapter ID"
            );
        } else {
            assert!(
                !budget.can_admit(0),
                "provider-local estimate must not authorize automatic admission"
            );
        }
        eprintln!(
            "Vulkan device='{}' heap_total={} MiB budget_source={:?} adapter={:?}",
            p.device_name(),
            total >> 20,
            budget.source,
            budget.adapter
        );
        assert!(total > 0, "heap > 0");
    }

    #[test]
    #[ignore = "requires Vulkan loader + software or physical ICD"]
    fn exact_device_open_rejects_out_of_range_ordinal_without_clamping() {
        let count = VulkanProvider::device_count().expect("counts Vulkan devices");
        assert!(count > 0, "ICD exposes at least one device");
        let selected = VulkanProvider::open_exact(0).expect("opens exact ordinal zero");
        assert!(!selected.device_name().is_empty());
        assert!(matches!(
            VulkanProvider::open_exact(count),
            Err(VramError::Provider(message)) if message.contains("out of range")
        ));
    }

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe is enough; run with --ignored)"]
    fn vulkan_roundtrip_write_then_read() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        let initial_budget = p.budget_snapshot().expect("initial budget");
        let total = initial_budget.total_bytes.unwrap_or(0);
        assert!(total > 0, "total > 0");

        // 2 MiB region; payload > staging (1 MiB) and offset != 0 -> exercises the chunk loop.
        let size = 2 * 1024 * 1024;
        let mut m = p.alloc(size).expect("alloc 2 MiB");
        assert_eq!(p.allocated.load(Ordering::Relaxed), size as u64);
        assert_eq!(m.len(), size, "reported len = requested bytes");

        let n = (STAGING_BYTES as usize) + 4096; // 1 MiB + 4 KiB -> 2 chunks
        let off = 4096u64;
        let pattern: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        m.write_at(off, &pattern).expect("write");
        let mut back = vec![0u8; n];
        m.read_at(off, &mut back).expect("read");
        assert_eq!(back, pattern, "round-trip identical bytes");

        // zero zeroes the region.
        m.zero().expect("zero");
        m.read_at(off, &mut back).expect("read post-zero");
        assert!(back.iter().all(|&b| b == 0), "zero left everything as 0");

        // bounds-check.
        let mut one = [0u8; 1];
        assert!(
            matches!(
                m.read_at(size as u64, &mut one),
                Err(VramError::OutOfRange { .. })
            ),
            "read beyond the end -> OutOfRange"
        );

        drop(m);
        assert_eq!(
            p.allocated.load(Ordering::Relaxed),
            0,
            "RAII released provider allocation accounting"
        );
        let final_budget = p.budget_snapshot().expect("final budget");
        eprintln!(
            "Vulkan round-trip OK device='{}' total={} MiB budget={:?} -> {:?}",
            p.device_name(),
            total >> 20,
            initial_budget.source,
            final_budget.source
        );
    }

    #[test]
    fn device_local_allocation_matches_the_largest_reported_budget_heap() {
        let defaults = vk::PhysicalDeviceMemoryProperties::default();
        let mut memory_heaps = defaults.memory_heaps;
        memory_heaps[0] = vk::MemoryHeap::default()
            .size(4_000)
            .flags(vk::MemoryHeapFlags::DEVICE_LOCAL);
        memory_heaps[1] = vk::MemoryHeap::default()
            .size(8_000)
            .flags(vk::MemoryHeapFlags::DEVICE_LOCAL);
        let mut memory_types = defaults.memory_types;
        memory_types[0] = vk::MemoryType::default()
            .heap_index(0)
            .property_flags(vk::MemoryPropertyFlags::DEVICE_LOCAL);
        memory_types[1] = vk::MemoryType::default()
            .heap_index(1)
            .property_flags(vk::MemoryPropertyFlags::DEVICE_LOCAL);
        let properties = vk::PhysicalDeviceMemoryProperties {
            memory_heap_count: 2,
            memory_heaps,
            memory_type_count: 2,
            memory_types,
        };

        assert_eq!(largest_device_local_heap_index(&properties), Some(1));
        assert_eq!(
            pick_memory_type_on_heap(&properties, 0b11, vk::MemoryPropertyFlags::DEVICE_LOCAL, 1,),
            Some(1)
        );
        assert_eq!(
            pick_memory_type_on_heap(&properties, 0b01, vk::MemoryPropertyFlags::DEVICE_LOCAL, 1,),
            None,
            "allocation requirements that exclude the reported heap must fail closed"
        );
    }

    #[test]
    fn buffer_size_rounding_rejects_overflow() {
        assert_eq!(
            rounded_buffer_size(0).expect("zero rounds to minimal buffer"),
            4
        );
        assert_eq!(
            rounded_buffer_size(5).expect("rounds up to four-byte boundary"),
            8
        );
        assert!(rounded_buffer_size(usize::MAX).is_err());
    }
}
