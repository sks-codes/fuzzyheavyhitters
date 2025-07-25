use counttree::{
    fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType},
    fuzzy_match::check_phase::{CheckPhase, CheckConfig},
    data_structures::modint::ModInt,
    util::u128_to_bits,
};
use scuttlebutt::{AesRng, Channel};
use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
use std::thread;
use std::sync::mpsc;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test the complete check phase pipeline: 
    /// 1. Generate OKVS shares using SharePhase
    /// 2. Evaluate at query points on both shares  
    /// 3. Run check phase to compare evaluations using garbled circuits
    #[test]
    fn test_okvs_check_phase_pipeline() {
        // Set up configuration for OKVS sharing
        let share_config = ShareConfig {
            method: ShareMethod::OKVS,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 16, // v = 16 for output values
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };

        let share_phase = SharePhase::new(share_config);

        // Step 1: Generate shares for a range around x with delta
        let x = vec![100u128, 150u128];
        let delta = 5u128;

        let (share1, share2) = share_phase.share_range(&x, delta)
            .expect("OKVS share generation should succeed");

        // Step 2: Define query points to test
        let test_cases = vec![
            // Points inside the range (should result in equal evaluations)
            vec![100u128, 150u128], // Exact center
            vec![98u128, 148u128],  // Inside range
            vec![102u128, 152u128], // Inside range
            
            // Points outside the range (should result in different evaluations)
            vec![90u128, 140u128],  // Outside range
            vec![110u128, 160u128], // Outside range
            vec![50u128, 200u128],  // Far outside range
        ];

        for (test_idx, query_point) in test_cases.iter().enumerate() {
            println!("Testing query point {:?} (test case {})", query_point, test_idx);

            // Step 4: Run the check phase using Unix domain sockets for communication
            let (stream1, stream2) = UnixStream::pair().expect("Failed to create Unix socket pair");

            let query_point_clone = query_point.clone();
            let share1_clone = share1.clone();
            let share2_clone = share2.clone();
            let share_phase_clone = share_phase.clone();

            // Create channels for both parties
            let (sender, receiver) = mpsc::channel();

            // Spawn garbler thread (Party 1)
            let garbler_handle = thread::spawn(move || {
                let mut rng = AesRng::new();
                let reader = BufReader::new(stream1.try_clone().unwrap());
                let writer = BufWriter::new(stream1);
                let mut channel = Channel::new(reader, writer);

                let mut check_config_garbler = CheckConfig {
                    input_bit_length: 16,
                    output_bit_length: 20,
                    num_dimensions: 2,
                    is_garbler_side: true,
                };

                let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
                
                // Convert query point from u128 to Vec<bool>
                let query_point_bits: Vec<Vec<bool>> = query_point_clone.iter()
                    .map(|&point| u128_to_bits(point, 8))
                    .collect();
                
                let result = check_phase_garbler.run_fuzzy_match_check(
                    &share2_clone, // Server 1 gets share2
                    &query_point_bits,
                    &mut channel,
                    &mut rng,
                );

                sender.send(result).unwrap();
            });

            // Run evaluator (Party 0) in main thread
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream2.try_clone().unwrap());
            let writer = BufWriter::new(stream2);
            let mut channel = Channel::new(reader, writer);

            let check_config_evaluator = CheckConfig {
                input_bit_length: 16,
                output_bit_length: 20,
                num_dimensions: 2,
                is_garbler_side: false,
            };

            let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
            
            // Convert query point from u128 to Vec<bool>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits(point, 8))
                .collect();
            
            let evaluator_result = check_phase_evaluator.run_fuzzy_match_check(
                &share1, // Server 0 gets share1
                &query_point_bits,
                &mut channel,
                &mut rng,
            );

            // Wait for garbler result
            garbler_handle.join().expect("Garbler thread panicked");
            let garbler_result = receiver.recv().expect("Failed to receive garbler result");

            // Step 5: Verify results
            assert!(evaluator_result.is_ok(), "Evaluator check phase should succeed for test case {}", test_idx);
            assert!(garbler_result.is_ok(), "Garbler check phase should succeed for test case {}", test_idx);

            let eval_share = evaluator_result.unwrap();
            let garb_share = garbler_result.unwrap();

            // Reconstruct the final result by adding the shares
            let final_result = eval_share + garb_share;

            // Check if the query point is in the expected range
            let in_range = query_point[0] >= 95 && query_point[0] <= 105 &&
                          query_point[1] >= 145 && query_point[1] <= 155;

            println!("Query point {:?}: in_range={}, final_result={}", 
                    query_point, in_range, final_result.val);

            // The final result should be 1 if evaluations are equal (point is in range),
            // and 0 if evaluations are different (point is outside range)
            if in_range {
                assert_eq!(final_result.val, 1, 
                    "Query point {:?} is in range, evaluations should be equal (result=1)", query_point);
            } else {
                assert_eq!(final_result.val, 0, 
                    "Query point {:?} is outside range, evaluations should be different (result=0)", query_point);
            }
        }
    }

    /// Test the check phase with IntervalFSS shares
    #[test]
    fn test_interval_fss_check_phase_pipeline() {
        // Set up configuration for IntervalFSS sharing
        let share_config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            dictionary_type: DictionaryType::Known,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 16, // v = 16 for output values
            dimension: 2,
            data: ShareData::IntervalFSS {},
        };

        let share_phase = SharePhase::new(share_config);

        // Generate shares for a range around x with delta
        let x = vec![100u128, 150u128];
        let delta = 5u128;

        let (share1, share2) = share_phase.share_range(&x, delta)
            .expect("IntervalFSS share generation should succeed");

        // Test with a few query points
        let test_cases = vec![
            vec![100u128, 150u128], // Inside range
            vec![90u128, 140u128],  // Outside range
        ];

        for (test_idx, query_point) in test_cases.iter().enumerate() {
            println!("Testing IntervalFSS with query point {:?}", query_point);

            // Create Unix domain socket pair
            let (stream1, stream2) = UnixStream::pair().expect("Failed to create Unix socket pair");

            let query_point_clone = query_point.clone();
            let share1_clone = share1.clone();
            let share2_clone = share2.clone();
            let share_phase_clone = share_phase.clone();

            let (sender, receiver) = mpsc::channel();

            // Spawn garbler thread
            let garbler_handle = thread::spawn(move || {
                let mut rng = AesRng::new();
                let reader = BufReader::new(stream1.try_clone().unwrap());
                let writer = BufWriter::new(stream1);
                let mut channel = Channel::new(reader, writer);

                let check_config_garbler = CheckConfig {
                    input_bit_length: 16,
                    output_bit_length: 20,
                    num_dimensions: 2,
                    is_garbler_side: true,
                };

                let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
                
                // Convert query point from u128 to Vec<bool>
                let query_point_bits: Vec<Vec<bool>> = query_point_clone.iter()
                    .map(|&point| u128_to_bits(point, 8))
                    .collect();
                
                let result = check_phase_garbler.run_fuzzy_match_check(
                    &share2_clone,
                    &query_point_bits,
                    &mut channel,
                    &mut rng,
                );

                sender.send(result).unwrap();
            });

            // Run evaluator in main thread
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream2.try_clone().unwrap());
            let writer = BufWriter::new(stream2);
            let mut channel = Channel::new(reader, writer);

            let check_config_evaluator = CheckConfig {
                input_bit_length: 16,
                output_bit_length: 20,
                num_dimensions: 2,
                is_garbler_side: false,
            };

            let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
            
            // Convert query point from u128 to Vec<bool>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits(point, 8))
                .collect();
            
            let evaluator_result = check_phase_evaluator.run_fuzzy_match_check(
                &share1,
                &query_point_bits,
                &mut channel,
                &mut rng,
            );

            // Wait for garbler and get results
            garbler_handle.join().expect("Garbler thread panicked");
            let garbler_result = receiver.recv().expect("Failed to receive garbler result");

            // Verify both parties succeeded
            assert!(evaluator_result.is_ok(), "Evaluator should succeed for IntervalFSS test case {}", test_idx);
            assert!(garbler_result.is_ok(), "Garbler should succeed for IntervalFSS test case {}", test_idx);

            let eval_share = evaluator_result.unwrap();
            let garb_share = garbler_result.unwrap();

            // Reconstruct the final result
            let final_result = eval_share + garb_share;

            // Check if the query point is in the expected range
            let in_range = query_point[0] >= 95 && query_point[0] <= 105 &&
                          query_point[1] >= 145 && query_point[1] <= 155;

            println!("IntervalFSS Query point {:?}: in_range={}, final_result={}", 
                    query_point, in_range, final_result.val);

            // For IntervalFSS, the logic is the same: 1 if equal (in range), 0 if different (outside range)
            if in_range {
                assert_eq!(final_result.val, 1, 
                    "Query point {:?} is in range, evaluations should be equal (result=1)", query_point);
            } else {
                assert_eq!(final_result.val, 0, 
                    "Query point {:?} is outside range, evaluations should be different (result=0)", query_point);
            }
        }
    }
}
