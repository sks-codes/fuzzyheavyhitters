use std::time::Instant;
use std::env;
use std::hint::black_box;

use counttree::data_structures::payload::RingVec; // existing static version
use counttree::data_structures::ringvec_new::DynRingVec; // new dynamic version

fn parse_arg<T: std::str::FromStr>(idx: usize, default: T) -> T { env::args().nth(idx).and_then(|s| s.parse().ok()).unwrap_or(default) }

fn bench_compare<const N: usize>(iterations: usize, modulus_bits: usize) {
    assert!(modulus_bits > 0 && modulus_bits <= 128, "modulus bits must be 1..=128");
    let modulus: u128 = 1u128 << modulus_bits;

    // Pre-build random vectors
    let vecs_a_static: Vec<RingVec<N>> = (0..iterations).map(|_| RingVec::<N>::random(modulus)).collect();
    let vecs_b_static: Vec<RingVec<N>> = (0..iterations).map(|_| RingVec::<N>::random(modulus)).collect();

    let vecs_a_dyn: Vec<DynRingVec> = (0..iterations).map(|_| DynRingVec::random(N, modulus)).collect();
    let vecs_b_dyn: Vec<DynRingVec> = (0..iterations).map(|_| DynRingVec::random(N, modulus)).collect();

    // --- Static RingVec ---
    let start_add = Instant::now(); let mut acc_add = RingVec::<N>::zero(modulus); for i in 0..iterations { acc_add = vecs_a_static[i] + vecs_b_static[i]; } let dur_add = start_add.elapsed(); black_box(&acc_add);
    let start_sub = Instant::now(); let mut acc_sub = RingVec::<N>::zero(modulus); for i in 0..iterations { acc_sub = vecs_a_static[i] - vecs_b_static[i]; } let dur_sub = start_sub.elapsed(); black_box(&acc_sub);
    let start_mul = Instant::now(); let mut acc_mul = RingVec::<N>::zero(modulus); for i in 0..iterations { acc_mul = vecs_a_static[i] * vecs_b_static[i]; } let dur_mul = start_mul.elapsed(); black_box(&acc_mul);

    // --- Dynamic DynRingVec ---
    let start_add_d = Instant::now(); let mut acc_add_d = DynRingVec::zero(N, modulus); for i in 0..iterations { acc_add_d = vecs_a_dyn[i].add(&vecs_b_dyn[i]); } let dur_add_d = start_add_d.elapsed(); black_box(&acc_add_d);
    let start_sub_d = Instant::now(); let mut acc_sub_d = DynRingVec::zero(N, modulus); for i in 0..iterations { acc_sub_d = vecs_a_dyn[i].sub(&vecs_b_dyn[i]); } let dur_sub_d = start_sub_d.elapsed(); black_box(&acc_sub_d);
    let start_mul_d = Instant::now(); let mut acc_mul_d = DynRingVec::zero(N, modulus); for i in 0..iterations { acc_mul_d = vecs_a_dyn[i].mul(&vecs_b_dyn[i]); } let dur_mul_d = start_mul_d.elapsed(); black_box(&acc_mul_d);

    println!("Compare RingVec(static array u128) vs DynRingVec(runtime width) N={} bits={} iterations={}", N, modulus_bits, iterations);
    println!(" static add {:?} (avg {:?}) | dyn add {:?} (avg {:?})", dur_add, dur_add / iterations as u32, dur_add_d, dur_add_d / iterations as u32);
    println!(" static sub {:?} (avg {:?}) | dyn sub {:?} (avg {:?})", dur_sub, dur_sub / iterations as u32, dur_sub_d, dur_sub_d / iterations as u32);
    println!(" static mul {:?} (avg {:?}) | dyn mul {:?} (avg {:?})", dur_mul, dur_mul / iterations as u32, dur_mul_d, dur_mul_d / iterations as u32);

    // Serialization cost comparison (single example vector)
    let bytes_static = vecs_a_static[0].to_bytes();
    let bytes_dyn = vecs_a_dyn[0].to_bytes();
    println!(" static serialized bytes: {} | dyn serialized bytes: {}", bytes_static.len(), bytes_dyn.len());
}

fn main() {
    let iterations = parse_arg(1, 50_000usize);
    let n_runtime = parse_arg(2, 8usize);
    let modulus_bits = parse_arg(3, 14usize); // e.g. 14 shows benefit (uses u16 internally)
    match n_runtime {
        1 => bench_compare::<1>(iterations, modulus_bits),
        2 => bench_compare::<2>(iterations, modulus_bits),
        4 => bench_compare::<4>(iterations, modulus_bits),
        8 => bench_compare::<8>(iterations, modulus_bits),
        16 => bench_compare::<16>(iterations, modulus_bits),
        32 => bench_compare::<32>(iterations, modulus_bits),
        other => { eprintln!("Unsupported N {} (choose 1,2,4,8,16,32)", other); std::process::exit(1); }
    }
}
