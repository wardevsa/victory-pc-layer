//! Experimental GPU detiling of large, read-only 2D textures.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use victory_formats::texture::{Dimension, Layout, Swizzle};
use windows::Win32::Graphics::Direct3D::{Fxc::D3DCompile, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{Host, transition};

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=7, b0), SRV(t0), UAV(u0)"
cbuffer Params : register(b0) {
    uint width, height, element_bytes, rows, columns, pitch, output;
};
ByteAddressBuffer offsets : register(t0);
RWByteAddressBuffer scratch : register(u0);
[RootSignature(RS)]
[numthreads(32, 4, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= width || id.y >= height) return;
    uint row = offsets.Load(rows + id.y * 4);
    uint src = row + offsets.Load(columns + id.x * 4);
    uint dst = output + id.y * pitch + id.x * element_bytes;
    if (element_bytes == 16)
        scratch.Store4(dst, scratch.Load4(src));
    else if (element_bytes == 8)
        scratch.Store2(dst, scratch.Load2(src));
    else
        scratch.Store(dst, scratch.Load(src));
}
"#;

struct Kernel {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
}

static DEVICE_BYTES: AtomicU64 = AtomicU64::new(0);
const BUDGET: u64 = 64 << 20;

pub(super) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_GPU_DETILE").is_some_and(|value| value == "1"))
        && !crate::host::late()
}

fn kernel(host: &Host) -> Option<&'static Kernel> {
    static KERNEL: OnceLock<Option<Kernel>> = OnceLock::new();
    KERNEL
        .get_or_init(|| match compile(&host.device) {
            Ok(kernel) => Some(kernel),
            Err(error) => {
                tracing::warn!(%error, "GPU texture detiling unavailable");
                None
            }
        })
        .as_ref()
}

fn compile(device: &ID3D12Device) -> windows::core::Result<Kernel> {
    let mut blob: Option<ID3DBlob> = None;
    // SAFETY: the source, entry and target are live through compilation.
    unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("gpu texture detile"),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut blob,
            None,
        )?;
        let blob = blob.ok_or_else(windows::core::Error::empty)?;
        let bytes =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: bytes.as_ptr().cast(),
                BytecodeLength: bytes.len(),
            },
            ..Default::default()
        })?;
        Ok(Kernel { root, pso })
    }
}

struct Reservation(u64);

impl Reservation {
    fn take(bytes: u64) -> Option<Self> {
        DEVICE_BYTES
            .fetch_update(Relaxed, Relaxed, |used| {
                used.checked_add(bytes).filter(|&next| next <= BUDGET)
            })
            .ok()?;
        Some(Self(bytes))
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        DEVICE_BYTES.fetch_sub(self.0, Relaxed);
    }
}

pub(super) struct GpuDetile {
    kernel: &'static Kernel,
    upload: ID3D12Resource,
    offsets: ID3D12Resource,
    pub(super) scratch: ID3D12Resource,
    upload_at: usize,
    raw_bytes: u64,
    pub(super) linear_base: u64,
    dispatches: Vec<[u32; 7]>,
    _reservation: Reservation,
}

impl GpuDetile {
    pub(super) fn new(
        host: &Host,
        layout: &Layout,
        footprints: &[D3D12_PLACED_SUBRESOURCE_FOOTPRINT],
        rows: &[u32],
        linear_bytes: u64,
    ) -> Option<Self> {
        let desc = &layout.desc;
        if desc.dimension != Dimension::Tex2d
            || desc.depth != 1
            || !matches!(desc.element_bytes, 4 | 8 | 16)
            || desc.swizzle == Swizzle::LINEAR
            || !layout.addressable()
            || layout.mips.len() != footprints.len()
            || footprints.len() != rows.len()
            || !layout.size.is_multiple_of(4)
        {
            return None;
        }
        let raw_bytes = layout.size;
        let linear_base = raw_bytes.checked_next_multiple_of(512)?;
        let bytes = linear_base.checked_add(linear_bytes.checked_next_multiple_of(4)?)?;
        u32::try_from(bytes).ok()?;
        let reservation = Reservation::take(bytes)?;
        let kernel = kernel(host)?;
        let mut offsets = Vec::new();
        let mut dispatches = Vec::new();
        let meta_offset = |count: usize| u32::try_from(count.checked_mul(4)?).ok();
        for (index, ((mip, footprint), &height)) in
            layout.mips.iter().zip(footprints).zip(rows).enumerate()
        {
            let pitch = footprint.Footprint.RowPitch;
            let row_bytes = mip.width.checked_mul(desc.element_bytes)?;
            if mip.height != height
                || footprint.Footprint.Depth != 1
                || row_bytes > pitch
                || !footprint.Offset.is_multiple_of(4)
            {
                return None;
            }
            let end = footprint
                .Offset
                .checked_add(u64::from(height.checked_sub(1)?) * u64::from(pitch))?
                .checked_add(u64::from(row_bytes))?;
            if end > linear_bytes {
                return None;
            }
            let index = index as u32;
            let base = layout.offset(index, 0, 0, 0).ok()?;
            let row_index = meta_offset(offsets.len())?;
            for y in 0..height {
                let row = layout.offset(index, 0, y, 0).ok()?;
                offsets.push(u32::try_from(row).ok()?);
            }
            let col_index = meta_offset(offsets.len())?;
            for x in 0..mip.width {
                let column = layout.offset(index, x, 0, 0).ok()?.checked_sub(base)?;
                offsets.push(u32::try_from(column).ok()?);
            }
            let output = u32::try_from(linear_base + footprint.Offset).ok()?;
            dispatches.push([
                mip.width,
                height,
                desc.element_bytes,
                row_index,
                col_index,
                pitch,
                output,
            ]);
        }
        let (upload, upload_at) = host.upload_buffer(raw_bytes)?;
        let (table, table_at) = host.upload_buffer(size_of_val(offsets.as_slice()) as u64)?;
        // SAFETY: both CPU pointers name newly allocated upload buffers;
        // the offsets fit the table and the initial tiled contents are zero.
        unsafe {
            std::ptr::copy_nonoverlapping(offsets.as_ptr(), table_at.cast(), offsets.len());
            upload_at.write_bytes(0, raw_bytes as usize);
        }
        let scratch = host.uav_buffer(bytes)?;
        Some(Self {
            kernel,
            upload,
            offsets: table,
            scratch,
            upload_at: upload_at as usize,
            raw_bytes,
            linear_base,
            dispatches,
            _reservation: reservation,
        })
    }

    /// The staging lock serializes CPU updates. The experiment uses only
    /// submissions that finish before the next one prepares its uploads.
    pub(super) fn fill(&self, bytes: &[u8]) {
        assert_eq!(bytes.len(), self.raw_bytes as usize);
        // SAFETY: this object holds a mapped upload with exactly raw_bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.upload_at as *mut u8, bytes.len())
        };
    }

    pub(super) fn bytes(&self) -> u64 {
        self._reservation.0 + self.raw_bytes
    }

    /// Records into the list prelude, leaving scratch in COMMON. The
    /// consumer list promotes it to COPY_SOURCE and restores COMMON.
    pub(super) fn record(&self, list: &ID3D12GraphicsCommandList) {
        let common = D3D12_RESOURCE_STATE_COMMON;
        let copy = D3D12_RESOURCE_STATE_COPY_DEST;
        let uav = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
        // SAFETY: the staging object keeps all resources alive through both
        // command lists. The input and output occupy disjoint regions of
        // scratch and each dispatch writes only its own footprint.
        unsafe {
            list.ResourceBarrier(&[transition(&self.scratch, common, copy)]);
            list.CopyBufferRegion(&self.scratch, 0, &self.upload, 0, self.raw_bytes);
            list.ResourceBarrier(&[transition(&self.scratch, copy, uav)]);
            list.SetComputeRootSignature(&self.kernel.root);
            list.SetPipelineState(&self.kernel.pso);
            list.SetComputeRootShaderResourceView(1, self.offsets.GetGPUVirtualAddress());
            list.SetComputeRootUnorderedAccessView(2, self.scratch.GetGPUVirtualAddress());
            for args in &self.dispatches {
                list.SetComputeRoot32BitConstants(0, args.len() as u32, args.as_ptr().cast(), 0);
                list.Dispatch(args[0].div_ceil(32), args[1].div_ceil(4), 1);
            }
            list.ResourceBarrier(&[transition(&self.scratch, uav, common)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use victory_formats::texture::Desc;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT, DXGI_FORMAT_BC7_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
    };

    fn compare_uploads(format: DXGI_FORMAT, side: u32, mips: u16, swizzle: Swizzle, rounds: usize) {
        let host = crate::host::host().expect("D3D12 required");
        let (block, element_bytes) = if format == DXGI_FORMAT_BC7_UNORM {
            (4, 16)
        } else {
            (1, 4)
        };
        let layout = Layout::new(&Desc {
            dimension: Dimension::Tex2d,
            width: side,
            height: side,
            depth: 1,
            mips: u32::from(mips),
            element_bytes,
            block,
            samples: 1,
            swizzle,
        })
        .unwrap();
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: u64::from(side),
            Height: side,
            DepthOrArraySize: 1,
            MipLevels: mips,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        let mut footprints = vec![D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); mips as usize];
        let mut rows = vec![0u32; mips as usize];
        let mut total = 0;
        let mut texture: Option<ID3D12Resource> = None;
        // SAFETY: the footprint arrays cover the whole texture and the out pointer is ours.
        unsafe {
            host.device.GetCopyableFootprints(
                &desc,
                0,
                u32::from(mips),
                0,
                Some(footprints.as_mut_ptr()),
                Some(rows.as_mut_ptr()),
                None,
                Some(&mut total),
            );
            host.device
                .CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_DEFAULT,
                        ..Default::default()
                    },
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut texture,
                )
                .unwrap();
        }
        let texture = texture.unwrap();
        let gpu = GpuDetile::new(host, &layout, &footprints, &rows, total).unwrap();
        let (cpu, linear_at) = host.upload_buffer(total).unwrap();
        let back = host.read_back_buffer(total).unwrap();
        let tiled: Vec<u8> = (0..layout.size)
            .map(|i| ((i ^ (i >> 8) ^ (i >> 16)) & 255) as u8)
            .collect();
        let mut timings = [Vec::new(), Vec::new()];
        for trial in 0..=rounds {
            for accelerated in [false, true] {
                let begin = std::time::Instant::now();
                if accelerated {
                    gpu.fill(&tiled);
                } else {
                    // SAFETY: the previous submission finished before any writes.
                    let linear =
                        unsafe { std::slice::from_raw_parts_mut(linear_at, total as usize) };
                    for (i, (footprint, &height)) in footprints.iter().zip(&rows).enumerate() {
                        let pitch = footprint.Footprint.RowPitch as usize;
                        layout
                            .detile(
                                &tiled,
                                i as u32,
                                0,
                                &mut linear[footprint.Offset as usize..],
                                pitch,
                                pitch * height as usize,
                            )
                            .unwrap();
                    }
                }
                let filled = begin.elapsed();
                let list = host.new_list().unwrap();
                let l = &list.list;
                let (buffer, offset) = if accelerated {
                    gpu.record(l);
                    (&gpu.scratch, gpu.linear_base)
                } else {
                    (&cpu, 0)
                };
                let texture_part = |i: usize| D3D12_TEXTURE_COPY_LOCATION {
                    // SAFETY: these locations borrow live resources for recording.
                    pResource: unsafe { std::mem::transmute_copy(&texture) },
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: i as u32,
                    },
                };
                let buffer_part =
                    |resource: &ID3D12Resource, footprint| D3D12_TEXTURE_COPY_LOCATION {
                        // SAFETY: as above.
                        pResource: unsafe { std::mem::transmute_copy(resource) },
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                            PlacedFootprint: footprint,
                        },
                    };
                // SAFETY: every location uses the device's footprint, the scratch
                // offset is texture aligned, and the source is readable as a copy.
                unsafe {
                    for (i, footprint) in footprints.iter().enumerate() {
                        let mut source = *footprint;
                        source.Offset += offset;
                        l.CopyTextureRegion(
                            &texture_part(i),
                            0,
                            0,
                            0,
                            &buffer_part(buffer, source),
                            None,
                        );
                    }
                    if accelerated {
                        l.ResourceBarrier(&[transition(
                            &gpu.scratch,
                            D3D12_RESOURCE_STATE_COPY_SOURCE,
                            D3D12_RESOURCE_STATE_COMMON,
                        )]);
                    }
                    let done = begin.elapsed();
                    if trial != 0 {
                        // Time only the upload in repeated samples. The first
                        // iteration below verifies the resulting subresources.
                        list.close();
                        host.execute(&[&list]);
                        timings[usize::from(accelerated)].push((filled, begin.elapsed(), done));
                        continue;
                    }
                    let dest = D3D12_RESOURCE_STATE_COPY_DEST;
                    let source = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    l.ResourceBarrier(&[transition(&texture, dest, source)]);
                    for (i, &footprint) in footprints.iter().enumerate() {
                        l.CopyTextureRegion(
                            &buffer_part(&back, footprint),
                            0,
                            0,
                            0,
                            &texture_part(i),
                            None,
                        );
                    }
                    l.ResourceBarrier(&[transition(&texture, source, dest)]);
                    list.close();
                    host.execute(&[&list]);
                    let mut mapped = std::ptr::null_mut();
                    back.Map(0, None, Some(&mut mapped)).unwrap();
                    for (i, footprint) in footprints.iter().enumerate() {
                        let bytes = layout.mips[i].width as usize * element_bytes as usize;
                        for y in 0..rows[i] as usize {
                            let start = footprint.Offset as usize
                                + y * footprint.Footprint.RowPitch as usize;
                            let expected = std::slice::from_raw_parts(linear_at.add(start), bytes);
                            let actual =
                                std::slice::from_raw_parts(mapped.cast::<u8>().add(start), bytes);
                            assert!(
                                expected == actual,
                                "format {format:?}, mip {i}, row {y}, GPU {accelerated}"
                            );
                        }
                    }
                    back.Unmap(0, None);
                }
            }
        }
        if rounds != 0 {
            for (mode, measured) in ["CPU", "GPU"].into_iter().zip(&mut timings) {
                measured.sort_unstable_by_key(|sample| sample.1);
                let (fill, end, recorded) = measured[rounds / 2];
                eprintln!(
                    "{side}x{side}, {format:?}, {mode}: fill {fill:?}, recorded {recorded:?}, done {end:?}"
                );
            }
        }
    }

    #[test]
    fn gpu_detiling_matches_the_cpu_texture_upload() {
        compare_uploads(DXGI_FORMAT_R8G8B8A8_UNORM, 257, 9, Swizzle::SW_64KB_D, 0);
        compare_uploads(DXGI_FORMAT_BC7_UNORM, 516, 10, Swizzle::SW_64KB_D, 0);
    }

    #[test]
    #[ignore = "measures native end-to-end texture uploads"]
    fn bench_gpu_detiling_texture_uploads() {
        compare_uploads(DXGI_FORMAT_R8G8B8A8_UNORM, 2048, 12, Swizzle::SW_64KB_D, 11);
        compare_uploads(DXGI_FORMAT_BC7_UNORM, 4096, 13, Swizzle::SW_64KB_D, 11);
    }
}
