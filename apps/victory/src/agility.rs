//! The D3D12 Agility SDK: which `D3D12Core.dll` the D3D12 runtime loads.
//! Only with `--features agility-sdk`.
//!
//! Translated shaders use shader model 6.6 bindless heaps
//! (`CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED`), which Windows 10's own D3D12 does
//! not take: root signatures with those flags fail with `E_INVALIDARG`
//! (issue #69). With these two exports the runtime loads the newer
//! `D3D12Core.dll` from `D3D12\` beside `victory.exe`, which
//! `scripts/get-agility-sdk.ps1` puts there.
//!
//! Opt-in because with the exports and no DLL there, D3D12 does not fall back
//! to the system's runtime: every call fails with `D3D12_ERROR_INVALID_REDIST`
//! (0x887E0003). Under Wine and Proton, vkd3d-proton is the runtime and the
//! exports are not read.

/// The SDK version of the `D3D12Core.dll` that `scripts/get-agility-sdk.ps1`
/// fetches: the middle number of its package version (1.619.6). The two must
/// match, or the runtime does not load the DLL.
#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKVersion: u32 = 619;

/// A C string pointer, which the runtime reads from the export.
#[repr(transparent)]
pub struct SdkPath(*const u8);

// SAFETY: it points at a static string that nothing writes.
unsafe impl Sync for SdkPath {}

/// Where `D3D12Core.dll` is, relative to `victory.exe`.
#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKPath: SdkPath = SdkPath(c".\\D3D12\\".as_ptr().cast());

/// The linker directives that export the two (an executable exports nothing
/// unless told): read by the MSVC linker and lld-link from a `.drectve`
/// section, as `dllexport` does it.
#[unsafe(link_section = ".drectve")]
#[used]
static EXPORTS: [u8; 57] = *b" /EXPORT:D3D12SDKVersion,DATA /EXPORT:D3D12SDKPath,DATA  ";
