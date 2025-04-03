use counttree::ibDCF::{eval_str, ibDCFKey};
use counttree::{add_bitstrings, bits_to_u32, u32_to_bits, MSB_u32_to_bits};

#[test]
fn ibdcf_complete() {
    let nbits = 5;
    let alpha = u32_to_bits(nbits, 21);
    let (key0, key1) = ibDCFKey::gen_ibDCF(&alpha, false);

    for i in 0..(1 << nbits) {
        let alpha_eval = u32_to_bits(nbits, i);

        // println!("Alpha: {:?}, input: {:?}", alpha, alpha_eval);
        for j in 0..((nbits - 1) as usize) {
            if j < 2 {
                continue;
            }
            let eval0 = key0.eval_ibDCF(&alpha_eval[0..j].to_vec());
            let eval1 = key1.eval_ibDCF(&alpha_eval[0..j].to_vec());
            let alpha_prefix = bits_to_u32(&alpha[0..j]);
            let input_prefix = bits_to_u32(&alpha_eval[0..j]);
            if alpha_prefix <= input_prefix {
                // println!("1: alpha prefix - {:?}", alpha[0..j].to_vec());
                // println!("1: input prefix - {:?}", alpha_eval[0..j].to_vec());
                assert_eq!(
                    (eval0 ^ eval1) as u32,
                    1,
                    "[Level {:?}] Value incorrect at {:?}",
                    j,
                    alpha_eval
                );
            } else {
                // println!("2: alpha prefix - {:?}", alpha[0..j].to_vec());
                // println!("2: input prefix - {:?}", alpha_eval[0..j].to_vec());
                assert_eq!((eval0 ^ eval1) as u32, 0);
            }
        }
    }
}


#[test]
fn dcf_output_test() {
    let nbits = 4; // Test all 4-bit values (0-15)
    let alpha = 6;  // Test boundary
    let alpha_bits = MSB_u32_to_bits(nbits, alpha);

    // let (k0, k1) = ibDCFKey::gen_ibDCF(&alpha_bits, false);
    let r = &[false];
    let beta = add_bitstrings(alpha_bits.as_slice(), r);
    let x = ibDCFKey::gen_l_inf_ball(&alpha_bits,1);
    let (kl0, kr0) = x.0[0].clone();
    let (kl1, kr1) = x.1[0].clone();
    println!("DCF outputs for α={} ({}),  beta={} ({:?}):", alpha, alpha_bits.iter().map(|&b| if b { '1' } else { '0' }).collect::<String>(), bits_to_u32(beta.as_slice()), beta.clone().iter().map(|&b| if b { '1' } else { '0' }).collect::<String>());
    println!(" x | x_bits | Client | Server | Final");
    println!("---+--------+--------+--------+-------");

    for x in 0..(1 << nbits) {
        let x_bits = MSB_u32_to_bits(nbits, x);
        let evall0 = kl0.eval_ibDCF(&x_bits);
        let evall1 = kl1.eval_ibDCF(&x_bits);
        let evalr0 = kr0.eval_ibDCF(&x_bits);
        let evalr1 = kr1.eval_ibDCF(&x_bits);

        // let res0 = evall0 == evall1;
        // let res1 = evalr0 == evalr1;
        // let res = res0 ^ res1;

        // let res0 = evall0 ^ evalr0;
        // let res1 = evall1 ^ evalr1;
        // let res = res0 == res1;

        let res0 = vec![evall0, evalr0];
        let res1 = vec![evall1, evalr1];
        let res = res0 == res1;


        println!("{:2} | {:6?} | {:6} | {:6} | {:6} | {:6} | {:5} {}",
                 x,
                 x_bits.iter().map(|&b| if b { 1 } else { 0 }).collect::<Vec<_>>(),
                 evall0 as u8,
                 evall1 as u8,
                 evalr0 as u8,
                 evalr1 as u8,
                 res as u8,
                 if res != (x >= alpha && x <= bits_to_u32(beta.as_slice())) { "← WRONG" } else { "" });
    }
    assert!(false)
}

#[test]
fn test_incremental_evaluation() {
    let nbits = 4;
    let alpha = 5; // 0101 in binary (LSB-first)
    let alpha_bits = u32_to_bits(nbits, alpha);

    // Generate DCF keys
    let (k0, k1) = ibDCFKey::gen_ibDCF(&alpha_bits, false);

    println!("Testing incremental evaluation for α={:?}", alpha_bits);
    println!("Bit | Prefix | k0 | k1 | Combined | Expected");
    println!("----+--------+----+----+----------+---------");

    // Test all possible prefixes
    for len in 1..=nbits {
        let all_prefixes = all_bit_vectors(len);

        for prefix in all_prefixes {
            // Full evaluation for reference
            let full_bits = extend_to_length(&prefix, nbits as usize);
            let full_res0 = k0.eval_ibDCF(&full_bits);
            let full_res1 = k1.eval_ibDCF(&full_bits);
            let full_combined = full_res0 ^ full_res1;

            // Incremental evaluation
            let mut state0 = k0.eval_init();
            let mut state1 = k1.eval_init();
            let mut incremental_res0 = false;
            let mut incremental_res1 = false;

            for &bit in &prefix {
                let new_state0 = k0.eval_bit(&state0, bit);
                let new_state1 = k1.eval_bit(&state1, bit);
                incremental_res0 = new_state0.y_bit.clone();
                incremental_res1 = new_state1.y_bit.clone();
                state0 = new_state0;
                state1 = new_state1;
            }

            let incremental_combined = incremental_res0 ^ incremental_res1;
            let expected = bits_to_u32(&prefix) < (alpha >> (nbits - len));

            println!("{:2}  | {:?} | {} | {} | {} | {} {}",
                     len,
                     prefix,
                     incremental_res0 as u8,
                     incremental_res1 as u8,
                     incremental_combined as u8,
                     expected as u8,
                     if incremental_combined != expected { "← WRONG" } else { "" }
            );
            //
            // assert_eq!(incremental_combined, expected,
            //            "Failed at prefix {:?}", prefix);
            //
            // // Verify incremental matches full evaluation
            // assert_eq!(incremental_combined, full_combined,
            //            "Incremental doesn't match full evaluation for {:?}", prefix);

        }
    }
    assert!(false)
}


#[test]
fn test_incremental_interval_evaluation() {
    let nbits = 4;
    let alpha = 5; // 0101 in binary (LSB-first)
    let alpha_bits = u32_to_bits(nbits, alpha);

    // Generate interval keys
    let ((k0_left, k0_right), (k1_left, k1_right)) = ibDCFKey::gen_interval(&alpha_bits, &alpha_bits);

    println!("Testing incremental interval evaluation for α={:?}", alpha_bits);
    println!("Bit | Prefix | k0_left | k1_left | k0_right | k1_right | Combined | Expected");
    println!("----+--------+---------+---------+----------+----------+----------+---------");

    // Test all possible prefixes
    for len in 1..=nbits {
        let all_prefixes = all_bit_vectors(len);

        for prefix in all_prefixes {
            // Full evaluation for reference
            let full_bits = extend_to_length(&prefix, nbits as usize);
            let full_res0_left = k0_left.eval_ibDCF(&full_bits);
            let full_res0_right = k0_right.eval_ibDCF(&full_bits);
            let full_res1_left = k1_left.eval_ibDCF(&full_bits);
            let full_res1_right = k1_right.eval_ibDCF(&full_bits);
            let full_combined = (full_res0_left == full_res1_left) && (full_res0_right == full_res1_right);

            // Incremental evaluation
            let mut state0_left = k0_left.eval_init();
            let mut state0_right = k0_right.eval_init();
            let mut state1_left = k1_left.eval_init();
            let mut state1_right = k1_right.eval_init();
            let mut incremental_res0_left = false;
            let mut incremental_res1_left = false;
            let mut incremental_res0_right = false;
            let mut incremental_res1_right = false;

            for &bit in &prefix {
                let new_state0_left = k0_left.eval_bit(&state0_left, bit);
                let new_state1_left = k1_left.eval_bit(&state1_left, bit);
                let new_state0_right = k0_right.eval_bit(&state0_right, bit);
                let new_state1_right = k1_right.eval_bit(&state1_right, bit);
                // let stuff = eval_str(&vec![(k0_left.clone(), k0_right.clone())], &vec![(state0_left.clone(), state0_right.clone())], &vec![bit]);
                // let stuff2 = eval_str(&vec![(k1_left.clone(), k1_right.clone())], &vec![(state1_left.clone(), state1_right.clone())], &vec![bit]);
                //
                // let new_state0_left = stuff[0].0.clone();
                // let new_state1_left = stuff2[0].0.clone();
                // let new_state0_right = stuff[0].1.clone();
                // let new_state1_right = stuff2[0].1.clone();

                incremental_res0_left = new_state0_left.y_bit.clone() ^ new_state0_left.bit.clone();
                incremental_res1_left = new_state1_left.y_bit.clone() ^ new_state1_left.bit.clone();
                incremental_res0_right = new_state0_right.y_bit.clone() ^ new_state0_right.bit.clone();
                incremental_res1_right = new_state1_right.y_bit.clone() ^ new_state1_right.bit.clone();

                state0_left = new_state0_left;
                state1_left = new_state1_left;
                state0_right = new_state0_right;
                state1_right = new_state1_right;
            }

            let incremental_combined = (incremental_res0_left ^ incremental_res1_left) && (incremental_res0_right ^ incremental_res1_right);
            let expected = bits_to_u32(&prefix) < (alpha >> (nbits - len));

            println!("{:2}  | {:?} | {} | {} | {} | {} | {} | {} {}",
                     len,
                     prefix,
                     incremental_res0_left as u8,
                     incremental_res1_left as u8,
                     incremental_res0_right as u8,
                     incremental_res1_right as u8,
                     incremental_combined as u8,
                     expected as u8,
                     if incremental_combined != expected { "← WRONG" } else { "" }
            );

            // // Verify that incremental evaluation matches full evaluation
            // assert_eq!(incremental_combined, full_combined,
            //            "Incremental doesn't match full evaluation for {:?}", prefix);
        }
    }
    assert!(false)
}



// Helper function to generate all bit vectors of length n
fn all_bit_vectors(n: u8) -> Vec<Vec<bool>> {
    (0..(1 << n)).map(|i| u32_to_bits(n, i)).collect()
}

// Helper to extend prefix to full length with zeros
fn extend_to_length(bits: &[bool], len: usize) -> Vec<bool> {
    let mut extended = bits.to_vec();
    extended.resize(len, false);
    extended
}


#[test]
fn test_individual_dcfs() {
    let nbits = 5;

    // Test left-bound DCF (should be true when x < boundary)
    let boundary = 10;
    let boundary_bits = u32_to_bits(nbits, boundary);

    // Generate left-bound DCF (x < boundary)
    // let (left_key0, left_key1) = ibDCFKey::gen_ibDCF(&boundary_bits, true);
    //
    // // Test right-bound DCF (should be true when x > boundary)
    // let (right_key0, right_key1) = ibDCFKey::gen_ibDCF(&boundary_bits, false);
    let ((left_key0, right_key0), (left_key1, right_key1)) = ibDCFKey::gen_interval(&boundary_bits, &boundary_bits);

    // Test values
    let test_values = vec![8, 9, 10, 11, 12];

    println!("\nTesting left-bound DCF (x < {})", boundary);
    for &x in &test_values {
        let x_bits = u32_to_bits(nbits, x);
        let eval0 = left_key0.eval_ibDCF(&x_bits);
        let eval1 = left_key1.eval_ibDCF(&x_bits);
        let res = !(eval0 ^ eval1);
        println!("  {} < {}: {}", x, boundary, res);
        assert_eq!(res, x < boundary);
    }

    println!("\nTesting right-bound DCF (x > {})", boundary);
    for &x in &test_values {
        let x_bits = u32_to_bits(nbits, x);
        let eval0 = right_key0.eval_ibDCF(&x_bits);
        let eval1 = right_key1.eval_ibDCF(&x_bits);
        let res = !(eval0 ^ eval1);
        println!("  {} > {}: {}", x, boundary, res);
        assert_eq!(res, x > boundary);
    }
}


#[test]
fn interval_test() {
    let nbits = 5;  // For 5-bit numbers (0-31)

    // Test cases: (left, right, test_values_and_expected)
    let test_cases = vec![
        // Normal interval
        (5, 10, vec![(4, true), (5, false), (7, false), (10, false), (11, true)]),
        // Single-point interval
        (8, 8, vec![(7, true), (8, false), (9, true)]),
        // Full range
        (0, 31, vec![(0, false), (15, false), (31, false)]),
        // Edge cases
        (0, 0, vec![(0, false), (1, true)]),
        (31, 31, vec![(30, true), (31, false)]),
    ];

    for (left, right, tests) in test_cases {
        let left_bits = u32_to_bits(nbits, left);
        let right_bits = u32_to_bits(nbits, right);

        println!("\nTesting interval [{}, {}]", left, right);

        // Generate interval DCF pair
        let ((client_left, client_right), (server_left, server_right)) =
            ibDCFKey::gen_interval(&left_bits, &right_bits);

        for (test_val, expected) in tests {
            let test_bits = u32_to_bits(nbits, test_val);

            // Full evaluation function
            let evaluate = |key: &ibDCFKey, test: &[bool]| {
                let mut state = key.eval_init();
                for bit in test {
                    state = key.eval_bit(&state, *bit);
                }
                state.y_bit
            };

            // Evaluate all four DCFs
            let client_l = evaluate(&client_left, &test_bits);
            let client_r = evaluate(&client_right, &test_bits);
            let server_l = evaluate(&server_left, &test_bits);
            let server_r = evaluate(&server_right, &test_bits);

            // Combine shares (using XOR)
            let left_dcf = client_l == server_l;  // x >= left
            let right_dcf = client_r == server_r; // x <= right

            // Final interval check (AND of both conditions)
            let final_res = left_dcf ^ right_dcf;

            println!("  {}: {:?} (left: {}, right: {})",
                     test_val, final_res, left_dcf, right_dcf);

            assert_eq!(
                final_res, expected,
                "Failed at {} ∈ [{},{}]: expected {}, got {}",
                test_val, left, right, expected, final_res
            );
        }
    }
}