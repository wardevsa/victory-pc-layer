//! An outstanding list's UAV and resource stay paired through another list's reset.
use super::HostTexture;
use crate::host::{Host, HostList};
use crate::rootsig::Param;
use std::sync::Arc;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*, Dxgi::*};
use windows::core::{Interface, s};

fn texture(host: &Host) -> Arc<HostTexture> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: 4,
        Height: 4,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut resource = None;
    // SAFETY: the descriptors and output pointer are valid for this call.
    unsafe {
        host.device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )
            .unwrap()
    };
    Arc::new(HostTexture::new(
        resource.unwrap(),
        D3D12_RESOURCE_STATE_COMMON.0 as u32,
    ))
}

fn record(
    host: &Host,
    list: &HostList,
    t: &Arc<HostTexture>,
    root: &ID3D12RootSignature,
    pso: &ID3D12PipelineState,
    marker: u32,
) -> ID3D12Resource {
    let view = t.march_uav(&host.device).unwrap();
    let back = host.read_back_buffer(4 * 256).unwrap();
    list.settle(t, D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32);
    // SAFETY: the list owns the texture and immutable heap; root, PSO and back outlive execution.
    unsafe {
        list.list.SetDescriptorHeaps(&[Some(view.clone())]);
        list.list.SetComputeRootSignature(root);
        list.list.SetPipelineState(pso);
        list.list.SetComputeRoot32BitConstant(0, marker, 0);
        list.list
            .SetComputeRootDescriptorTable(1, view.GetGPUDescriptorHandleForHeapStart());
        list.list.Dispatch(1, 1, 1);
    }
    list.settle(t, D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32);
    // SAFETY: the destination covers all four aligned rows and the borrowed resources stay live.
    unsafe {
        let dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(&back),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                        Width: 4,
                        Height: 4,
                        Depth: 1,
                        RowPitch: 256,
                    },
                },
            },
        };
        let src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(&t.texture),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        list.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
    }
    list.close();
    back
}

fn verify(back: &ID3D12Resource, marker: usize) {
    // All coordinates and markers in the shader are exactly representable in half precision.
    let half = [0u16, 0x3c00, 0x4000, 0x4200];
    // SAFETY: execution has finished; each row is 256 bytes and contains four RGBA16F texels.
    let got: Vec<[u16; 4]> = unsafe {
        let mut p = std::ptr::null_mut();
        back.Map(0, None, Some(&mut p)).unwrap();
        let data = std::slice::from_raw_parts(p.cast::<u16>(), 4 * 128);
        let pixels = (0..16)
            .map(|i| {
                let at = (i / 4) * 128 + (i % 4) * 4;
                data[at..at + 4].try_into().unwrap()
            })
            .collect();
        back.Unmap(0, None);
        pixels
    };
    for (i, pixel) in got.iter().enumerate() {
        assert_eq!(*pixel, [half[marker], half[i % 4], half[i / 4], half[1]]);
    }
}

#[test]
fn march_views_are_immutable_across_interleaved_lists_and_reset() {
    let Some(host) = crate::host::host() else {
        eprintln!("no D3D12 device; zero march UAV readbacks");
        return;
    };
    // SAFETY: the selected D3D12 device's own LUID identifies the adapter.
    let desc = unsafe {
        CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0))
            .unwrap()
            .EnumAdapterByLuid::<IDXGIAdapter1>(host.device.GetAdapterLuid())
            .unwrap()
            .GetDesc1()
            .unwrap()
    };
    let end = desc
        .Description
        .iter()
        .position(|&x| x == 0)
        .unwrap_or(desc.Description.len());
    let name = String::from_utf16_lossy(&desc.Description[..end]);
    let root = crate::rootsig::make(
        &host.device,
        &[
            Param::Constants {
                register: 0,
                count: 1,
            },
            Param::UavTable {
                register: 0,
                count: 1,
            },
        ],
        "march UAV lifetime test",
    )
    .unwrap();
    let code=crate::host::compile(
        "cbuffer C : register(b0) { uint mark; }; RWTexture2D<float4> image : register(u0);
         [numthreads(4,4,1)] void main(uint3 p:SV_DispatchThreadID) { image[p.xy]=float4(mark,p.x,p.y,1); }",
        s!("main"),s!("cs_5_1")).unwrap();
    // SAFETY: the root signature and bytecode outlive pipeline creation.
    let pso: ID3D12PipelineState = unsafe {
        host.device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&Some(root.clone())),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            })
            .unwrap()
    };
    let a = texture(host);
    let b = texture(host);
    let (a_weak, b_weak) = (Arc::downgrade(&a), Arc::downgrade(&b));
    let first = host.new_list().unwrap();
    let second = host.new_list().unwrap();
    let first_back = record(host, &first, &a, &root, &pso, 1);
    let a_heap = a.march_uav(&host.device).unwrap().as_raw();
    let b_heap = b.march_uav(&host.device).unwrap().as_raw();
    assert_ne!(a_heap, b_heap);
    // More requests than the old global ring had slots while A's recorded list is still outstanding.
    for _ in 0..4097 {
        assert_eq!(b.march_uav(&host.device).unwrap().as_raw(), b_heap);
    }
    let second_back = record(host, &second, &b, &root, &pso, 2);
    drop((a, b));
    assert!(a_weak.upgrade().is_some() && b_weak.upgrade().is_some());
    host.execute_sync_internal(&second);
    verify(&second_back, 2);
    second.reset();
    assert!(b_weak.upgrade().is_none(), "reset releases completed B");
    host.execute_sync_internal(&first);
    verify(&first_back, 1);
    let a = a_weak.upgrade().expect("unreset list still owns A");
    let third_back = record(host, &second, &a, &root, &pso, 3);
    drop(a);
    host.execute_sync_internal(&second);
    verify(&third_back, 3);
    second.reset();
    host.execute_sync_internal(&first);
    verify(&first_back, 1);
    first.reset();
    assert!(
        a_weak.upgrade().is_none(),
        "A is released after both lists reset"
    );
    eprintln!("{name}: four 4x4 RGBA16F readbacks, 256 component comparisons passed");
}
