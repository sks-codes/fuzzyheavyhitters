use counttree::fss::dpf::DpfKey;
use counttree::data_structures::payload::RingVec;
use counttree::{u32_to_bits, bits_to_u32};

#[test]
fn test_ldcf_basic_functionality() {
    const N: usize = 1;
    let modulus = 256u128; // Power of 2

    let nbits = 5;
    let alpha = 18u32;
    let mut alpha_bits = u32_to_bits(nbits, alpha);
    alpha_bits.reverse(); // Reverse bits for correct order
    println!("Alpha bits (reversed): {:?}", alpha_bits);

    // Create payloads for left, middle, and right regions
    let payload_in = RingVec::<N>::new([1], modulus);   // x < alpha
    let payload_out = RingVec::<N>::new([99], modulus); // x > beta

    // Generate FSS keys
    let (key0, key1) = DpfKey::gen_DpfKey(
        &alpha_bits,
        &payload_in,
        &payload_out,
        modulus,
    );

    println!("Testing dpf [{}] with {} bits", alpha, nbits);
    println!("Payload: in={}, out={}", 1, 99);
    println!(" x | Expected | Key0 | Key1 | Diff | Correct");
    println!("---+----------+------+------+------+--------");

    for prefix_length in 1..(nbits+1) {
        for x in 0u32..((1<<prefix_length) as u32) {
            let mut x_bits = u32_to_bits(prefix_length, x);
            x_bits.reverse(); // Reverse bits for correct order
        
            let eval0 = key0.eval_dpf(&x_bits, modulus);
            let eval1 = key1.eval_dpf(&x_bits, modulus);

            let result = eval0 - eval1;

            let mut expected = 0;
            let alpha_prefix = alpha >> (nbits-prefix_length);
            if x == alpha_prefix {
                expected = 1;
            } else {
                expected = 99;
            }

            assert_eq!(result[0], expected,
                        "Failed for x_bits={:?}: expected {}, got {}", x_bits, expected, result[0]);
        }
    }
}
