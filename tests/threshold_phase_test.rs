use counttree::{
    fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, DictionaryType},
    util::u128_to_bits,
    fuzzy_match::check_phase::{CheckPhase, CheckConfig},
    fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData},
    data_structures::{modint::ModInt, payload::RingVec},
    fss::interval::IntervalFSSKey,
};
use scuttlebutt::{AesRng, Channel};
use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
use std::thread;
use std::sync::mpsc;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test the complete threshold phase pipeline with brute force testing:
    /// 1. Multiple clients generate shares using SharePhase
    /// 2. Two servers evaluate at MULTIPLE query points with all client shares
    /// 3. Run check phase to get n ring shares (one per client) for each query
    /// 4. Run threshold phase to aggregate and compare with threshold for each query
    /// 5. Exchange final bits to reconstruct result for each query
    #[test]
    fn test_threshold_phase_complete_pipeline() {
        const NUM_CLIENTS: usize = 5;
        const THRESHOLD: u128 = 3; // Need at least 3 clients to match
        const INPUT_BIT_LENGTH: usize = 8;
        const OUTPUT_BIT_LENGTH: usize = 16;
        
        // Step 1: Set up multiple client points around different locations
        let client_points = vec![
            vec![100u128, 150u128], // Client 1 - center at (100, 150) with delta=5 -> range [95-105, 145-155]
            vec![102u128, 148u128], // Client 2 - center at (102, 148) with delta=5 -> range [97-107, 143-153]
            vec![98u128, 152u128],  // Client 3 - center at (98, 152) with delta=5 -> range [93-103, 147-157]
            vec![105u128, 145u128], // Client 4 - center at (105, 145) with delta=5 -> range [100-110, 140-150]
            vec![200u128, 250u128], // Client 5 - center at (200, 250) with delta=5 -> range [195-205, 245-255]
        ];
        
        let delta = 5u128;
        
        // Step 2: Define multiple query points to test brute force
        let test_query_points = vec![
            // Points that should match multiple clients
            vec![100u128, 150u128], // Should match clients 1, 2, 3, 4 (4 matches >= threshold 3) ✓
            vec![102u128, 148u128], // Should match clients 1, 2, 3, 4 (4 matches >= threshold 3) ✓
            vec![98u128, 152u128],  // Should match clients 1, 2, 3 (3 matches >= threshold 3) ✓
            vec![105u128, 145u128], // Should match clients 1, 2, 4 (3 matches >= threshold 3) ✓
            
            // Points that should match fewer than threshold
            vec![95u128, 155u128],  // Should match clients 1, 3 (2 matches < threshold 3) ✗
            vec![110u128, 140u128], // Should match client 4 only (1 match < threshold 3) ✗
            vec![200u128, 250u128], // Should match client 5 only (1 match < threshold 3) ✗
            
            // Points that should match no clients
            vec![50u128, 50u128],   // Should match no clients (0 matches < threshold 3) ✗
            vec![255u128, 255u128], // Should match no clients (0 matches < threshold 3) ✗
        ];
        
        // Step 3: Generate shares for all clients using SharePhase (only once)
        let share_config = ShareConfig {
            method: ShareMethod::OKVS,
            dictionary_type: DictionaryType::Known,
            input_bit_length: INPUT_BIT_LENGTH,
            output_bit_length: OUTPUT_BIT_LENGTH,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };

        let share_phase = SharePhase::new(share_config);
        
        // Generate shares for all clients
        let mut all_shares_server0 = Vec::new();
        let mut all_shares_server1 = Vec::new();
        
        for client_point in &client_points {
            let (share0, share1) = share_phase.share_range(client_point, delta)
                .expect("Share generation should succeed");
            all_shares_server0.push(share0);
            all_shares_server1.push(share1);
        }

        println!("=== Starting Brute Force Threshold Phase Testing ===");
        println!("Testing {} query points against {} clients", 
            test_query_points.len(), NUM_CLIENTS);
        println!("Threshold: {} matches required", THRESHOLD);
        println!();

        // Step 4: Test each query point
        for (query_idx, query_point) in test_query_points.iter().enumerate() {
            println!("--- Test {}/{}: Query Point {:?} ---", 
                query_idx + 1, test_query_points.len(), query_point);

            // Set up Unix socket pair for communication
            let (stream1, stream2) = UnixStream::pair().expect("Failed to create Unix socket pair");
            
            // Set up communication channel for threshold comparison
            let (sender, receiver) = mpsc::channel();
            
            // Run two-party computation
            let query_point_clone = query_point.clone();
            let all_shares_server1_clone = all_shares_server1.clone();
            let share_phase_clone = share_phase.clone();
            
            let garbler_handle = thread::spawn(move || {
                let mut rng = AesRng::new();
                let reader = BufReader::new(stream1.try_clone().unwrap());
                let writer = BufWriter::new(stream1);
                let mut channel = Channel::new(reader, writer);
                
                // Server 1 (Garbler) operations
                let check_config_garbler = CheckConfig {
                    input_bit_length: OUTPUT_BIT_LENGTH,
                    output_bit_length: OUTPUT_BIT_LENGTH + 4, // Extra bits for aggregation
                    num_dimensions: 2,
                    is_garbler_side: true,
                };
                
                let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
                
                // Run check phase for all client shares
                let mut match_results_server1 = Vec::new();
                
                // Convert query point from u128 to Vec<bool>
                let query_point_bits: Vec<Vec<bool>> = query_point_clone.iter()
                    .map(|&point| u128_to_bits(point, INPUT_BIT_LENGTH))
                    .collect();
                
                for share in &all_shares_server1_clone {
                    let result = check_phase_garbler.run_fuzzy_match_check(
                        share,
                        &query_point_bits,
                        &mut channel,
                        &mut rng,
                    ).expect("Check phase should succeed");
                    
                    match_results_server1.push(result);
                }
                
                // Run threshold phase (garbler side)
                let threshold_config = ThresholdConfig {
                    input_bit_length: OUTPUT_BIT_LENGTH + 4,
                    is_garbler_side: true,
                    method: ThresholdMethod::GarbledCircuits,
                };
                
                let threshold_phase = ThresholdPhase::new(threshold_config);
                let threshold_data = ThresholdData::GarbledCircuits;
                
                let garbler_bit = threshold_phase.compare_with_threshold(
                    &match_results_server1,
                    THRESHOLD,
                    &threshold_data,
                    &mut channel,
                    &mut rng,
                ).expect("Threshold comparison should succeed");
                
                sender.send((match_results_server1, garbler_bit)).unwrap();
            });
            
            // Server 0 (Evaluator) operations in main thread
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream2.try_clone().unwrap());
            let writer = BufWriter::new(stream2);
            let mut channel = Channel::new(reader, writer);
            
            let check_config_evaluator = CheckConfig {
                input_bit_length: OUTPUT_BIT_LENGTH,
                output_bit_length: OUTPUT_BIT_LENGTH + 4,
                num_dimensions: 2,
                is_garbler_side: false,
            };
            
            let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
            
            // Run check phase for all client shares
            let mut match_results_server0 = Vec::new();
            
            // Convert query point from u128 to Vec<bool>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits(point, INPUT_BIT_LENGTH))
                .collect();
            
            for share in &all_shares_server0 {
                let result = check_phase_evaluator.run_fuzzy_match_check(
                    share,
                    &query_point_bits,
                    &mut channel,
                    &mut rng,
                ).expect("Check phase should succeed");
                
                match_results_server0.push(result);
            }
            
            // Run threshold phase (evaluator side)
            let threshold_config = ThresholdConfig {
                input_bit_length: OUTPUT_BIT_LENGTH + 4,
                is_garbler_side: false,
                method: ThresholdMethod::GarbledCircuits,
            };
            
            let threshold_phase = ThresholdPhase::new(threshold_config);
            let threshold_data = ThresholdData::GarbledCircuits;
            
            let evaluator_bit = threshold_phase.compare_with_threshold(
                &match_results_server0,
                THRESHOLD,
                &threshold_data,
                &mut channel,
                &mut rng,
            ).expect("Threshold comparison should succeed");
            
            // Wait for garbler thread to complete
            garbler_handle.join().expect("Garbler thread should complete successfully");
            let (match_results_server1, garbler_bit) = receiver.recv()
                .expect("Should receive results from garbler");
            
            // Verify intermediate results - reconstruct match results for each client
            let mut actual_matches = 0;
            let mut client_match_details = Vec::new();
            
            for i in 0..NUM_CLIENTS {
                let reconstructed_match = match_results_server0[i] + match_results_server1[i];
                let is_match = reconstructed_match.val() == 1;
                
                client_match_details.push((i + 1, client_points[i].clone(), is_match));
                
                if is_match {
                    actual_matches += 1;
                }
            }
            
            // Exchange and reconstruct final threshold result
            let final_result = evaluator_bit ^ garbler_bit;
            
            // Calculate expected result
            let expected_threshold_exceeded = actual_matches >= THRESHOLD as usize;
            
            // Print detailed results for this query
            println!("  Client matches:");
            for (client_id, client_point, is_match) in &client_match_details {
                let match_status = if *is_match { "✓" } else { "✗" };
                println!("    Client {}: {:?} {}", client_id, client_point, match_status);
            }
            println!("  Total matches: {}/{}", actual_matches, NUM_CLIENTS);
            println!("  Server 0 bit: {}, Server 1 bit: {}", evaluator_bit, garbler_bit);
            println!("  Final result: {} (expected: {})", 
                final_result, expected_threshold_exceeded);
            
            // Verify the final threshold result
            assert_eq!(final_result, expected_threshold_exceeded,
                "Query {:?}: Threshold result should be {} (actual_matches={} >= threshold={})",
                query_point, expected_threshold_exceeded, actual_matches, THRESHOLD);
                
            let status = if expected_threshold_exceeded { "EXCEEDED" } else { "NOT EXCEEDED" };
            println!("  Result: {} ✓", status);
            println!();
        }
        
        println!("=== Brute Force Threshold Phase Testing Completed Successfully! ===");
        println!("✅ Tested {} query points against {} clients", 
            test_query_points.len(), NUM_CLIENTS);
        println!("✅ All threshold comparisons worked correctly");
        println!("✅ Threshold: {} matches required", THRESHOLD);
    }

    /// Test the IntervalFSS-based threshold phase implementation
    #[test]
    fn test_threshold_phase_intervalfss() {
        const NUM_CLIENTS: usize = 4;
        const THRESHOLD: u128 = 2;
        const INPUT_BIT_LENGTH: usize = 8;
        const OUTPUT_BIT_LENGTH: usize = 16;
        const CHECK_BIT_LENGTH: usize = 20;
        
        // Step 1: Set up client points - 3 will match, 1 will not
        let client_points = vec![
            vec![100u128, 150u128], // Will match
            vec![102u128, 148u128], // Will match
            vec![98u128, 152u128],  // Will match
            vec![200u128, 250u128], // Will NOT match
        ];
        
        let delta = 5u128;
        let query_point = vec![100u128, 150u128]; // Should match first 3 clients
        
        // Step 2: Generate shares for all clients
        let share_config = ShareConfig {
            method: ShareMethod::OKVS,
            dictionary_type: DictionaryType::Known,
            input_bit_length: INPUT_BIT_LENGTH,
            output_bit_length: OUTPUT_BIT_LENGTH,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
        };

        let share_phase = SharePhase::new(share_config);
        
        let mut all_shares_server0 = Vec::new();
        let mut all_shares_server1 = Vec::new();
        
        for client_point in &client_points {
            let (share0, share1) = share_phase.share_range(client_point, delta)
                .expect("Share generation should succeed");
            all_shares_server0.push(share0);
            all_shares_server1.push(share1);
        }

        // Step 3: Generate random values for privacy (simulating dealer's role)
        use rand::Rng;
        let mut rng = rand::rng();
        let r0 = rng.random::<u128>() % (1u128 << 10); // Keep it small for testing
        let r1 = rng.random::<u128>() % (1u128 << 10); // Keep it small for testing

        println!("Random values: r0={}, r1={}, sum={}", r0, r1, r0 + r1);

        // Step 4: Generate IntervalFSS keys for threshold comparison
        // We want to check if count >= threshold, so we create FSS for interval [threshold + r0 + r1, MAX]
        let fss_input_bits = CHECK_BIT_LENGTH; // Extra bits for aggregation
        let max_value = (1u128 << fss_input_bits) - 1;
        let masked_threshold = THRESHOLD + r0 + r1;
        
        // Convert threshold bounds to bit representation
        let mut alpha_bits = u128_to_bits(masked_threshold, fss_input_bits);  // Lower bound: threshold + r0 + r1
        alpha_bits.reverse();
        let mut beta_bits = u128_to_bits(max_value, fss_input_bits);  // Upper bound: max value
        beta_bits.reverse();

        // Create payload vectors: outside=0, inside=1 (for interval [threshold + r0 + r1, MAX])
        let outside_payload = RingVec::<1>::zero(2);  // Binary modulus
        let inside_payload = RingVec::<1>::new([1], 2);  // Binary modulus
        
        // Generate FSS keys
        let (fss_key_0, fss_key_1) = IntervalFSSKey::gen_IntervalFSSKey(
            &alpha_bits,
            &beta_bits,
            outside_payload,  // left (below threshold + r0 + r1)
            inside_payload,   // mid (in [threshold + r0 + r1, MAX])
            outside_payload,  // right (above MAX - shouldn't happen)
            2  // Binary modulus
        );

        println!("=== Testing IntervalFSS Threshold Phase ===");
        println!("Clients: {}, Threshold: {}, Masked Threshold: {}, Query: {:?}", 
            NUM_CLIENTS, THRESHOLD, masked_threshold, query_point);

        // Step 5: Run the two-party computation
        let (stream1, stream2) = UnixStream::pair().expect("Failed to create Unix socket pair");
        let (sender, receiver) = mpsc::channel();
        
        let query_point_clone = query_point.clone();
        let all_shares_server1_clone = all_shares_server1.clone();
        let share_phase_clone = share_phase.clone();
        let fss_key_1_clone = fss_key_1.clone();
        
        let garbler_handle = thread::spawn(move || {
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream1.try_clone().unwrap());
            let writer = BufWriter::new(stream1);
            let mut channel = Channel::new(reader, writer);
            
            // Server 1 operations
            let check_config_garbler = CheckConfig {
                input_bit_length: OUTPUT_BIT_LENGTH,
                output_bit_length: CHECK_BIT_LENGTH,
                num_dimensions: 2,
                is_garbler_side: true,
            };
            
            let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
            
            // Run check phase for all client shares
            let mut match_results_server1 = Vec::new();
            
            // Convert query point from u128 to Vec<bool>
            let query_point_bits: Vec<Vec<bool>> = query_point_clone.iter()
                .map(|&point| u128_to_bits(point, INPUT_BIT_LENGTH))
                .collect();
            
            for share in &all_shares_server1_clone {
                let result = check_phase_garbler.run_fuzzy_match_check(
                    share,
                    &query_point_bits,
                    &mut channel,
                    &mut rng,
                ).expect("Check phase should succeed");
                
                match_results_server1.push(result);
                println!("Server 1: Client share match for share {:?} result: {:?}", share, result);
            }
            
            // Run threshold phase using IntervalFSS
            let threshold_config = ThresholdConfig {
                input_bit_length: CHECK_BIT_LENGTH,
                is_garbler_side: true,
                method: ThresholdMethod::IntervalFSS,
            };
            
            let threshold_phase = ThresholdPhase::new(threshold_config);
            let threshold_data = ThresholdData::IntervalFSS {
                fss_key: fss_key_1_clone,
                random_value: r1,
            };
            
            let garbler_result = threshold_phase.compare_with_threshold(
                &match_results_server1,
                THRESHOLD,
                &threshold_data,
                &mut channel,
                &mut rng,
            ).expect("IntervalFSS threshold comparison should succeed");
            
            sender.send((match_results_server1, garbler_result)).unwrap();
        });
        
        // Server 0 operations in main thread
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream2.try_clone().unwrap());
        let writer = BufWriter::new(stream2);
        let mut channel = Channel::new(reader, writer);
        
        let check_config_evaluator = CheckConfig {
            input_bit_length: OUTPUT_BIT_LENGTH,
            output_bit_length: CHECK_BIT_LENGTH,
            num_dimensions: 2,
            is_garbler_side: false,
        };
        
        let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
        
        // Run check phase for all client shares
        let mut match_results_server0 = Vec::new();
        
        // Convert query point from u128 to Vec<bool>
        let query_point_bits: Vec<Vec<bool>> = query_point.iter()
            .map(|&point| u128_to_bits(point, INPUT_BIT_LENGTH))
            .collect();
        
        for share in &all_shares_server0 {
            let result = check_phase_evaluator.run_fuzzy_match_check(
                share,
                &query_point_bits,
                &mut channel,
                &mut rng,
            ).expect("Check phase should succeed");
            
            match_results_server0.push(result);
        }

        println!("Server 0: Match results: {:?}", match_results_server0);

        // Run threshold phase using IntervalFSS
        let threshold_config = ThresholdConfig {
            input_bit_length: CHECK_BIT_LENGTH,
            is_garbler_side: false,
            method: ThresholdMethod::IntervalFSS,
        };
            
        let threshold_phase = ThresholdPhase::new(threshold_config);
        let threshold_data = ThresholdData::IntervalFSS {
            fss_key: fss_key_0,
            random_value: r0,
        };
            
        let evaluator_result = threshold_phase.compare_with_threshold(
            &match_results_server0,
            THRESHOLD,
            &threshold_data,
            &mut channel,
            &mut rng,
        ).expect("IntervalFSS threshold comparison should succeed");        // Wait for garbler thread
        garbler_handle.join().expect("Garbler thread should complete successfully");
        let (match_results_server1, garbler_result) = receiver.recv()
            .expect("Should receive results from garbler");
        
        // Step 5: Verify results
        let mut actual_matches = 0;
        
        for i in 0..NUM_CLIENTS {
            let reconstructed_match = match_results_server0[i] + match_results_server1[i];
            let is_match = reconstructed_match.val() == 1;
            
            println!("Client {}: Point {:?}, Match: {}", 
                i + 1, client_points[i], is_match);
            
            if is_match {
                actual_matches += 1;
            }
        }
        
        println!("Total matches: {}, Threshold: {}", actual_matches, THRESHOLD);
        println!("Server 0 FSS result: {}", evaluator_result);
        println!("Server 1 FSS result: {}", garbler_result);

        let final_result = evaluator_result ^ garbler_result;
        println!("Final IntervalFSS result: {}", final_result);
        
        // The result should be true if actual_matches >= threshold
        let expected_result = actual_matches >= THRESHOLD as usize;
        assert_eq!(final_result, expected_result,
            "IntervalFSS threshold result should be {} (actual_matches={} >= threshold={})",
            expected_result, actual_matches, THRESHOLD);
            
        println!("✅ IntervalFSS threshold test passed!");
        println!("   - {} matches >= {} threshold: {}", 
            actual_matches, THRESHOLD, expected_result);
    }
    
}
