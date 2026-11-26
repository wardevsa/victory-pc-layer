// Exports the two symbols the DirectX 12 Agility SDK looks for, so the D3D12
// runtime loads `D3D12Core.dll` from the `D3D12` folder beside the executable
// instead of the system copy. Rust's bin crates carry no export table, so the
// linker is told explicitly; `#[unsafe(no_mangle)]` in `main.rs` keeps the
// names unmangled.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKPath,DATA");
    }
}
