use mosaic::{
    data_structures::ringvec::RingVec,
    util::{u128_to_bits_msb, bits_to_u128_msb},
};
use std::time::Instant;
use std::env;
use std::hint::black_box;

fn parse_arg<T: std::str::FromStr>(idx: usize, default: T) -> T { env::args().nth(idx).and_then(|s| s.parse().ok()).unwrap_or(default) }

fn bench_for<const N: usize>(iterations: usize, modulus_bits: usize, bits_convert: bool) {
    assert!(modulus_bits > 0 && modulus_bits <= 64, "modulus bits must be 1..=64");
    let modulus: u128 = 1u128 << modulus_bits;

    // Pre-generate vectors to avoid timing RNG for every op inside tight loops.
    let mut vecs_a: Vec<RingVec<N>> = (0..iterations).map(|_| RingVec::<N>::random(modulus)).collect();
    let mut vecs_b: Vec<RingVec<N>> = (0..iterations).map(|_| RingVec::<N>::random(modulus)).collect();

    // Addition
    let start_add = Instant::now();
    let mut acc_add = RingVec::<N>::zero(modulus);
    for i in 0..iterations { acc_add = vecs_a[i] + vecs_b[i]; }
    let dur_add = start_add.elapsed();
    black_box(&acc_add);

    // Subtraction
    let start_sub = Instant::now();
    let mut acc_sub = RingVec::<N>::zero(modulus);
    for i in 0..iterations { acc_sub = vecs_a[i] - vecs_b[i]; }
    let dur_sub = start_sub.elapsed();
    black_box(&acc_sub);

    // Multiplication
    let start_mul = Instant::now();
    let mut acc_mul = RingVec::<N>::zero(modulus);
    for i in 0..iterations { acc_mul = vecs_a[i] * vecs_b[i]; }
    let dur_mul = start_mul.elapsed();
    black_box(&acc_mul);

    // Serialize (to_bytes)
    let start_ser = Instant::now();
    let mut ser_bytes_total: usize = 0;
    for v in &vecs_a { ser_bytes_total += v.to_bytes().len(); }
    let dur_ser = start_ser.elapsed();

    // Deserialize (from_bytes)
    let bytes_example = vecs_a[0].to_bytes();
    let start_de = Instant::now();
    let mut de_count = 0usize;
    for _ in 0..iterations { let (_v, _used) = RingVec::<N>::from_bytes(&bytes_example, modulus).unwrap(); de_count += 1; }
    let dur_de = start_de.elapsed();
    black_box(de_count);

    // Optional per-element bit conversion cost simulation
    let (dur_bits_out, dur_bits_in) = if bits_convert {
        // Convert each element of each RingVec to bits then back; measure separately
        let start_out = Instant::now();
        let bit_len = modulus_bits; // minimal bits
        let mut tmp_bits = Vec::with_capacity(bit_len);
        let mut tmp_u128 = 0u128;
        for v in &vecs_a { for &val in v.val() { tmp_bits = u128_to_bits_msb(val, bit_len); black_box(&tmp_bits); } }
        let dur_out = start_out.elapsed();

        let start_in = Instant::now();
        for v in &vecs_b { for &val in v.val() { tmp_u128 ^= bits_to_u128_msb(&u128_to_bits_msb(val, bit_len)); } }
        let dur_in = start_in.elapsed();
        black_box(tmp_u128);
        (Some(dur_out), Some(dur_in))
    } else { (None, None) };

    println!("RingVec<N={}> modulus_bits={} iterations={}", N, modulus_bits, iterations);
    println!(" add: {:?} (avg {:?})", dur_add, dur_add / iterations as u32);
    println!(" sub: {:?} (avg {:?})", dur_sub, dur_sub / iterations as u32);
    println!(" mul: {:?} (avg {:?})", dur_mul, dur_mul / iterations as u32);
    println!(" to_bytes: {:?} total ({} avg bytes serialized per vec)", dur_ser, ser_bytes_total as f64 / iterations as f64);
    println!(" from_bytes (same bytes reused): {:?} (avg {:?})", dur_de, dur_de / iterations as u32);
    if let (Some(o), Some(i)) = (dur_bits_out, dur_bits_in) { println!(" bit_out(total convert u128->bits): {:?}; bit_in(u128<-bits): {:?}", o, i); }
}

fn main() {
    // Args: iterations N modulus_bits bits_convert(0/1)
    let iterations = parse_arg(1, 100_000usize);
    let n_runtime = parse_arg(2, 4usize);
    let modulus_bits = parse_arg(3, 16usize);
    let bits_convert = parse_arg(4, 0usize) == 1;

    match n_runtime {
        1 => bench_for::<1>(iterations, modulus_bits, bits_convert),
        2 => bench_for::<2>(iterations, modulus_bits, bits_convert),
        4 => bench_for::<4>(iterations, modulus_bits, bits_convert),
        8 => bench_for::<8>(iterations, modulus_bits, bits_convert),
        16 => bench_for::<16>(iterations, modulus_bits, bits_convert),
        32 => bench_for::<32>(iterations, modulus_bits, bits_convert),
        other => {
            eprintln!("Unsupported N {} (choose one of 1,2,4,8,16,32)", other);
            std::process::exit(1);
        }
    }
}
