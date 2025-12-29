use mosaic::{data_structures::ringvec::RingVec, fss::ldcf::LdcfKey};
use std::env;
use std::hint::black_box;
use std::time::Instant;

fn parse_arg<T: std::str::FromStr>(idx: usize, default: T) -> T {
    env::args()
        .nth(idx)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

// Simple harness: generate random inputs, build keys once, then repeatedly call expand_prefix.
fn bench(iterations: usize, depth: usize, modulus_bits: usize, payload_len: usize) -> anyhow::Result<()> {
    assert!(depth > 0, "depth must be > 0");
    assert!(
        modulus_bits > 0 && modulus_bits <= 64,
        "modulus bits 1..=64 supported"
    );
    let modulus: u128 = 1u128 << modulus_bits;

    // Random alpha prefix bits
    let alpha_bits: Vec<bool> = (0..depth).map(|_| rand::random::<bool>()).collect();
    // Random payload vectors a, b
    let a = RingVec::random_with_len(payload_len, modulus).expect("Failed to generate payload a");
    let b = RingVec::random_with_len(payload_len, modulus).expect("Failed to generate payload b");

    let (k0, _k1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, modulus)?;
    // We'll just benchmark key 0.
    let mut state = k0.init_eval(modulus)?;

    // Walk until just before last level if iterations wants deeper loops; we cycle.
    // We'll repeatedly call expand_prefix on a state whose level < depth.
    // Reset to initial when reaching depth to avoid out-of-bounds.
    let start = Instant::now();
    let mut calls = 0usize;
    for _ in 0..iterations {
        let (l, _r) = k0.expand_prefix(&state, modulus)?; // we only need one branch to advance
        state = l; // choose left path consistently
        calls += 1;
    }
    let dur = start.elapsed();
    println!(
        "LDCF expand_prefix bench: payload_len={} depth={} modulus_bits={} iterations={}",
        payload_len, depth, modulus_bits, iterations
    );
    println!(" total: {:?}; avg: {:?}", dur, dur / calls as u32);
    black_box(state);
    Ok(())
}

fn main() {
    // Args: iterations N depth modulus_bits
    let iterations = parse_arg(1, 15usize);
    let payload_len = parse_arg(2, 8usize);
    let depth = parse_arg(3, 16usize);
    let modulus_bits = parse_arg(4, 16usize);
    if let Err(e) = bench(iterations, depth, modulus_bits, payload_len) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
