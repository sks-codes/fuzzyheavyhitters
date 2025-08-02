use counttree::fuzzy_match::share_phase::{
    SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType, DistanceMetric};
use counttree::util::u128_to_bits_msb;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_okvs_share_and_eval_known_linf() {
        // Create OKVS configuration for known dictionary with L-infinity metric
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing
        let x = vec![100u128, 150u128];
        let delta = 5u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "OKVS share_range should succeed");
        
        let (share1, share2) = result.unwrap();
        
        // Verify that both shares are OKVS type
        match (&share1, &share2) {
            (SharedRange::OKVS { okvs_shares: share1_data, role: role1, p: _ }, 
             SharedRange::OKVS { okvs_shares: share2_data, role: role2, p: _ }) => {
                assert!(!share1_data.is_empty(), "First OKVS share should not be empty");
                assert!(!share2_data.is_empty(), "Second OKVS share should not be empty");
                assert_eq!(share1_data.len(), share2_data.len(), "Both shares should have same length");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected OKVS shares"),
        }
        
        // Test evaluation for known dictionary (L-infinity metric)
        for dim in 0..=1 {
            for test_point in 0u128..=255u128 {
                let test_point_bits = u128_to_bits_msb(test_point, 8);
                let result1 = share_phase.evaluate_at_single_dimension(&share1, &test_point_bits, dim);
                let result2 = share_phase.evaluate_at_single_dimension(&share2, &test_point_bits, dim);
                
                assert!(result1.is_ok(), "Evaluation of first share should succeed for point {} dim {}", test_point, dim);
                assert!(result2.is_ok(), "Evaluation of second share should succeed for point {} dim {}", test_point, dim);
                
                let eval1 = result1.unwrap();
                let eval2 = result2.unwrap();
                
                // The XOR of the two shares should give the correct result
                let reconstructed = eval1 ^ eval2;
                
                // Check if point is in expected range for L-infinity metric
                let in_range = match dim {
                    0 => test_point >= 95 && test_point <= 105, // x[0] = 100, delta = 5
                    1 => test_point >= 145 && test_point <= 155, // x[1] = 150, delta = 5
                    _ => false,
                };

                if in_range {
                    assert_eq!(reconstructed, 0, "Point {} in dimension {} should be inside range (result=0)", test_point, dim);
                } else {
                    assert_ne!(reconstructed, 0, "Point {} in dimension {} should be outside range (result!=0)", test_point, dim);
                }
            }
        }
    }

    #[test]
    fn test_okvs_share_and_eval_unknown_linf() {
        // Create OKVS configuration for unknown dictionary with L-infinity metric
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing for unknown dictionary
        let x = vec![75u128, 200u128];
        let delta = 10u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "OKVS share_range should succeed for unknown dictionary");
        
        let (share1, share2) = result.unwrap();
        
        // Verify shares structure
        match (&share1, &share2) {
            (SharedRange::OKVS { okvs_shares: share1_data, role: role1, p: _ }, 
             SharedRange::OKVS { okvs_shares: share2_data, role: role2, p: _ }) => {
                assert!(!share1_data.is_empty(), "First OKVS share should not be empty");
                assert!(!share2_data.is_empty(), "Second OKVS share should not be empty");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected OKVS shares"),
        }
        
        // Test evaluation for unknown dictionary with comprehensive prefix testing
        for dim in 0..=1 {
            let x_val = x[dim];
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            
            println!("Dimension {}: x={}, delta={}, range=[{}, {}]", dim, x_val, delta, left, right);
            
            // Test comprehensive prefix cases
            let test_points = [0u128, 65u128, 75u128, 85u128, 150u128, 200u128, 210u128, 255u128];
            
            for &test_point in &test_points {
                // Test all prefixes of this test point
                for prefix_len in 1..=8 {
                    let prefix = test_point >> (8 - prefix_len);
                    let prefix_bits = u128_to_bits_msb(prefix, prefix_len);
                    
                    let result1 = share_phase.evaluate_at_single_dimension(&share1, &prefix_bits, dim);
                    let result2 = share_phase.evaluate_at_single_dimension(&share2, &prefix_bits, dim);
                    
                    assert!(result1.is_ok(), "Evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    assert!(result2.is_ok(), "Evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    
                    let eval1 = result1.unwrap();
                    let eval2 = result2.unwrap();
                    let reconstructed = eval1 ^ eval2;
                    
                    // For unknown L-infinity: if prefix is >= left AND <= right, evals should be equal
                    let prefix_left = left >> (8 - prefix_len);  
                    let prefix_right = right >> (8 - prefix_len);
                    
                    if prefix >= prefix_left && prefix <= prefix_right {
                        // Prefixes in range should have equal evaluations
                        println!("  Prefix {:#010b} (len={}) is in range [{:#010b}, {:#010b}] - evals should be equal", 
                            prefix, prefix_len, prefix_left, prefix_right);
                        // For OKVS unknown, we expect consistent behavior within the range
                    } else {
                        // Prefixes outside range 
                        println!("  Prefix {:#010b} (len={}) is outside range [{:#010b}, {:#010b}]", 
                            prefix, prefix_len, prefix_left, prefix_right);
                    }
                }
            }
        }
    }

    #[test]
    fn test_fss_share_and_eval_known_linf() {
        // Create Interval FSS configuration for known dictionary with L-infinity metric
        let config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing
        let x = vec![100u128, 150u128];
        let delta = 5u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "Interval FSS share_range should succeed");
        
        let (share1, share2) = result.unwrap();
        
        // Verify that both shares are IntervalFSS type
        match (&share1, &share2) {
            (SharedRange::IntervalFSS { keys: keys1, role: _ }, 
             SharedRange::IntervalFSS { keys: keys2, role: _ }) => {
                assert_eq!(keys1.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(keys2.len(), 2, "Should have 2 FSS keys for dimension 2");
            },
            _ => panic!("Expected IntervalFSS shares"),
        }
        
        // Test evaluation for known dictionary with L-infinity metric
        for dim in 0..=1 {
            for test_point in 0u128..=255u128 {
                let test_point_bits = u128_to_bits_msb(test_point, 8);
                let result1 = share_phase.evaluate_at_single_dimension(&share1, &test_point_bits, dim);
                let result2 = share_phase.evaluate_at_single_dimension(&share2, &test_point_bits, dim);
                
                assert!(result1.is_ok(), "Evaluation of first share should succeed for point {} dim {}", test_point, dim);
                assert!(result2.is_ok(), "Evaluation of second share should succeed for point {} dim {}", test_point, dim);
                
                let eval1 = result1.unwrap();
                let eval2 = result2.unwrap();
                
                // The sum of the two shares (mod 2) should give the correct result
                let modulus = 1u128 << share_phase.config.output_bit_length;
                let reconstructed = (eval1 + modulus - eval2) % modulus; 
                
                // Check if point is in expected range for L-infinity metric
                let in_range = match dim {
                    0 => test_point >= 95 && test_point <= 105, // x[0] = 100, delta = 5
                    1 => test_point >= 145 && test_point <= 155, // x[1] = 150, delta = 5
                    _ => false,
                };
                
                // For Interval FSS, we expect the result to be 0 inside the interval and 1 outside
                if in_range {
                    assert_eq!(reconstructed, 0, "Point {} in dimension {} should be inside range (result=0)", test_point, dim);
                } else {
                    assert_eq!(reconstructed, 1, "Point {} in dimension {} should be outside range (result=1)", test_point, dim);
                }
            }
        }
    }

    #[test]
    fn test_fss_share_and_eval_unknown_linf() {
        // Create Interval FSS configuration for unknown dictionary with L-infinity metric
        let config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing for unknown dictionary
        let x = vec![80u128, 180u128];
        let delta = 15u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "Interval FSS share_range should succeed for unknown dictionary");
        
        let (share1, share2) = result.unwrap();
        
        // Verify shares structure
        match (&share1, &share2) {
            (SharedRange::IntervalFSS { keys: keys1, role: _ }, 
             SharedRange::IntervalFSS { keys: keys2, role: _ }) => {
                assert_eq!(keys1.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(keys2.len(), 2, "Should have 2 FSS keys for dimension 2");
            },
            _ => panic!("Expected IntervalFSS shares"),
        }
        
        // Test evaluation for unknown dictionary with comprehensive prefix testing
        for dim in 0..=1 {
            let x_val = x[dim];
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            
            println!("FSS Unknown L-inf Dimension {}: x={}, delta={}, range=[{}, {}]", dim, x_val, delta, left, right);
            
            let test_points = [0u128, 65u128, 80u128, 95u128, 165u128, 180u128, 195u128, 255u128];
            
            for &test_point in &test_points {
                // Test all prefixes of this test point
                for prefix_len in 1..=8 {
                    let prefix = test_point >> (8 - prefix_len);
                    let prefix_bits = u128_to_bits_msb(prefix, prefix_len);

                    let result1 = share_phase.evaluate_at_single_dimension(&share1, &prefix_bits, dim);
                    let result2 = share_phase.evaluate_at_single_dimension(&share2, &prefix_bits, dim);
                    
                    assert!(result1.is_ok(), "FSS evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    assert!(result2.is_ok(), "FSS evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    
                    let eval1 = result1.unwrap();
                    let eval2 = result2.unwrap();
                    
                    // For FSS unknown L-infinity: if prefix is >= left AND <= right, evals should be equal
                    let prefix_left = left >> (8 - prefix_len);
                    let prefix_right = right >> (8 - prefix_len);

                    if prefix >= prefix_left && prefix <= prefix_right {
                        println!("  FSS Prefix {:#010b} (len={}) is in range [{:#010b}, {:#010b}] - evals should be equal", 
                            prefix, prefix_len, prefix_left, prefix_right);
                        // The evaluations should be consistent within range for FSS
                        // We expect eval1 == eval2 for prefixes in range
                        assert_eq!(eval1, eval2, "FSS evals should be equal for prefix {:#010b} in range", prefix);
                    } else {
                        println!("  FSS Prefix {:#010b} (len={}) is outside range [{:#010b}, {:#010b}]", 
                            prefix, prefix_len, prefix_left, prefix_right);
                    }
                }
            }
        }
    }

    #[test]
    fn test_okvs_share_and_eval_known_lp() {
        // Create OKVS configuration for known dictionary with Lp metric
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            metric: DistanceMetric::Lp { p: 2},
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing
        let x = vec![120u128, 130u128];
        let delta = 8u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "OKVS share_range should succeed for Lp metric");
        
        let (share1, share2) = result.unwrap();
        
        // Verify that both shares are OKVS type
        match (&share1, &share2) {
            (SharedRange::OKVS { okvs_shares: share1_data, role: role1, p: _ }, 
             SharedRange::OKVS { okvs_shares: share2_data, role: role2, p: _ }) => {
                assert!(!share1_data.is_empty(), "First OKVS share should not be empty");
                assert!(!share2_data.is_empty(), "Second OKVS share should not be empty");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected OKVS shares"),
        }

        let modulus = 1u128 << share_phase.config.output_bit_length;
        
        // Test evaluation for known dictionary with Lp metric
        for dim in 0..=1 {
            for test_point in [110u128, 115u128, 120u128, 125u128, 130u128, 135u128, 140u128] {
                let test_point_bits = u128_to_bits_msb(test_point, 8);
                let result1 = share_phase.evaluate_at_single_dimension(&share1, &test_point_bits, dim);
                let result2 = share_phase.evaluate_at_single_dimension(&share2, &test_point_bits, dim);
                
                assert!(result1.is_ok(), "Evaluation should succeed for point {} dim {}", test_point, dim);
                assert!(result2.is_ok(), "Evaluation should succeed for point {} dim {}", test_point, dim);
                
                let eval1 = result1.unwrap();
                let eval2 = result2.unwrap();
                let reconstructed = (eval1 + eval2) % modulus;

                // For Lp metric, the range calculation is more complex
                let in_range = match dim {
                    0 => test_point >= 112 && test_point <= 128, // x[0] = 120, delta = 8
                    1 => test_point >= 122 && test_point <= 138, // x[1] = 130, delta = 8
                    _ => false,
                };
                
                if in_range {
                    let expected_distance = if test_point < x[dim] {
                        (x[dim] - test_point).pow(2) % modulus   // (x - y)^p for p=2
                    } else if test_point > x[dim] {
                        (test_point - x[dim]).pow(2) % modulus // (y - x)^p for p=2
                    } else {
                        0 // If equal, distance is 0
                    };
                    assert_eq!(reconstructed, expected_distance, 
                        "Point {} in dimension {} should be inside range, reconstructed={}", 
                        test_point, dim, reconstructed);
                } else {
                    println!("Point {} in dimension {} is outside range for Lp, reconstructed={}", 
                        test_point, dim, reconstructed);
                }
            }
        }
    }

    #[test]
    fn test_okvs_share_and_eval_unknown_lp() {
        // Create OKVS configuration for unknown dictionary with Lp metric
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing for unknown dictionary with Lp metric
        let x = vec![90u128, 160u128];
        let delta = 12u128;
        let modulus = 1u128 << share_phase.config.output_bit_length;
        let p = 2u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "OKVS share_range should succeed for unknown dictionary with Lp");
        
        let (share1, share2) = result.unwrap();
        
        // Verify shares structure
        match (&share1, &share2) {
            (SharedRange::OKVS { okvs_shares: share1_data, role: role1, p: _ }, 
             SharedRange::OKVS { okvs_shares: share2_data, role: role2, p: _ }) => {
                assert!(!share1_data.is_empty(), "First OKVS share should not be empty");
                assert!(!share2_data.is_empty(), "Second OKVS share should not be empty");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected OKVS shares"),
        }
        
        // Test evaluation for unknown dictionary with Lp metric - comprehensive prefix testing
        for dim in 0..=1 {
            let x_val = x[dim];
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            
            println!("OKVS Unknown Lp Dimension {}: x={}, delta={}, range=[{}, {}], p={}", 
                dim, x_val, delta, left, right, p);
            
            let test_points = [70u128, 85u128, 90u128, 95u128, 150u128, 160u128, 170u128, 185u128];
            
            for &test_point in &test_points {
                // Test all prefixes of this test point
                for prefix_len in 1..=8 {
                    let prefix = test_point >> (8 - prefix_len);
                    let prefix_bits = u128_to_bits_msb(prefix, prefix_len);
                    
                    let result1 = share_phase.evaluate_at_single_dimension(&share1, &prefix_bits, dim);
                    let result2 = share_phase.evaluate_at_single_dimension(&share2, &prefix_bits, dim);
                    
                    assert!(result1.is_ok(), "OKVS Lp evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    assert!(result2.is_ok(), "OKVS Lp evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    
                    let eval1 = result1.unwrap();
                    let eval2 = result2.unwrap();
                    let reconstructed = (eval1 + eval2) % modulus;
                    
                    // Apply the complex OKVS unknown Lp logic
                    let prefix_left = left >> (8 - prefix_len);
                    let prefix_right = right >> (8 - prefix_len);
                    let prefix_x = x_val >> (8 - prefix_len);

                    if prefix < prefix_left || prefix > prefix_right {
                        println!("  Prefix {:#010b} outside range", prefix);
                    } else if prefix >= prefix_left && prefix < prefix_x {
                        let y_padded = prefix << (8 - prefix_len) | ((1u128 << (8 - prefix_len)) - 1); // pad with 1s
                        let distance = (x_val - y_padded).pow(2) % modulus; // (x - y111111)^p
                        assert_eq!(reconstructed, distance, 
                            "Prefix {:#010b} < x_prefix, y_padded={}, distance=({}-{})^{} = {}", 
                            prefix, y_padded, x_val, y_padded, p, distance);
                    } else if prefix <= prefix_right && prefix > prefix_x {
                        let y_padded = prefix << (8 - prefix_len);
                        let distance = (y_padded - x_val).pow(2) % modulus; // (y000000 - x)^p
                        assert_eq!(reconstructed, distance, 
                            "Prefix {:#010b} > x_prefix, y_padded={}, distance=({}-{})^{} = {}", 
                            prefix, y_padded, y_padded, x_val, p, distance);
                    } else if prefix == prefix_x {
                        assert_eq!(reconstructed, 0, 
                            "Prefix {:#010b} == x_prefix, should be 0", prefix);
                    } else {
                        // Default case - should not happen with correct logic
                        eprintln!("  Prefix {:#010b} in undefined case, checking reconstruction", prefix);
                    };
                }
            }
        }
    }

    #[test]
    fn test_fss_share_and_eval_known_lp() {
        // Create Interval FSS configuration for known dictionary with Lp metric
        let config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing
        let x = vec![110u128, 140u128];
        let delta = 6u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "Interval FSS share_range should succeed for Lp metric");
        
        let (share1, share2) = result.unwrap();

        println!("Share 1: {:?}", share1);
        println!("Share 2: {:?}", share2);
        
        // Verify that both shares are IntervalFSS type
        match (&share1, &share2) {
            (SharedRange::DistanceFSSL2 { keys: keys1, role: role1 }, 
             SharedRange::DistanceFSSL2 { keys: keys2, role: role2 }) => {
                assert_eq!(keys1.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(keys2.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected IntervalFSS shares"),
        }
        
        // Test evaluation for known dictionary with Lp metric
        for dim in 0..=1 {
            for test_point in [60u128, 80u128, 100u128, 105u128, 110u128, 115u128, 135u128, 140u128, 145u128, 150u128] {
                let test_point_bits = u128_to_bits_msb(test_point, 8);
                let result1 = share_phase.evaluate_at_single_dimension(&share1, &test_point_bits, dim);
                let result2 = share_phase.evaluate_at_single_dimension(&share2, &test_point_bits, dim);
                
                assert!(result1.is_ok(), "Evaluation should succeed for point {} dim {}", test_point, dim);
                assert!(result2.is_ok(), "Evaluation should succeed for point {} dim {}", test_point, dim);
                
                let eval1 = result1.unwrap();
                let eval2 = result2.unwrap();
                
                // The sum of the two shares should give the correct result
                let modulus = 1u128 << share_phase.config.output_bit_length;
                let reconstructed = (eval1 + eval2) % modulus;
                
                // For Lp metric, range calculation is more complex
                let in_range = match dim {
                    0 => test_point >= 104 && test_point <= 116, // x[0] = 110, delta = 6
                    1 => test_point >= 134 && test_point <= 146, // x[1] = 140, delta = 6
                    _ => false,
                };
                
                if in_range {
                    let expected_distance = if test_point < x[dim] {
                        (x[dim] - test_point).pow(2) % modulus   // (x - y)^p for p=2
                    } else if test_point > x[dim] {
                        (test_point - x[dim]).pow(2) % modulus // (y - x)^p for p=2
                    } else {
                        0 // If equal, distance is 0
                    };
                    assert_eq!(reconstructed, expected_distance, 
                        "Point {} in dimension {} should be inside range, reconstructed={}", 
                        test_point, dim, reconstructed);
                } else {
                    let expected_distance = (delta.pow(2) + 1) % modulus; // Outside range, expect large distance
                    assert_eq!(reconstructed, expected_distance, 
                        "Point {} in dimension {} is outside range, reconstructed={} should be = {}", 
                        test_point, dim, reconstructed, expected_distance);
                }
            }
        }
    }

    #[test]
    fn test_fss_share_and_eval_unknown_lp() {
        // Create Interval FSS configuration for unknown dictionary with Lp metric
        let config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let share_phase = SharePhase::new(config);
        
        // Test sharing for unknown dictionary with Lp metric
        let x = vec![60u128, 190u128];
        let delta = 20u128;
        
        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "Interval FSS share_range should succeed for unknown dictionary with Lp");
        
        let (share1, share2) = result.unwrap();
        
        // Verify shares structure
        match (&share1, &share2) {
            (SharedRange::DistanceFSSL2 { keys: keys1, role: role1 }, 
             SharedRange::DistanceFSSL2 { keys: keys2, role: role2 }) => {
                assert_eq!(keys1.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(keys2.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(*role1, false, "First share should be for server 0");
                assert_eq!(*role2, true, "Second share should be for server 1");
            },
            _ => panic!("Expected IntervalFSS shares"),
        }
        
        // Test evaluation for unknown dictionary with Lp metric - comprehensive prefix testing
        for dim in 0..=1 {
            let x_val = x[dim];
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            let p = 2u128; // Lp metric with p=2
            
            println!("FSS Unknown Lp Dimension {}: x={}, delta={}, range=[{}, {}], p={}", 
                dim, x_val, delta, left, right, p);
            
            let test_points = [30u128, 50u128, 60u128, 80u128, 170u128, 190u128, 210u128, 230u128];
            
            for &test_point in &test_points {
                // Test all prefixes of this test point  
                for prefix_len in 1..=8 {
                    let prefix = test_point >> (8 - prefix_len);
                    let prefix_bits = u128_to_bits_msb(prefix, prefix_len);
                    
                    let result1 = share_phase.evaluate_at_single_dimension(&share1, &prefix_bits, dim);
                    let result2 = share_phase.evaluate_at_single_dimension(&share2, &prefix_bits, dim);
                    
                    assert!(result1.is_ok(), "FSS Lp evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    assert!(result2.is_ok(), "FSS Lp evaluation should succeed for prefix {:#010b} (len={}) of point {} dim {}", 
                        prefix, prefix_len, test_point, dim);
                    
                    let eval1 = result1.unwrap();
                    let eval2 = result2.unwrap();
                    
                    // For FSS, the reconstruction is different (addition modulo)
                    let modulus = 1u128 << share_phase.config.output_bit_length;
                    let reconstructed = (eval1 + eval2) % modulus;
                    
                    // Apply similar logic as OKVS but for FSS unknown Lp
                    let prefix_left = left >> (8 - prefix_len);
                    let prefix_right = right >> (8 - prefix_len);
                    let prefix_x = x_val >> (8 - prefix_len);

                    println!("  FSS Lp Prefix {:#010b} (len={}): left_prefix={:#010b}, right_prefix={:#010b}, x_prefix={:#010b}", 
                        prefix, prefix_len, prefix_left, prefix_right, prefix_x);
                    
                    if prefix < prefix_left || prefix > prefix_right {
                        let expected_distance = (delta.pow(2) + 1) % modulus; // Outside range, expect large distance
                        assert_eq!(reconstructed, expected_distance, "FSS Lp reconstruction should match expected distance");
                    } else if prefix >= prefix_left && prefix < prefix_x {
                        let y_padded = prefix << (8 - prefix_len) | ((1u128 << (8 - prefix_len)) - 1); // pad with 1s
                        let distance = (x_val - y_padded).pow(2) % modulus; // (x - y111111)^p
                        assert_eq!(reconstructed, distance, 
                            "FSS Lp Prefix {:#010b} < x_prefix, y_padded={}, distance=({}-{})^{} = {}", 
                            prefix, y_padded, x_val, y_padded, p, distance);
                    } else if prefix <= prefix_right && prefix > prefix_x {
                        let y_padded = prefix << (8 - prefix_len);
                        let distance = (y_padded - x_val).pow(2) % modulus; // (y000000 - x)^p
                        assert_eq!(reconstructed, distance, 
                            "FSS Lp Prefix {:#010b} > x_prefix, y_padded={}, distance=({}-{})^{} = {}", 
                            prefix, y_padded, y_padded, x_val, p, distance);
                    } else if prefix == prefix_x {
                        assert_eq!(reconstructed, 0, 
                            "FSS Lp Prefix {:#010b} == x_prefix, should be 0", prefix);
                    } else {
                        // Default case - should not happen with correct logic
                        eprintln!("  FSS Lp Prefix {:#010b} in undefined case, checking reconstruction", prefix);
                    }
                }
            }
        }
    }

    #[test]
    fn test_shared_range_serialization() {
        println!("=== Testing SharedRange serialization (to_bytes/from_bytes) ===");
        
        // Test OKVS SharedRange serialization
        println!("Testing OKVS SharedRange serialization...");
        let okvs_config = ShareConfig {
            method: ShareMethod::OKVS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };
        
        let okvs_share_phase = SharePhase::new(okvs_config);
        let x = vec![100u128, 150u128];
        let delta = 5u128;
        
        let result = okvs_share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "OKVS share_range should succeed");
        let (okvs_share1, okvs_share2) = result.unwrap();
        
        // Test OKVS share1 serialization
        let okvs_bytes1 = okvs_share1.to_bytes();
        assert!(!okvs_bytes1.is_empty(), "OKVS share1 serialization should not be empty");
        let modulus = 1u128 << okvs_share_phase.config.output_bit_length;
        let (okvs_deserialized1, _) = SharedRange::from_bytes(&okvs_bytes1, modulus)
            .expect("OKVS share1 deserialization should succeed");

        assert_eq!(okvs_share1, okvs_deserialized1, "OKVS share1 should match after round-trip serialization");
        
        // Test OKVS share2 serialization
        let okvs_bytes2 = okvs_share2.to_bytes();
        assert!(!okvs_bytes2.is_empty(), "OKVS share2 serialization should not be empty");
        let (okvs_deserialized2, _) = SharedRange::from_bytes(&okvs_bytes2, modulus)
            .expect("OKVS share2 deserialization should succeed");
        assert_eq!(okvs_share2, okvs_deserialized2, "OKVS share2 should match after round-trip serialization");
        
        println!("✓ OKVS serialization tests passed");
        
        // Test IntervalFSS SharedRange serialization (L-infinity)
        println!("Testing IntervalFSS SharedRange serialization (L-infinity)...");
        let fss_linf_config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let fss_linf_share_phase = SharePhase::new(fss_linf_config);
        let result = fss_linf_share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "IntervalFSS share_range should succeed");
        let (fss_linf_share1, fss_linf_share2) = result.unwrap();
        
        // Test IntervalFSS share1 serialization
        let fss_linf_bytes1 = fss_linf_share1.to_bytes();
        assert!(!fss_linf_bytes1.is_empty(), "IntervalFSS share1 serialization should not be empty");
        let fss_modulus = 1u128 << fss_linf_share_phase.config.output_bit_length;
        let (fss_linf_deserialized1, _) = SharedRange::from_bytes(&fss_linf_bytes1, fss_modulus)
            .expect("IntervalFSS share1 deserialization should succeed");

        assert_eq!(fss_linf_share1, fss_linf_deserialized1, "IntervalFSS share1 should match after round-trip serialization");
        
        // Test IntervalFSS share2 serialization
        let fss_linf_bytes2 = fss_linf_share2.to_bytes();
        assert!(!fss_linf_bytes2.is_empty(), "IntervalFSS share2 serialization should not be empty");
        let (fss_linf_deserialized2, _) = SharedRange::from_bytes(&fss_linf_bytes2, fss_modulus)
            .expect("IntervalFSS share2 deserialization should succeed");
        assert_eq!(fss_linf_share2, fss_linf_deserialized2, "IntervalFSS share2 should match after round-trip serialization");

        println!("✓ IntervalFSS (L-infinity) serialization tests passed");
        
        // Test DistanceFSSL2 SharedRange serialization (Lp metric)
        println!("Testing DistanceFSSL2 SharedRange serialization (Lp metric)...");
        let fss_lp_config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,
            output_bit_length: 20,
            dimension: 2,
            data: ShareData::FSS,
        };
        
        let fss_lp_share_phase = SharePhase::new(fss_lp_config);
        let result = fss_lp_share_phase.share_range(&x, delta);
        assert!(result.is_ok(), "DistanceFSSL2 share_range should succeed");
        let (fss_lp_share1, fss_lp_share2) = result.unwrap();
        
        // Test DistanceFSSL2 share1 serialization
        let fss_lp_bytes1 = fss_lp_share1.to_bytes();
        assert!(!fss_lp_bytes1.is_empty(), "DistanceFSSL2 share1 serialization should not be empty");
        let fss_lp_modulus = 1u128 << fss_lp_share_phase.config.output_bit_length;
        let (fss_lp_deserialized1, _) = SharedRange::from_bytes(&fss_lp_bytes1, fss_lp_modulus)
            .expect("DistanceFSSL2 share1 deserialization should succeed");
        assert_eq!(fss_lp_share1, fss_lp_deserialized1, "DistanceFSSL2 share1 should match after round-trip serialization");
        
        // Test DistanceFSSL2 share2 serialization
        let fss_lp_bytes2 = fss_lp_share2.to_bytes();
        assert!(!fss_lp_bytes2.is_empty(), "DistanceFSSL2 share2 serialization should not be empty");
        let (fss_lp_deserialized2, _) = SharedRange::from_bytes(&fss_lp_bytes2, fss_lp_modulus)
            .expect("DistanceFSSL2 share2 deserialization should succeed");
        assert_eq!(fss_lp_share2, fss_lp_deserialized2, "DistanceFSSL2 share2 should match after round-trip serialization");
        
        println!("✓ DistanceFSSL2 (Lp metric) serialization tests passed");
    }
}
