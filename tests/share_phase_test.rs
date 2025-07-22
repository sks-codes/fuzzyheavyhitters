use counttree::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, SharePhaseError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_okvs_share_and_eval() {
        // Create OKVS configuration
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 8,
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
            (SharedRange::OKVS { okvs_shares: share1_data }, 
             SharedRange::OKVS { okvs_shares: share2_data }) => {
                assert!(!share1_data.is_empty(), "First OKVS share should not be empty");
                assert!(!share2_data.is_empty(), "Second OKVS share should not be empty");
                assert_eq!(share1_data.len(), share2_data.len(), "Both shares should have same length");
            },
            _ => panic!("Expected OKVS shares"),
        }
        
        // Test evaluation at points within and outside the range
        let test_points = vec![
            (102u128, 0), // Within [95, 105] for dimension 0
            (152u128, 1), // Within [145, 155] for dimension 1
            (90u128, 0),  // Outside range for dimension 0
            (160u128, 1), // Outside range for dimension 1
        ];
        
        for (test_point, dim) in test_points {
            let result1 = share_phase.evaluate_at_single_dimension(&share1, test_point, dim);
            let result2 = share_phase.evaluate_at_single_dimension(&share2, test_point, dim);
            
            assert!(result1.is_ok(), "Evaluation of first share should succeed for point {} dim {}", test_point, dim);
            assert!(result2.is_ok(), "Evaluation of second share should succeed for point {} dim {}", test_point, dim);
            
            let eval1 = result1.unwrap();
            let eval2 = result2.unwrap();
            
            // The XOR of the two shares should give the correct result
            let reconstructed = eval1 ^ eval2;
            
            // Check if point is in expected range
            let in_range = match dim {
                0 => test_point >= 95 && test_point <= 105, // x[0] = 100, delta = 5
                1 => test_point >= 145 && test_point <= 155, // x[1] = 150, delta = 5
                _ => false,
            };
            
            // For OKVS, we expect the XOR result to be non-zero if the point is exactly x[dim]
            // Since OKVS encodes value=1 at x[dim] and value=0 elsewhere
            let expected_nonzero = match dim {
                0 => test_point == 100, // Should be non-zero only at x[0] = 100
                1 => test_point == 150, // Should be non-zero only at x[1] = 150  
                _ => false,
            };
            
            if expected_nonzero {
                assert_ne!(reconstructed, 0, "Point {} in dimension {} should have non-zero value (point equals x[dim])", test_point, dim);
            } else {
                assert_eq!(reconstructed, 0, "Point {} in dimension {} should have zero value (point does not equal x[dim])", test_point, dim);
            }
        }
    }

    #[test]
    fn test_interval_fss_share_and_eval() {
        // Create Interval FSS configuration
        let config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 8, // v = 8, for output values
            dimension: 2,
            data: ShareData::IntervalFSS {},
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
            (SharedRange::IntervalFSS { fss_key: keys1 }, 
             SharedRange::IntervalFSS { fss_key: keys2 }) => {
                assert_eq!(keys1.len(), 2, "Should have 2 FSS keys for dimension 2");
                assert_eq!(keys2.len(), 2, "Should have 2 FSS keys for dimension 2");
            },
            _ => panic!("Expected IntervalFSS shares"),
        }
        
        // Test evaluation at points within and outside the range
        let test_points = vec![
            (102u128, 0), // Within [95, 105] for dimension 0
            (152u128, 1), // Within [145, 155] for dimension 1
            (90u128, 0),  // Outside range for dimension 0
            (160u128, 1), // Outside range for dimension 1
        ];
        
        for (test_point, dim) in test_points {
            let result1 = share_phase.evaluate_at_single_dimension(&share1, test_point, dim);
            let result2 = share_phase.evaluate_at_single_dimension(&share2, test_point, dim);
            
            assert!(result1.is_ok(), "Evaluation of first share should succeed for point {} dim {}", test_point, dim);
            assert!(result2.is_ok(), "Evaluation of second share should succeed for point {} dim {}", test_point, dim);
            
            let eval1 = result1.unwrap();
            let eval2 = result2.unwrap();
            
            // The sum of the two shares (mod 2) should give the correct result
            let reconstructed = eval1 ^ eval2; // XOR for boolean addition mod 2
            
            // Check if point is in expected range
            let in_range = match dim {
                0 => test_point >= 95 && test_point <= 105, // x[0] = 100, delta = 5
                1 => test_point >= 145 && test_point <= 155, // x[1] = 150, delta = 5
                _ => false,
            };
            
            // For Interval FSS, we expect the result to be 0 inside the interval and 1 outside
            // Since we use left=1, mid=0, right=1 configuration
            if in_range {
                assert_eq!(reconstructed, 0, "Point {} in dimension {} should be inside range (result=0)", test_point, dim);
            } else {
                assert_eq!(reconstructed, 1, "Point {} in dimension {} should be outside range (result=1)", test_point, dim);
            }
        }
    }
}
