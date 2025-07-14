use counttree::fss::distance::DistanceFSSKey;
use counttree::data_structures::payload::RingVec;

/// Convert an integer to a bit vector of specified length
fn int_to_bits(value: u128, len: usize) -> Vec<bool> {
    let mut bits = Vec::new();
    for i in (0..len).rev() {
        bits.push((value >> i) & 1 == 1);
    }
    bits
}

/// Convert a bit vector to an integer
fn bits_to_int(bits: &[bool]) -> u128 {
    let mut result = 0u128;
    for &bit in bits {
        result = (result << 1) | (if bit { 1 } else { 0 });
    }
    result
}

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
    
    // Test with a specific x value
    let x = 85u128; // Binary: 01010101
    let x_bits = int_to_bits(x, BIT_LENGTH);
    let x_prefix = x >> (BIT_LENGTH - PREFIX_LENGTH);
    
    println!("Testing with x = {} (binary: {:08b})", x, x);
    println!("x_prefix = {} (binary: {:04b})", x_prefix, x_prefix);
    
    // Create left and right boundaries (we'll test all possible prefixes)
    let left_value = 0u128;
    let right_value = (1u128 << BIT_LENGTH) - 1; // All 1s
    let left_bits = int_to_bits(left_value, BIT_LENGTH);
    let right_bits = int_to_bits(right_value, BIT_LENGTH);
    
    // Generate distance FSS keys
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MODULUS
    );
    
    println!("Generated FSS keys successfully");
    
    // Test all possible prefixes
    let mut test_count = 0;
    let mut success_count = 0;
    
    for prefix in 0..(1u128 << PREFIX_LENGTH) {
        // Create the prefix bits
        let prefix_bits = int_to_bits(prefix, PREFIX_LENGTH);
        println!("Prefix bits: {:?}", prefix_bits);
        
        // Evaluate with both keys
        let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
        let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
        
        // Reconstruct the secret share
        let reconstructed = (eval0 + MODULUS - eval1) % MODULUS;
        
        // Compute the expected value based on prefix comparison
        let expected = match compare_prefixes(prefix, x_prefix, PREFIX_LENGTH) {
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
    
    // Test edge case: x = 0
    let x = 0u128;
    let x_bits = int_to_bits(x, BIT_LENGTH);
    let left_bits = int_to_bits(0, BIT_LENGTH);
    let right_bits = int_to_bits((1u128 << BIT_LENGTH) - 1, BIT_LENGTH);
    
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MODULUS
    );
    
    // Test prefix 0 (should equal x)
    let prefix_bits = int_to_bits(0, PREFIX_LENGTH);
    let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let reconstructed = (eval0 + eval1) % MODULUS;
    
    assert_eq!(reconstructed, 0, "Distance should be 0 when prefix equals x");
    
    // Test edge case: x = maximum value
    let x = (1u128 << BIT_LENGTH) - 1; // All 1s
    let x_bits = int_to_bits(x, BIT_LENGTH);
    
    let (key0, key1) = DistanceFSSKey::<N>::gen_distance_fss_key(
        x, &x_bits, &left_bits, &right_bits, MODULUS
    );
    
    // Test prefix that's all 1s (should equal x)
    let prefix = (1u128 << PREFIX_LENGTH) - 1;
    let prefix_bits = int_to_bits(prefix, PREFIX_LENGTH);
    let eval0 = key0.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let eval1 = key1.eval_distance_fss(&prefix_bits, BIT_LENGTH, MODULUS);
    let reconstructed = (eval0 + eval1) % MODULUS;
    
    assert_eq!(reconstructed, 0, "Distance should be 0 when prefix equals x (max case)");
}
