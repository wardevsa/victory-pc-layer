//! Disassembles raw GFX10 code from files (for example the code blocks
//! `VICTORY_PIPELINE_DUMP` writes): `cargo run -p victory-shader --example
//! disasm_raw -- [--wave64] <file>...`. Code is decoded from offset 0 as far
//! as control flow reaches.

fn main() {
    let mut wave64 = false;
    for arg in std::env::args().skip(1) {
        if arg == "--wave64" {
            wave64 = true;
            continue;
        }
        let bytes = match std::fs::read(&arg) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{arg}: {e}");
                continue;
            }
        };
        println!("== {arg} ({} bytes)", bytes.len());
        match victory_shader::gfx10::decode(&bytes) {
            Ok(insts) => {
                for i in &insts {
                    println!("  {:5x}: {}", i.offset, i.text(wave64));
                }
            }
            Err(e) => println!("  not decoded: {e}"),
        }
    }
}
