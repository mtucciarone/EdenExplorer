use super::adapter_rank;
use eframe::wgpu::{Backend, DeviceType};

fn pick(adapters: &[(DeviceType, Backend)]) -> (DeviceType, Backend) {
    *adapters
        .iter()
        .max_by_key(|(device_type, backend)| adapter_rank(*device_type, *backend))
        .unwrap()
}

#[test]
fn discrete_gpu_on_dx12_is_preferred() {
    let adapters = [
        (DeviceType::DiscreteGpu, Backend::Dx12),
        (DeviceType::DiscreteGpu, Backend::Vulkan),
        (DeviceType::IntegratedGpu, Backend::Dx12),
        (DeviceType::Cpu, Backend::Dx12),
    ];
    assert_eq!(pick(&adapters), (DeviceType::DiscreteGpu, Backend::Dx12));
}

#[test]
fn integrated_gpu_beats_the_software_renderer() {
    let adapters = [
        (DeviceType::IntegratedGpu, Backend::Dx12),
        (DeviceType::IntegratedGpu, Backend::Vulkan),
        (DeviceType::Cpu, Backend::Dx12),
    ];
    assert_eq!(pick(&adapters), (DeviceType::IntegratedGpu, Backend::Dx12));
}

#[test]
fn integrated_gpu_without_dx12_still_beats_the_software_renderer() {
    let adapters = [
        (DeviceType::Cpu, Backend::Dx12),
        (DeviceType::IntegratedGpu, Backend::Gl),
    ];
    assert_eq!(pick(&adapters), (DeviceType::IntegratedGpu, Backend::Gl));
}
