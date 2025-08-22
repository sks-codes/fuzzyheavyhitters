use std::time::Instant;
use std::env;
use std::hint::black_box;

use counttree::util::{u128_to_bits_msb, bits_to_u128_msb};

fn parse_arg_usize(idx: usize, default: usize) -> usize {
    env::args().nth(idx).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn main() {
    // Args: <iterations> <bit_length>
    let iterations = parse_arg_usize(1, 1_000_000);
    let bit_length = parse_arg_usize(2, 64); // must be <= 128
    assert!(bit_length <= 128, "bit_length must be <= 128");

    println!("Running {} iterations with bit_length {}", iterations, bit_length);

    // Benchmark bits_to_u128_msb
    let mut bits = vec![false; bit_length];
    let mut acc_u128 = 0u128;
    let start_bits_to = Instant::now();
    for i in 0..iterations {
        // Flip one bit to avoid constant-folding; distribution not important.
        let pos = i % bit_length;
        bits[pos] = !bits[pos];
        let v = bits_to_u128_msb(&bits);
        // Mix into accumulator to keep dependency.
        acc_u128 ^= v.rotate_left((i & 63) as u32);
    }
    let dur_bits_to = start_bits_to.elapsed();
    // Prevent optimizer deleting loop.
    black_box(acc_u128);

    println!("bits_to_u128_msb: {:?} total (avg {:?} per call)", dur_bits_to, dur_bits_to / iterations as u32);

    // Benchmark u128_to_bits_msb
    let mut value: u128 = 0x1234_5678_9ABC_DEF0_0FED_CBA9_8765_4321u128;
    let mut parity_acc = 0u64;
    let start_u128_to = Instant::now();
    for i in 0..iterations {
        // Vary the value in a reversible, cheap way.
        value = value.wrapping_add(0x9E37_79B9_7F4A_7C15_6A09_E667_F3BC_C909u128);
        let bits_vec = u128_to_bits_msb(value, bit_length);
        // Accumulate parity of first word of bits to force use.
        parity_acc ^= bits_vec.iter().fold(0u64, |acc, &b| acc ^ b as u64);
        // black_box inside loop would slow timing; rely on using result.
    }
    let dur_u128_to = start_u128_to.elapsed();
    black_box(parity_acc);

    println!("u128_to_bits_msb: {:?} total (avg {:?} per call)", dur_u128_to, dur_u128_to / iterations as u32);

    println!("Final accumulators: acc_u128 = {acc_u128:#034x}, parity_acc = {parity_acc}");
}
