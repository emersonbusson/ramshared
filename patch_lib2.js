const fs = require('fs');
let content = fs.readFileSync('crates/ramshared-vulkan/src/lib.rs', 'utf8');

const allocStart = content.indexOf('    fn alloc(&self, bytes: usize) -> Result<Self::Mem<\'_>, VramError> {');
const memInfoStart = content.indexOf('    fn mem_info(&self) -> Result<(u64, u64), VramError> {');

let libAllocContent = content.slice(allocStart, memInfoStart);

content = content.replace(libAllocContent,
`    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        memory::alloc_memory(self, bytes)
    }

`);

// Also extract `create_device_resources` since it has `allocate_memory` and `map_memory`
const createStart = content.indexOf('/// Creates logical device + queue + cmd pool/buffer + fence + mapped staging buffer, with RAII cleanup on error.');
const createEnd = content.indexOf('impl VramProvider for VulkanProvider {');

let createResourcesContent = content.slice(createStart, createEnd);

content = content.replace(createResourcesContent, '');

// Also DeviceBits and ResGuard
const bitsStart = content.indexOf('/// Logical device resources created in `open` (loaded into `VulkanProvider` on success).');
const bitsEnd = content.indexOf('/// Vulkan Provider (thread-affine');

let bitsContent = content.slice(bitsStart, bitsEnd);

content = content.replace(bitsContent, '');

content = content.replace('use memory::pick_memory_type;', '');
content = content.replace('pub use memory::VulkanMem;', 'pub use memory::VulkanMem;\nuse memory::{DeviceBits, create_device_resources};');

content = content.replace('fn vk_err', 'pub(crate) fn vk_err');

content = content.replace(
    '    instance: ash::Instance,',
    '    pub(crate) instance: ash::Instance,'
);
content = content.replace(
    '    phys: vk::PhysicalDevice,',
    '    pub(crate) phys: vk::PhysicalDevice,'
);

fs.writeFileSync('crates/ramshared-vulkan/src/lib.rs', content);

let memoryContent = fs.readFileSync('crates/ramshared-vulkan/src/memory.rs', 'utf8');

memoryContent += `

${bitsContent.replace(/pub\(crate\) device:/g, 'pub device:').replace(/pub\(crate\) staging_buffer:/g, 'pub staging_buffer:').replace(/pub\(crate\) staging_mapped:/g, 'pub staging_mapped:')}

${createResourcesContent.replace('fn create_device_resources', 'pub(crate) fn create_device_resources').replace('fn vk_err', 'crate::vk_err').replace(/vk_err/g, 'crate::vk_err').replace('crate::vk_err("alloc', 'crate::vk_err("alloc')}

pub(crate) fn alloc_memory<'p>(provider: &'p VulkanProvider, bytes: usize) -> Result<VulkanMem<'p>, VramError> {
    // Rounds buffer size to a multiple of 4 (requirement for vkCmdFillBuffer with WHOLE_SIZE
    // in zero); the logical len remains \`bytes\`.
    let buf_size = ((bytes as u64).max(1) + 3) & !3;
    let buf_ci = vk::BufferCreateInfo::default()
        .size(buf_size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: device + buf_ci valid.
    let buffer = unsafe { provider.device.create_buffer(&buf_ci, None) }
        .map_err(|e| crate::vk_err("create_buffer", e))?;

    // SAFETY: buffer valid.
    let req = unsafe { provider.device.get_buffer_memory_requirements(buffer) };
    // SAFETY: phys valid.
    let mprops = unsafe {
        provider.instance
            .get_physical_device_memory_properties(provider.phys)
    };
    let mt = match pick_memory_type(
        &mprops,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    ) {
        Some(i) => i,
        None => {
            // SAFETY: buffer created above; destroyed before returning (no leak).
            unsafe { provider.device.destroy_buffer(buffer, None) };
            return Err(VramError::Provider(
                "no DEVICE_LOCAL memory type for the buffer".into(),
            ));
        }
    };
    let mai = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(mt);
    // SAFETY: device + mai valid.
    let memory = match unsafe { provider.device.allocate_memory(&mai, None) } {
        Ok(m) => m,
        Err(e) => {
            // SAFETY: buffer created above; destroyed on error.
            unsafe { provider.device.destroy_buffer(buffer, None) };
            return Err(crate::vk_err("allocate_memory", e));
        }
    };
    // SAFETY: buffer + memory valid; offset 0.
    if let Err(e) = unsafe { provider.device.bind_buffer_memory(buffer, memory, 0) } {
        // SAFETY: buffer + memory created above; freed in reverse order on error.
        unsafe {
            provider.device.free_memory(memory, None);
            provider.device.destroy_buffer(buffer, None);
        }
        return Err(crate::vk_err("bind_buffer_memory", e));
    }
    provider.allocated.fetch_add(bytes as u64, Ordering::Relaxed);
    Ok(VulkanMem {
        provider,
        buffer,
        memory,
        len: bytes,
    })
}
`;

memoryContent += `
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn pick_memory_type_finds_match() {
        let mut props = vk::PhysicalDeviceMemoryProperties::default();
        props.memory_type_count = 2;
        props.memory_types[0].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
        props.memory_types[1].property_flags = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;

        // bits: 0b10 (type 1)
        let mt = pick_memory_type(&props, 2, vk::MemoryPropertyFlags::HOST_VISIBLE);
        assert_eq!(mt, Some(1));

        // bits: 0b01 (type 0)
        let mt2 = pick_memory_type(&props, 1, vk::MemoryPropertyFlags::DEVICE_LOCAL);
        assert_eq!(mt2, Some(0));

        // flags don't match
        let mt3 = pick_memory_type(&props, 1, vk::MemoryPropertyFlags::HOST_VISIBLE);
        assert_eq!(mt3, None);
    }
}
`;

fs.writeFileSync('crates/ramshared-vulkan/src/memory.rs', memoryContent);
