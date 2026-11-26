//! Reflections brought down to the light around them.
//!
//! The title's reflections come from rays, which do not run here: what its
//! history pass for them (`CS_UpdateHistory`) keeps is the sky's light,
//! unoccluded, on every glossy surface, indoors as well. Seen as a white
//! glaze on sunlit ground and walls and as bright single pixels on skin a
//! lamp lights in close-ups (runs of 2026-11-25 on an 8 GB laptop).
//!
//! Right after that pass, each texel of its reflection image is put
//! through a soft knee: up to half of `ceiling` it is left as it is, above
//! that it bends towards `ceiling` and never reaches it. Hue is kept, only
//! the brightness is bent. The ceiling is `VICTORY_REFLECT_KNEE` (1.0 by
//! default) times the ambient light's level ([`crate::ambient`]), so the
//! reflections stay in step with the light that stands in for the traced
//! diffuse light. `VICTORY_REFLECT_KNEE=off` leaves the title's reflections
//! alone, as does the ambient light being off.
//! shortcut: stands in for what the title's traced reflections would see.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::s;

use crate::host::{HostList, host};
use crate::object::This;

/// The pass whose output is bent, by the start of its name.
pub(crate) const PROGRAM: &[u8] = b"CS_UpdateHistory";
/// Where it writes the reflections (RGB9E5 in 32-bit integers): root
/// parameter and place in its table.
pub(crate) const IMAGE: (u8, u32) = (1, 40);

/// The knee's ceiling as a part of the ambient light's level; `None` if
/// the reflections are left alone.
fn knee() -> Option<f32> {
    static KNEE: OnceLock<f32> = OnceLock::new();
    let knee = *KNEE.get_or_init(|| {
        let set = std::env::var("VICTORY_REFLECT_KNEE").unwrap_or_default();
        if set.trim() == "off" {
            return -1.0;
        }
        set.trim()
            .parse()
            .ok()
            .filter(|k: &f32| k.is_finite() && *k > 0.0)
            .unwrap_or(1.0)
    });
    (knee > 0.0).then_some(knee)
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=1, b0), DescriptorTable(UAV(u0))"
cbuffer Knee : register(b0) { float ceiling; };
RWTexture2D<uint> image : register(u0);
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float most = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(most, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (most * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void main(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    image.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    float3 c = unpack(image[at.xy]);
    // The brightest channel, so that a saturated colour is bent as hard
    // as a white one of the same strength.
    float most = max(c.r, max(c.g, c.b));
    float start = 0.5 * ceiling;
    if (!(most > start)) return;
    float room = ceiling - start;
    float bent = start + room * (1.0 - exp(-(most - start) / room));
    image[at.xy] = pack(c * (bent / most));
}
"#;

/// Views, used in turn: more than the passes of every list in flight.
const SLOTS: u32 = 1024;

struct Pass {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    views: ID3D12DescriptorHeap,
    step: u32,
    next: AtomicU32,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device) -> windows::core::Result<Pass> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and names live through the call; out pointers ours.
    let compiled = unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("reflect"),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let text = |b: &ID3DBlob| {
        // SAFETY: the blob's own bytes.
        unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize()) }
            .to_vec()
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            tracing::warn!(
                "reflection knee: {}",
                String::from_utf8_lossy(&text(&errors))
            );
        }
        return Err(e);
    }
    let code = text(&code.ok_or_else(windows::core::Error::empty)?);
    // SAFETY: the root signature is inside the shader; plain creations.
    unsafe {
        let root: ID3D12RootSignature = device.CreateRootSignature(0, &code)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: code.as_ptr().cast(),
                BytecodeLength: code.len(),
            },
            ..Default::default()
        })?;
        let kind = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        let views = device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: kind,
            NumDescriptors: SLOTS,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        })?;
        Ok(Pass {
            root,
            pso,
            views,
            step: device.GetDescriptorHandleIncrementSize(kind),
            next: AtomicU32::new(0),
        })
    }
}

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        ..Default::default()
    };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// Bends what `image` holds, in `list`, after the pass that wrote it as an
/// image. Whether it recorded anything (the list's bindings are then ours).
pub(crate) fn bend(list: &HostList, image: This) -> bool {
    let (Some(knee), Some(level), Some(host)) = (knee(), crate::ambient::level(), host()) else {
        return false;
    };
    if level <= 0.0 {
        return false;
    }
    let Some(image) = crate::resource::host_resource(image) else {
        return false;
    };
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    let Some(pass) = PASS
        .get_or_init(|| {
            make(&host.device)
                .inspect_err(|e| tracing::warn!(error = %e, "no reflection knee"))
                .ok()
        })
        .as_ref()
    else {
        return false;
    };
    // SAFETY: a live resource.
    let d = unsafe { image.GetDesc() };
    static ONCE: std::sync::Once = std::sync::Once::new();
    if d.Format != DXGI_FORMAT_R32_UINT || d.DepthOrArraySize != 1 {
        ONCE.call_once(|| {
            tracing::warn!(
                format = d.Format.0,
                layers = d.DepthOrArraySize,
                "no reflection knee: another image"
            );
        });
        return false;
    }
    let ceiling = knee * level;
    ONCE.call_once(|| {
        tracing::info!(
            ceiling,
            width = d.Width,
            height = d.Height,
            "reflection knee"
        );
    });
    let slot = pass.next.fetch_add(1, Ordering::Relaxed) % SLOTS;
    list.keep(image);
    let l = &list.list;
    uav_barrier(l);
    // SAFETY: an open list; a live texture the list keeps, in the
    // unordered-access state as the pass before left it; a view in a slot
    // no list in flight reads.
    unsafe {
        let cpu = pass.views.GetCPUDescriptorHandleForHeapStart();
        let gpu = pass.views.GetGPUDescriptorHandleForHeapStart();
        let at = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: cpu.ptr + (slot * pass.step) as usize,
        };
        let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        host.device
            .CreateUnorderedAccessView(image, None::<&ID3D12Resource>, Some(&view), at);
        l.SetDescriptorHeaps(&[Some(pass.views.clone())]);
        l.SetComputeRootSignature(&pass.root);
        l.SetPipelineState(&pass.pso);
        l.SetComputeRoot32BitConstants(0, 1, (&ceiling as *const f32).cast(), 0);
        l.SetComputeRootDescriptorTable(
            1,
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr + u64::from(slot * pass.step),
            },
        );
        l.Dispatch((d.Width as u32).div_ceil(8), d.Height.div_ceil(8), 1);
    }
    uav_barrier(l);
    true
}

#[cfg(test)]
mod tests {
    /// The pass compiles with the system's compiler and the host takes it.
    #[test]
    fn reflection_knee_builds() {
        let Some(host) = crate::host::host() else {
            return;
        };
        super::make(&host.device).expect("the reflection knee");
    }
}
