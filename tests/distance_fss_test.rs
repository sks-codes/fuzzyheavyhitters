use counttree::fss::distance::DistanceFSSKey;
use counttree::data_structures::payload::RingVec;
use counttree::util::{u128_to_bits, bits_to_u128, u128_to_bits_msb};

/// Compute u1111... (prefix u followed by all 1s)
fn compute_u1111(prefix: u128, prefix_len: usize, total_len: usize) -> u128 {
    let mut result = prefix;
    for _ in prefix_len..total_len {
        result = (result << 1) | 1;
    }
    result
}

/// Compute u0000... (prefix u followed by all 0s)
fn compute_u0000(prefix: u128, prefix_len: usize, total_len: usize) -> u128 {
    let mut result = prefix;
    for _ in prefix_len..total_len {
        result = result << 1;
    }
    result
}

/// Compare two prefixes of the same length
fn compare_prefixes(prefix1: u128, prefix2: u128, prefix_len: usize) -> std::cmp::Ordering {
    let mask = (1u128 << prefix_len) - 1;
    let p1 = prefix1 & mask;
    let p2 = prefix2 & mask;
    p1.cmp(&p2)
}

#[test]
fn test_distance_fss_brute_force() {
    const N: usize = 3; // Testing for P = 2 (squared distance)
    const MODULUS: u128 = 1 << 20; // 2^20
    const BIT_LENGTH: usize = 8; // 8-bit values for manageable test size
    const PREFIX_LENGTH: usize = 4; // Test with 4-bit prefixes
    const MAX_DISTANCE: u128 = 1000; // Maximum distance for out-of-range prefixes
    
    // Test with a specific x value
    let x = 85u128; // Binary: 01010101
    let x_bits = u128_to_bits_msb(x, BIT_LENGTH);
    let x_prefix = x >> (BIT_LENGTH - PREFIX_LENGTH);
    
    println!("Testing with x = {} (binary: {:08b})", x, x);
    println!("x_prefix = {} (binary: {:04b})", x_prefix, x_prefix);
    
    // Create left and right boundaries
    let left_value = 16u128; // 00010000 - setting a non-zero left boundary
    let right_value = 240u128; // 11110000 - setting a non-max right boundary
    let left_bits = u128_to_bits_msb(left_value, BIT_LENGTH);
    let right_bits = u128_to_bits_msb(right_value, BIT_LENGTH);
    let left_prefix = left_value >> (BIT_LENGTH - PREFIX_LENGTH);
    let right_prefix = right_value >> (BIT_LENGTH - PREFIX_LENGTH);
    
    println!("left_value = {} (binary: {:08b}), left_prefix = {} (binary: {:04b})", 
             left_value, left_value, left_prefix, left_prefix);
    println!("right_value = {} (binary: {:08b}), right_prefix = {} (binary: {:04b})", 
             right_value, right_value, right_prefix, right_prefix);
    
    // Generate distance FSS keys
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MAX_DISTANCE, MODULUS
    );
    
    println!("Generated FSS keys successfully");
    
    // Test all possible prefixes
    let mut test_count = 0;
    let mut success_count = 0;
    
    for prefix in 0..(1u128 << PREFIX_LENGTH) {
        // Create the prefix bits using util function with MSB ordering
        let prefix_bits = u128_to_bits_msb(prefix, PREFIX_LENGTH);
        println!("Testing prefix {}: bits: {:?}", prefix, prefix_bits);
        
        // Evaluate with both keys
        let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
        let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
        
        // Reconstruct the secret share
        let reconstructed = (eval0 + MODULUS - eval1) % MODULUS;
        
        // Compute the expected value based on prefix comparison with new logic
        let expected = if prefix < left_prefix || prefix > right_prefix {
            // prefix is outside the valid range: return max_distance
            println!("prefix {} is outside range [{}, {}]: returning max_distance={}", 
                     prefix, left_prefix, right_prefix, MAX_DISTANCE);
            MAX_DISTANCE
        } else {
            match compare_prefixes(prefix, x_prefix, PREFIX_LENGTH) {
                std::cmp::Ordering::Less => {
                    // prefix < x_prefix: return (x - u1111)^P
                    let u1111 = compute_u1111(prefix, PREFIX_LENGTH, BIT_LENGTH);
                    let diff = if x >= u1111 { x - u1111 } else { MODULUS - (u1111 - x) };
                    
                    // Compute diff^P where P = N-1 = 2
                    let result = (diff * diff) % MODULUS;
                    println!("prefix {} < x_prefix {}: u1111={}, x-u1111={}, (x-u1111)^2={}", 
                             prefix, x_prefix, u1111, diff, result);
                    result
                },
                std::cmp::Ordering::Equal => {
                    // prefix == x_prefix: return 0
                    println!("prefix {} == x_prefix {}: returning 0", prefix, x_prefix);
                    0
                },
                std::cmp::Ordering::Greater => {
                    // prefix > x_prefix: return (u0000 - x)^P
                    let u0000 = compute_u0000(prefix, PREFIX_LENGTH, BIT_LENGTH);
                    let diff = if u0000 >= x { u0000 - x } else { MODULUS - (x - u0000) };
                    
                    // Compute diff^P where P = N-1 = 2
                    let result = (diff * diff) % MODULUS;
                    println!("prefix {} > x_prefix {}: u0000={}, u0000-x={}, (u0000-x)^2={}", 
                             prefix, x_prefix, u0000, diff, result);
                    result
                }
            }
        };
        
        test_count += 1;
        
        if reconstructed == expected {
            success_count += 1;
        } else {
            println!("MISMATCH for prefix {}: expected {}, got {}", 
                     prefix, expected, reconstructed);
        }
    }
    
    println!("Test completed: {}/{} tests passed", success_count, test_count);
    assert_eq!(success_count, test_count, "Some distance FSS evaluations were incorrect");
}

#[test]
fn test_distance_fss_edge_cases() {
    const N: usize = 2; // Testing for P = 1 (absolute distance)
    const MODULUS: u128 = 1 << 16;
    const BIT_LENGTH: usize = 6;
    const PREFIX_LENGTH: usize = 3;
    const MAX_DISTANCE: u128 = 500; // Maximum distance for out-of-range prefixes
    
    // Test edge case: x = 0
    let x = 0u128;
    let x_bits = u128_to_bits_msb(x, BIT_LENGTH);
    let left_bits = u128_to_bits_msb(0, BIT_LENGTH);
    let right_bits = u128_to_bits_msb((1u128 << BIT_LENGTH) - 1, BIT_LENGTH);
    
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MAX_DISTANCE, MODULUS
    );
    
    // Test prefix 0 (should equal x)
    let prefix_bits = u128_to_bits_msb(0, PREFIX_LENGTH);
    let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let reconstructed = (eval0 + eval1) % MODULUS;
    
    assert_eq!(reconstructed, 0, "Distance should be 0 when prefix equals x");
    
    // Test edge case: x = maximum value
    let x = (1u128 << BIT_LENGTH) - 1; // All 1s
    let x_bits = u128_to_bits_msb(x, BIT_LENGTH);
    
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MAX_DISTANCE, MODULUS
    );
    
    // Test prefix that's all 1s (should equal x)
    let prefix = (1u128 << PREFIX_LENGTH) - 1;
    let prefix_bits = u128_to_bits_msb(prefix, PREFIX_LENGTH);
    let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let reconstructed = (eval0 + eval1) % MODULUS;
    
    assert_eq!(reconstructed, 0, "Distance should be 0 when prefix equals x (max case)");
}
