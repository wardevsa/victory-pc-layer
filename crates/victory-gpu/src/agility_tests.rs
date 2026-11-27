//! For the test programs: the D3D12 Agility SDK exports `victory.exe` has
//! (`apps/victory/src/agility.rs`, which says why). The linker is told to
//! export them by a directive in this object ([`EXPORTS`]), as `dllexport` does.
//! With `scripts/get-agility-sdk.ps1` run, `D3D12Core.dll` is in `D3D12\`
//! beside the test executables, and on Windows 10 the GPU tests find a shader
//! model 6.6 device (issue #74). Keep the version equal to `victory.exe`'s.

#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKVersion: u32 = 619;

/// A C string pointer, which the runtime reads from the export.
#[repr(transparent)]
pub struct SdkPath(*const u8);

// SAFETY: it points at a static string that nothing writes.
unsafe impl Sync for SdkPath {}

#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKPath: SdkPath = SdkPath(c".\\D3D12\\".as_ptr().cast());

/// The linker directives that export the two (an executable exports nothing
/// unless told): read by the MSVC linker and lld-link from a `.drectve` section.
#[unsafe(link_section = ".drectve")]
#[used]
static EXPORTS: [u8; 57] = *b" /EXPORT:D3D12SDKVersion,DATA /EXPORT:D3D12SDKPath,DATA  ";
