use counttree::fss::rdcf::RdcfKey;
use counttree::data_structures::payload::RingVec;
use counttree::{u32_to_bits, bits_to_u32};

#[test]
fn test_rdcf_basic_functionality() {
    const N: usize = 1;
    let nbits = 5;
    let modulus = 256u128; // Power of 2

    // Test interval [5, 10]
    let alpha = 18u32;
    let mut alpha_bits = u32_to_bits(nbits, alpha);
    alpha_bits.reverse(); // Reverse bits for correct order

    // Create payloads for left, middle, and right regions
    let payload_left = RingVec::<N>::new([1], modulus);   // x < alpha
    let payload_right = RingVec::<N>::new([99], modulus); // x > beta

    // Generate FSS keys
    let (key0, key1) = RdcfKey::gen_RdcfKey(
        &alpha_bits,
        &payload_left,
        &payload_right,
        modulus,
    );

    println!("Testing ldcf [{}] with {} bits", alpha, nbits);
    println!("Payload: left={}, right={}", 1, 99);
    println!(" x | Expected | Key0 | Key1 | Diff | Correct");
    println!("---+----------+------+------+------+--------");

    for prefix_length in 1..(nbits+1) {
        for x in 0u32..((1<<prefix_length) as u32) {
            let mut x_bits = u32_to_bits(prefix_length, x);
            x_bits.reverse(); // Reverse bits for correct order
        
            let eval0 = key0.eval_rdcf(&x_bits, modulus);
            let eval1 = key1.eval_rdcf(&x_bits, modulus);

            let result = eval0 - eval1;

            let mut expected = 0;
            let alpha_prefix = alpha >> (nbits-prefix_length);
            if x <= alpha_prefix {
                expected = 1;
            } else {
                expected = 99;
            }

            assert_eq!(result[0], expected,
                        "Failed for x_bits={:?}: expected {}, got {}", x_bits, expected, result[0]);
        }
    }
}
