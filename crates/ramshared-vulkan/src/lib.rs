//! `ramshared-vulkan` — backend Vulkan do `VramProvider` (RF-G2).
//!
//! 2ª implementação do trait `ramshared_vram::VramProvider` (a 1ª, CUDA, fica intacta), destravando
//! "qualquer GPU" + um host Linux nativo onde o ublk+VRAM e o eviction-sob-carga rodam e2e.
//!
//! **IMPL completa (RF-V1..V3):** `open` faz loader, instância, physical device, device lógico,
//! transfer queue e staging `HOST_VISIBLE|HOST_COHERENT`. `impl VramProvider` cobre `alloc`
//! (`DEVICE_LOCAL`) e `mem_info`. `impl VramMemory` cobre `read_at`/`write_at` (staging +
//! `vkCmdCopyBuffer` + `VkFence`) e `zero` (`vkCmdFillBuffer`). Conforme
//! `docs/vulkan-backend/SPEC.md` (DT-1..DT-10).
//!
//! Validável por **software** (lavapipe/llvmpipe) sem GPU — todo `unsafe` (FFI `ash`) é isolado aqui
//! com `// SAFETY:` por bloco; a fronteira do trait é segura. `mem_info` usa `VK_EXT_memory_budget`
//! quando presente; senão o fallback DT-10 (maior heap `DEVICE_LOCAL` − Σ alocado).

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

use ash::vk;
use ramshared_vram::{VramError, VramMemory, VramProvider};

/// Staging buffer único por provider (sem alloc no hot path, DT-8): 1 MiB. I/O maior é fatiado.
const STAGING_BYTES: u64 = 1 << 20;

fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}

/// Escolhe uma queue family de transfer (prefere `TRANSFER` explícito; aceita `GRAPHICS`/`COMPUTE`,
/// que implicam transfer pela spec). Retorna o índice da família.
fn pick_transfer_family(instance: &ash::Instance, phys: vk::PhysicalDevice) -> Option<u32> {
    // SAFETY: `phys` foi enumerado de `instance`; a query só lê propriedades.
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

/// Índice do 1º memory type que satisfaz `type_bits` (bitmask de `MemoryRequirements`) e contém
/// todas as `want` flags. `None` se nenhum servir.
fn pick_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    want: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0 && props.memory_types[i as usize].property_flags.contains(want)
    })
}

/// Recursos do device lógico criados em `open` (carregados p/ o `VulkanProvider` em caso de sucesso).
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

/// Guarda RAII p/ o `goto out_err` (kernel idiom) na criação do device: em erro (qualquer `?`),
/// destrói os recursos já criados na ordem inversa **e** o device. Em sucesso, `disarm()` impede a
/// limpeza e os handles seguem p/ o `VulkanProvider`.
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
        // SAFETY: todos os handles `Some` foram criados de `self.device` neste fluxo e são destruídos
        // exatamente uma vez (ordem inversa da alocação). `device_wait_idle` garante que nada está em
        // voo antes de liberar.
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

/// Provedor Vulkan (thread-afim — criar/usar na mesma thread, igual ao contexto CUDA; a fila é
/// externamente sincronizada, DT-7). Mantém 1 staging buffer + 1 cmd buffer + 1 fence reusados.
pub struct VulkanProvider {
    instance: ash::Instance,
    _entry: ash::Entry, // mantém o loader vivo enquanto a instância existir
    phys: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_mapped: *mut u8,
    allocated: AtomicU64, // Σ bytes alocados via `alloc` (fallback do `mem_info`, DT-10)
    name: String,
}

impl VulkanProvider {
    /// Carrega o loader Vulkan, cria instância, seleciona o physical device (prefere `DISCRETE_GPU`;
    /// senão o `ordinal`) e monta o device lógico + transfer queue + staging. RF-V1.
    pub fn open(ordinal: u32) -> Result<Self, VramError> {
        // SAFETY: carrega libvulkan.so.1 via libloading; os símbolos vivem enquanto `entry` viver.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `ci`/`app` válidos durante a chamada; `None` = allocator padrão.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;

        // A partir daqui, qualquer erro precisa destruir a instância (idiom goto out_err).
        match Self::after_instance(&instance, ordinal) {
            Ok((phys, name, bits)) => Ok(Self {
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
            }),
            Err(e) => {
                // SAFETY: `instance` criada acima e destruída exatamente uma vez aqui.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    /// Seleção de device + nome + montagem dos recursos do device (já com cleanup próprio em erro).
    fn after_instance(
        instance: &ash::Instance,
        ordinal: u32,
    ) -> Result<(vk::PhysicalDevice, String, DeviceBits), VramError> {
        // SAFETY: `instance` válida.
        let pdevs = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| vk_err("enumerate_physical_devices", e))?;
        if pdevs.is_empty() {
            return Err(VramError::Provider("nenhum physical device Vulkan".into()));
        }
        // Prefere uma GPU discreta; senão o ordinal pedido (clampado).
        let discrete = pdevs.iter().copied().find(|&p| {
            // SAFETY: `p` é um handle válido enumerado de `instance`.
            unsafe { instance.get_physical_device_properties(p) }.device_type
                == vk::PhysicalDeviceType::DISCRETE_GPU
        });
        let phys = discrete.unwrap_or_else(|| pdevs[(ordinal as usize).min(pdevs.len() - 1)]);
        // SAFETY: `phys` válido; `device_name` é C-string NUL-terminado de tamanho fixo.
        let props = unsafe { instance.get_physical_device_properties(phys) };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let qf = pick_transfer_family(instance, phys)
            .ok_or_else(|| VramError::Provider("sem queue family de transfer".into()))?;
        let bits = create_device_resources(instance, phys, qf)?;
        Ok((phys, name, bits))
    }

    /// Nome do device selecionado (ex.: "NVIDIA GeForce RTX 2060" ou "llvmpipe" no software).
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Tamanho do maior heap `DEVICE_LOCAL` (bytes) — base do `total` do `mem_info` (DT-10). Fallback
    /// p/ o maior heap se não houver DEVICE_LOCAL (caso de software/memória unificada).
    pub fn device_local_total(&self) -> u64 {
        // SAFETY: `phys` válido.
        let mp = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        let heaps = &mp.memory_heaps[..mp.memory_heap_count as usize];
        heaps
            .iter()
            .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
            .map(|h| h.size)
            .max()
            .or_else(|| heaps.iter().map(|h| h.size).max())
            .unwrap_or(0)
    }

    /// Grava + submete + espera 1 comando na transfer queue (síncrono, DT-5). O `record` grava no
    /// `cmd_buf` reusado; após o `wait`, a fence é resetada. Single-thread (DT-7): sem corrida no
    /// cmd_buf/fence/staging compartilhados.
    fn submit_wait<F>(&self, record: F) -> Result<(), VramError>
    where
        F: FnOnce(&ash::Device, vk::CommandBuffer),
    {
        let dev = &self.device;
        let cmd = self.cmd_buf;
        // SAFETY: `cmd` veio do `cmd_pool` deste provider; resetado antes de regravar; uso
        // single-thread. As chamadas de gravação dentro de `record` têm seu próprio `// SAFETY:`.
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

/// Cria device lógico + queue + cmd pool/buffer + fence + staging mapeado, com cleanup RAII em erro.
fn create_device_resources(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    qf: u32,
) -> Result<DeviceBits, VramError> {
    let prio = [1.0f32];
    let qci = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(qf)
        .queue_priorities(&prio)];
    let dci = vk::DeviceCreateInfo::default().queue_create_infos(&qci);
    // SAFETY: `dci`/`qci`/`prio` válidos durante a chamada; `phys` enumerado de `instance`. Antes do
    // device não há recurso a limpar (se falhar, retorna direto).
    let device = unsafe { instance.create_device(phys, &dci, None) }
        .map_err(|e| vk_err("create_device", e))?;

    // Daqui pra baixo todo `?` é coberto pelo `guard` (destrói children + device em erro).
    let mut guard = ResGuard::new(device);

    // SAFETY: `guard.device`/`qf` válidos.
    let queue = unsafe { guard.device.get_device_queue(qf, 0) };

    let pool_ci = vk::CommandPoolCreateInfo::default()
        .queue_family_index(qf)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    // SAFETY: device + pool_ci válidos.
    let cmd_pool = unsafe { guard.device.create_command_pool(&pool_ci, None) }
        .map_err(|e| vk_err("create_command_pool", e))?;
    guard.cmd_pool = Some(cmd_pool);

    let cb_ai = vk::CommandBufferAllocateInfo::default()
        .command_pool(cmd_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: device + cb_ai válidos; o(s) cmd buffer(s) são liberados junto com o pool.
    let cbs = unsafe { guard.device.allocate_command_buffers(&cb_ai) }
        .map_err(|e| vk_err("allocate_command_buffers", e))?;
    let cmd_buf = cbs
        .first()
        .copied()
        .ok_or_else(|| VramError::Provider("allocate_command_buffers devolveu vazio".into()))?;

    // SAFETY: device válido.
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
    // SAFETY: device + buf_ci válidos.
    let staging_buffer = unsafe { guard.device.create_buffer(&buf_ci, None) }
        .map_err(|e| vk_err("create_buffer(staging)", e))?;
    guard.staging_buffer = Some(staging_buffer);

    // SAFETY: buffer válido.
    let req = unsafe { guard.device.get_buffer_memory_requirements(staging_buffer) };
    // SAFETY: phys válido.
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
    // SAFETY: device + mai válidos.
    let staging_memory = unsafe { guard.device.allocate_memory(&mai, None) }
        .map_err(|e| vk_err("allocate_memory(staging)", e))?;
    guard.staging_memory = Some(staging_memory);

    // SAFETY: buffer + memory válidos; offset 0 satisfaz o alinhamento de `req`.
    unsafe {
        guard
            .device
            .bind_buffer_memory(staging_buffer, staging_memory, 0)
    }
    .map_err(|e| vk_err("bind_buffer_memory(staging)", e))?;

    // SAFETY: memory HOST_VISIBLE recém-alocada; mapeia toda a faixa.
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

    // Sucesso: desarma o guard e leva os handles (o device é clonado — handle leve do `ash`; o
    // destroy real fica no `Drop` do `VulkanProvider`).
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
    // GAT: a memória empresta `&self` (igual ao `DeviceMem` do CUDA) → afinidade de thread sem `Arc`.
    type Mem<'p>
        = VulkanMem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        // Arredonda o buffer p/ múltiplo de 4 (requisito do `vkCmdFillBuffer` com WHOLE_SIZE no
        // `zero`); o `len` lógico continua sendo `bytes`.
        let buf_size = ((bytes as u64).max(1) + 3) & !3;
        let buf_ci = vk::BufferCreateInfo::default()
            .size(buf_size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: device + buf_ci válidos.
        let buffer = unsafe { self.device.create_buffer(&buf_ci, None) }
            .map_err(|e| vk_err("create_buffer", e))?;

        // SAFETY: buffer válido.
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        // SAFETY: phys válido.
        let mprops = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        let mt = match pick_memory_type(
            &mprops,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ) {
            Some(i) => i,
            None => {
                // SAFETY: buffer criado acima; destruído antes de retornar (sem leak).
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(VramError::Provider(
                    "sem memory type DEVICE_LOCAL p/ o buffer".into(),
                ));
            }
        };
        let mai = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(mt);
        // SAFETY: device + mai válidos.
        let memory = match unsafe { self.device.allocate_memory(&mai, None) } {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: buffer criado acima; destruído no erro.
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(vk_err("allocate_memory", e));
            }
        };
        // SAFETY: buffer + memory válidos; offset 0.
        if let Err(e) = unsafe { self.device.bind_buffer_memory(buffer, memory, 0) } {
            // SAFETY: buffer + memory criados acima; liberados na ordem inversa no erro.
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
        // DT-10 (fallback sem VK_EXT_memory_budget): total = maior heap DEVICE_LOCAL; free = total −
        // Σ alocado por este provider. (Budget exato p/ VRAM de outros processos: só no real-GPU.)
        let total = self.device_local_total();
        let used = self.allocated.load(Ordering::Relaxed);
        Ok((total.saturating_sub(used), total))
    }
}

impl Drop for VulkanProvider {
    fn drop(&mut self) {
        // SAFETY: recursos criados em `open`, destruídos uma vez na ordem inversa da alocação. Todos
        // os `VulkanMem` já caíram (emprestam `&self`), então o staging/fila estão ociosos; ainda
        // assim `device_wait_idle` garante quiesce. `_entry`/`instance` caem depois (campos).
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

/// Região de VRAM Vulkan (GAT: empresta `&'p VulkanProvider`). RAII: `Drop` libera buffer+memory.
pub struct VulkanMem<'p> {
    provider: &'p VulkanProvider,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    len: usize,
}

impl VulkanMem<'_> {
    /// `off + len ≤ self.len`, senão `OutOfRange` (espelha o bounds-check do CUDA).
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
            // SAFETY: `cmd` em recording; `buffer` deste provider; WHOLE_SIZE zera todo o buffer
            // (alocado múltiplo de 4 p/ satisfazer o `vkCmdFillBuffer`).
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
            // GPU: copia [src_off, +chunk) do buffer DEVICE_LOCAL → staging.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(src_off)
                    .dst_offset(0)
                    .size(chunk as u64)];
                // SAFETY: buffers do provider; `chunk ≤ STAGING_BYTES` e bounds-checked no buffer.
                unsafe { dev.cmd_copy_buffer(cmd, buffer, p.staging_buffer, &region) };
            })?;
            // Host: staging.mapped → dst[done..].
            // SAFETY: `staging_mapped` tem STAGING_BYTES bytes (HOST_VISIBLE|COHERENT, sem flush);
            // `chunk ≤ STAGING_BYTES`; `dst[done..done+chunk]` é válido (bounds do slice).
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
            // Host: src[done..] → staging.mapped.
            // SAFETY: `staging_mapped` tem STAGING_BYTES bytes; `chunk ≤ STAGING_BYTES`;
            // `src[done..done+chunk]` é válido (bounds do slice). HOST_COHERENT: sem flush.
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(done), p.staging_mapped, chunk)
            };
            let dst_off = off + done as u64;
            // GPU: copia staging → [dst_off, +chunk) do buffer DEVICE_LOCAL.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(0)
                    .dst_offset(dst_off)
                    .size(chunk as u64)];
                // SAFETY: buffers do provider; `chunk ≤ STAGING_BYTES` e bounds-checked no buffer.
                unsafe { dev.cmd_copy_buffer(cmd, p.staging_buffer, buffer, &region) };
            })?;
            done += chunk;
        }
        Ok(())
    }
}

impl Drop for VulkanMem<'_> {
    fn drop(&mut self) {
        // SAFETY: buffer+memory criados em `alloc` deste provider; destruídos uma vez na ordem
        // inversa. O device segue vivo (emprestamos `&'p provider`).
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
    #[ignore = "requer loader Vulkan + ICD (lavapipe/llvmpipe basta; rodar com --ignored)"]
    fn open_enumerates_device_and_heap() {
        let p = VulkanProvider::open(0).expect("abre Vulkan");
        assert!(!p.device_name().is_empty(), "device tem nome");
        let total = p.device_local_total();
        eprintln!(
            "Vulkan device='{}' heap_total={} MiB",
            p.device_name(),
            total >> 20
        );
        assert!(total > 0, "heap > 0");
    }

    #[test]
    #[ignore = "requer loader Vulkan + ICD (lavapipe basta; rodar com --ignored)"]
    fn vulkan_roundtrip_write_then_read() {
        let p = VulkanProvider::open(0).expect("abre Vulkan");
        let (free0, total) = p.mem_info().expect("mem_info");
        assert!(total > 0, "total > 0");

        // 2 MiB de região; payload > staging (1 MiB) e offset != 0 → exercita o loop de chunks.
        let size = 2 * 1024 * 1024;
        let mut m = p.alloc(size).expect("alloc 2 MiB");
        assert_eq!(m.len(), size, "len reportado = bytes pedidos");

        let n = (STAGING_BYTES as usize) + 4096; // 1 MiB + 4 KiB → 2 chunks
        let off = 4096u64;
        let pattern: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        m.write_at(off, &pattern).expect("write");
        let mut back = vec![0u8; n];
        m.read_at(off, &mut back).expect("read");
        assert_eq!(back, pattern, "round-trip bytes iguais");

        // zero zera a região.
        m.zero().expect("zero");
        m.read_at(off, &mut back).expect("read pós-zero");
        assert!(back.iter().all(|&b| b == 0), "zero deixou tudo 0");

        // bounds-check.
        let mut one = [0u8; 1];
        assert!(
            matches!(
                m.read_at(size as u64, &mut one),
                Err(VramError::OutOfRange { .. })
            ),
            "read além do fim → OutOfRange"
        );

        // free caiu após alloc (fallback DT-10).
        let (free1, _) = p.mem_info().expect("mem_info 2");
        assert!(free1 <= free0, "free não aumentou após alloc");
        eprintln!(
            "Vulkan round-trip OK device='{}' total={} MiB free0={} MiB free1={} MiB",
            p.device_name(),
            total >> 20,
            free0 >> 20,
            free1 >> 20
        );
    }
}
