//! `ramshared-vulkan` — backend Vulkan do `VramProvider` (RF-G2).
//!
//! 2ª implementação do trait `ramshared_vram::VramProvider` (a 1ª, CUDA, fica intacta), destravando
//! "qualquer GPU" + um host Linux nativo onde o ublk+VRAM e o eviction-sob-carga rodam e2e.
//!
//! **1ª fatia (esta):** `open` (loader + instância + seleção de physical device) + leitura do heap
//! `DEVICE_LOCAL`. Próximas: device lógico + transfer queue + staging (`alloc`/`mem_info`/`read_at`/
//! `write_at`/`zero`), conforme `docs/vulkan-backend/SPEC.md` (RF-V1..V4).
//!
//! Validável por **software** (lavapipe/llvmpipe) sem GPU — o `unsafe` (FFI `ash`) é isolado aqui
//! com `// SAFETY:` por bloco; a fronteira do trait é segura.

use std::ffi::CStr;

use ash::vk;
use ramshared_vram::VramError;

fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}

/// Provedor Vulkan (thread-afim — criar/usar na mesma thread, igual ao contexto CUDA).
pub struct VulkanProvider {
    instance: ash::Instance,
    _entry: ash::Entry, // mantém o loader vivo enquanto a instância existir
    phys: vk::PhysicalDevice,
    name: String,
}

impl VulkanProvider {
    /// Carrega o loader Vulkan, cria a instância e seleciona o physical device (prefere
    /// `DISCRETE_GPU`; senão o `ordinal`). RF-V1 — sem device lógico/queue nesta fatia.
    pub fn open(ordinal: u32) -> Result<Self, VramError> {
        // SAFETY: carrega libvulkan.so.1 via libloading; os símbolos vivem enquanto `entry` viver.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `ci`/`app` válidos durante a chamada; `None` = allocator padrão.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;

        // SAFETY: `instance` válida. Em erro, destrói a instância (idiom goto out_err).
        let pdevs = match unsafe { instance.enumerate_physical_devices() } {
            Ok(v) => v,
            Err(e) => {
                unsafe { instance.destroy_instance(None) };
                return Err(vk_err("enumerate_physical_devices", e));
            }
        };
        if pdevs.is_empty() {
            unsafe { instance.destroy_instance(None) };
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
        Ok(Self {
            instance,
            _entry: entry,
            phys,
            name,
        })
    }

    /// Nome do device selecionado (ex.: "NVIDIA GeForce RTX 2060" ou "llvmpipe" no software).
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Tamanho do maior heap `DEVICE_LOCAL` (bytes) — base do `total` do futuro `mem_info`
    /// (RF-V2). Sem `VK_EXT_memory_budget` ainda; fallback p/ o maior heap se não houver DEVICE_LOCAL
    /// (caso de software/unificado).
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
}

impl Drop for VulkanProvider {
    fn drop(&mut self) {
        // SAFETY: `instance` criada em `open` e destruída exatamente uma vez aqui.
        unsafe { self.instance.destroy_instance(None) };
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
}
