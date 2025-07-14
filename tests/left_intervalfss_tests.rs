use counttree::fss::left_interval::LIntervalFSSKey;
use counttree::data_structures::payload::RingVec;
use counttree::{u32_to_bits, bits_to_u32};

#[test]
fn test_lintervalfss_basic_functionality() {
    const N: usize = 1;
    let nbits = 5;
    let modulus = 256u128; // Power of 2

    // Test interval [5, 10]
    let alpha = 18u32;
    let beta = 25u32;
    let mut alpha_bits = u32_to_bits(nbits, alpha);
    alpha_bits.reverse(); // Reverse bits for correct order
    let mut beta_bits = u32_to_bits(nbits, beta);
    beta_bits.reverse(); // Reverse bits for correct order

    println!("Alpha bits: {:?}, Beta bits: {:?}", alpha_bits, beta_bits);

    // Create payloads for left, middle, and right regions
    let payload_left = RingVec::<N>::new([1], modulus);   // x < alpha
    let payload_mid = RingVec::<N>::new([42], modulus);   // alpha <= x <= beta
    let payload_right = RingVec::<N>::new([99], modulus); // x > beta

    // Generate FSS keys
    let (key0, key1) = LIntervalFSSKey::gen_LIntervalFSSKey(
        &alpha_bits, 
        &beta_bits, 
        payload_left, 
        payload_mid, 
        payload_right, 
        modulus, 
    );

    println!("Testing interval [{}, {}] with {} bits", alpha, beta, nbits);
    println!("Payload: left={}, mid={}, right={}", 1, 42, 99);
    println!(" x | Expected | Key0 | Key1 | Diff | Correct");
    println!("---+----------+------+------+------+--------");

    for prefix_length in 1..(nbits+1) {
        for x in 0u32..((1<<prefix_length) as u32) {
            let mut x_bits = u32_to_bits(prefix_length, x);
            x_bits.reverse(); // Reverse bits for correct order
        
            let eval0 = key0.eval_lintervalFSS(&x_bits, modulus);
            let eval1 = key1.eval_lintervalFSS(&x_bits, modulus);
            println!();
        
            let result = eval0 - eval1;

            let mut expected = 0;
            let alpha_prefix = alpha >> (nbits-prefix_length);
            let beta_prefix = beta >> (nbits-prefix_length);
            if x < alpha_prefix {
                expected = 1;
            } else if x >= alpha_prefix && x < beta_prefix {
                expected = 42;
            } else {
                expected = 99;
            }

            assert_eq!(result[0], expected,
                        "Failed for x_bits={:?}: expected {}, got {}", x_bits, expected, result[0]);
        }
    }
}
