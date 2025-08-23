use std::env;
use std::time::Instant;
use std::hint::black_box;

use counttree::fss::ldcf::{LdcfKey, LdcfEval};
use counttree::data_structures::payload::RingVec;

fn parse_arg<T: std::str::FromStr>(idx: usize, default: T) -> T { env::args().nth(idx).and_then(|s| s.parse().ok()).unwrap_or(default) }

// Simple harness: generate random inputs, build keys once, then repeatedly call expand_prefix.
fn bench<const N: usize>(iterations: usize, depth: usize, modulus_bits: usize) {
    assert!(depth > 0, "depth must be > 0");
    assert!(modulus_bits > 0 && modulus_bits <= 64, "modulus bits 1..=64 supported");
    let modulus: u128 = 1u128 << modulus_bits;

    // Random alpha prefix bits
    let alpha_bits: Vec<bool> = (0..depth).map(|_| rand::random::<bool>()).collect();
    // Random payload vectors a, b
    let a = RingVec::<N>::random(modulus);
    let b = RingVec::<N>::random(modulus);

    let (k0, _k1) = LdcfKey::<N>::gen_LdcfKey(&alpha_bits, &a, &b, modulus);
    // We'll just benchmark key 0.
    let mut state = k0.eval_init(modulus);

    // Walk until just before last level if iterations wants deeper loops; we cycle.
    // We'll repeatedly call expand_prefix on a state whose level < depth.
    // Reset to initial when reaching depth to avoid out-of-bounds.
    let start = Instant::now();
    let mut calls = 0usize;
    for _ in 0..iterations {
        let (l, _r) = k0.expand_prefix(&state, modulus); // we only need one branch to advance
        state = l; // choose left path consistently
        calls += 1;
    }
    let dur = start.elapsed();
    println!("LDCF expand_prefix bench: N={} depth={} modulus_bits={} iterations={}", N, depth, modulus_bits, iterations);
    println!(" total: {:?}; avg: {:?}", dur, dur / calls as u32);
    black_box(state);
}

fn main() {
    // Args: iterations N depth modulus_bits
    let iterations = parse_arg(1, 15usize);
    let n_runtime = parse_arg(2, 8usize);
    let depth = parse_arg(3, 16usize);
    let modulus_bits = parse_arg(4, 16usize);
    match n_runtime {
        1 => bench::<1>(iterations, depth, modulus_bits),
        2 => bench::<2>(iterations, depth, modulus_bits),
        4 => bench::<4>(iterations, depth, modulus_bits),
        8 => bench::<8>(iterations, depth, modulus_bits),
        16 => bench::<16>(iterations, depth, modulus_bits),
        32 => bench::<32>(iterations, depth, modulus_bits),
        other => { eprintln!("Unsupported N {} (choose 1,2,4,8,16,32)", other); std::process::exit(1); }
    }
}
