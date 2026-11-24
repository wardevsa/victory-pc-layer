//! Translates the compute programs of device method 0x218 from the blocks
//! `VICTORY_PIPELINE_DUMP` wrote: `cargo run -p victory-shader --example
//! precompiled -- <dump folder>`. For each call `218-NN`, its description's
//! words 1-8 name the register block, an info block, the code and the
//! metadata block, dumped as `-w01`, `-w03`, `-w05` and `-w07`.

use std::path::Path;

fn main() {
    let dir = std::env::args().nth(1).expect("a dump folder");
    let dir = Path::new(&dir);
    // With `--features host`: each translation also becomes a D3D12 compute
    // pipeline on the default adapter, which validates its DXIL.
    #[cfg(all(windows, feature = "host"))]
    let host = victory_shader::emit::host::Host::new(false).expect("a D3D12 device");
    let (mut ok, mut failed) = (0, 0);
    for n in 1..=999 {
        let name = |w: &str| dir.join(format!("218-{n:02}-{w}.bin"));
        let (Ok(regs), Ok(code), Ok(meta)) = (
            std::fs::read(name("w01")),
            std::fs::read(name("w05")),
            std::fs::read(name("w07")),
        ) else {
            continue;
        };
        let result = victory_shader::native::precompiled_compute(&regs, &meta, code)
            .and_then(|p| {
                let summary = format!(
                    "wave{} threads {:?} {} layout, {} used",
                    if p.wave64 { 64 } else { 32 },
                    match &p.io {
                        victory_shader::native::StageIo::Cs(cs) => cs.threads,
                        _ => [0; 3],
                    },
                    p.layout.len(),
                    p.used.len()
                );
                let container = victory_shader::native::native_container(&p)?;
                // The hash victory-gpu logs and VICTORY_PRECOMPILED's hash
                // ranges and skip list go by.
                let hash = blake3::hash(&container).to_hex()[..16].to_owned();
                let summary = format!("hash {hash} {summary}");
                let shader = victory_shader::emit::translate(&container)?;
                Ok((summary, shader))
            })
            .map_err(|e| e.to_string())
            .and_then(|(summary, shader)| {
                #[cfg(all(windows, feature = "host"))]
                host.compute_pipeline(&shader)
                    .map_err(|e| format!("host pipeline refused: {e}"))?;
                Ok((summary, shader.dxil.len()))
            });
        match result {
            Ok((summary, dxil)) => {
                ok += 1;
                println!("218-{n:02}: translated ({summary}), {dxil} bytes of DXIL");
            }
            Err(e) => {
                failed += 1;
                println!("218-{n:02}: {e}");
            }
        }
    }
    println!("{ok} translated, {failed} failed");
}
