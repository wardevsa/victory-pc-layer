//! Run with `cargo run --release -p victory-formats --example texture_copy_bench`.
//! The elapsed time is for a complete mip chain, including its smallest tails.

use std::hint::black_box;
use std::time::Instant;
use victory_formats::texture::{Desc, Dimension, Layout, Swizzle};

fn measure(name: &str, width: u32, block: u32, bpe: u32, iterations: usize) {
    let height = width * 3 / 4 + 1;
    let layout = Layout::new(&Desc {
        dimension: Dimension::Tex2d,
        width,
        height,
        depth: 1,
        mips: width.ilog2() + 1,
        element_bytes: bpe,
        block,
        samples: 1,
        swizzle: Swizzle::SW_64KB_D,
    })
    .unwrap();
    let title: Vec<u8> = (0..layout.size as usize)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let pitches: Vec<_> = layout
        .mips
        .iter()
        .map(|m| (m.width as usize * bpe as usize).next_multiple_of(256))
        .collect();
    let mut linear: Vec<Vec<u8>> = layout
        .mips
        .iter()
        .zip(&pitches)
        .map(|(m, pitch)| vec![0x29; m.height as usize * pitch])
        .collect();
    let mut copy = || {
        for (i, (output, &pitch)) in linear.iter_mut().zip(&pitches).enumerate() {
            let plane = output.len();
            layout
                .detile(
                    black_box(&title),
                    i as u32,
                    0,
                    black_box(output),
                    pitch,
                    plane,
                )
                .unwrap();
        }
    };
    copy();
    let start = Instant::now();
    for _ in 0..iterations {
        copy();
    }
    let elapsed = start.elapsed().as_secs_f64() * 1e6 / iterations as f64;
    let hash = linear.iter().flatten().fold(0xcbf29ce484222325u64, |h, v| {
        (h ^ u64::from(*v)).wrapping_mul(1099511628211)
    });
    println!("{name} {elapsed:.3} {hash:016x}");
}

fn main() {
    measure("BC7-64", 64, 4, 16, 10000);
    measure("BC7-256", 256, 4, 16, 1200);
    measure("BC7-1024", 1024, 4, 16, 140);
    measure("RGBA-257", 257, 1, 4, 600);
}
