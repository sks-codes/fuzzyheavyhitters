use counttree::{
    fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType, DistanceMetric},
    fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckData},
    data_structures::modint::ModInt,
    data_structures::payload::RingVec,
    util::{u128_to_bits, u128_to_bits_msb},
    channel::CommTrackingChannel,
    fss::interval::IntervalFSSKey,
};
use scuttlebutt::AesRng;
use rand::Rng;
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test L-infinity check phase with L-infinity metric
    /// Uses FSS and DistanceFSSL2 for sharing with 3-dimensional points
    #[test]
    fn test_linf_check_linf() {
        // Set up configuration for FSS sharing with DistanceFSSL2
        let share_config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::LInfinity,
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 16, // v = 16 for output values
            dimension: 3,  // 3-dimensional points
            data: ShareData::FSS,
        };

        let share_phase = SharePhase::new(share_config);

        // Generate shares for a range around x with delta
        let x = vec![120u128, 180u128, 80u128];  // 3-dimensional point
        let delta = 10u128;

        let (share1, share2) = share_phase.share_range(&x, delta)
            .expect("FSS share generation should succeed");

        // Test 3-dimensional points
        let test_points_3d = [
            vec![100u128, 170u128, 70u128],  // Mixed - some dimensions in range
            vec![115u128, 185u128, 85u128],  // Close to x
            vec![120u128, 180u128, 80u128],  // Same as x
            vec![50u128, 150u128, 50u128],   // All dimensions below range
            vec![150u128, 210u128, 110u128], // All dimensions above range
            vec![120u128, 200u128, 60u128],  // Mixed - dim1 in range, dim2-3 out
            vec![110u128, 180u128, 90u128],  // All dimensions in range
            vec![130u128, 170u128, 70u128],  // Mixed - dim1 out, dim2-3 in range
            vec![0u128, 255u128, 128u128],   // Extreme values
        ];
        
        // Calculate expected ranges for each dimension
        let ranges: Vec<(u128, u128)> = x.iter().map(|&x_val| {
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            (left, right)
        }).collect();

        println!("L-inf check L-inf 3D: x={:?}, delta={}, ranges={:?}", x, delta, ranges);

        // Create TCP listener once for all tests
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind TCP listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");
        let share2_clone = share2.clone();
        let share_phase_clone = share_phase.clone();
        let test_points_3d_clone = test_points_3d.clone();

        let (sender, receiver) = mpsc::channel();

        // Spawn garbler thread (Party 1) once
        let garbler_handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50)); // Give server time to start
            let stream1 = TcpStream::connect(local_addr).expect("Failed to connect to TCP server");
            
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream1.try_clone().unwrap());
            let writer = BufWriter::new(stream1);
            let mut channel = CommTrackingChannel::new(reader, writer);

            let check_config_garbler = CheckConfig {
                input_bit_length: 16,
                output_bit_length: 20,
                num_dimensions: 3,
                is_garbler_side: true,
                method: CheckMethod::Linf,
            };

            let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
            
            // Test only len1=len2=len3=4 for efficiency
            for test_point in &test_points_3d_clone {
                let prefix1 = test_point[0] >> (8 - 4);
                let prefix2 = test_point[1] >> (8 - 4);
                let prefix3 = test_point[2] >> (8 - 4);
                let query_point = vec![prefix1, prefix2, prefix3];
                
                // Convert query point to Vec<Vec<bool>>
                let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                    .map(|&point| u128_to_bits_msb(point, 4))
                    .collect();
                
                let check_data = CheckData::Linf;
                
                let result = check_phase_garbler.run_fuzzy_match_check(
                    &share2_clone, // Server 1 gets share2
                    &query_point_bits,
                    &check_data,
                    &mut channel,
                    &mut rng,
                );

                sender.send(result).unwrap();
            }
        });

        // Run evaluator (Party 0) in main thread - accept connection once
        let (stream2, _) = listener.accept().expect("Failed to accept connection");
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream2.try_clone().unwrap());
        let writer = BufWriter::new(stream2);
        let mut channel = CommTrackingChannel::new(reader, writer);

        let check_config_evaluator = CheckConfig {
            input_bit_length: 16,
            output_bit_length: 20,
            num_dimensions: 3,
            is_garbler_side: false,
            method: CheckMethod::Linf,
        };

        let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
        
        for test_point in &test_points_3d {
            println!("Testing 3D point: {:?}", test_point);
            
            // Test only len1=len2=len3=4 for efficiency
            let prefix1 = test_point[0] >> (8 - 4);
            let prefix2 = test_point[1] >> (8 - 4);
            let prefix3 = test_point[2] >> (8 - 4);
            let query_point = vec![prefix1, prefix2, prefix3];
            
            println!("  Testing 3D prefix: [{:#010b}(4), {:#010b}(4), {:#010b}(4)]", 
                prefix1, prefix2, prefix3);

            // Convert query point to Vec<Vec<bool>>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits_msb(point, 4))
                .collect();
            
            let check_data = CheckData::Linf;
            
            let evaluator_result = check_phase_evaluator.run_fuzzy_match_check(
                &share1, // Server 0 gets share1
                &query_point_bits,
                &check_data,
                &mut channel,
                &mut rng,
            );

            // Wait for garbler result for this test point
            let garbler_result = receiver.recv().expect("Failed to receive garbler result");

            // Verify results
            assert!(evaluator_result.is_ok(), "Evaluator check phase should succeed");
            assert!(garbler_result.is_ok(), "Garbler check phase should succeed");

            let eval_share = evaluator_result.unwrap();
            let garb_share = garbler_result.unwrap();

            // Reconstruct the final result by adding the shares
            let final_result = eval_share + garb_share;

            // Check if the 3D prefix is in the expected range for L-infinity metric
            // For FSS with L-infinity metric, we check if all dimensions are within range
            let mut all_in_range = true;
            for dim in 0..3 {
                let prefix_left = ranges[dim].0 >> (8 - 4);
                let prefix_right = ranges[dim].1 >> (8 - 4);
                let prefix = query_point[dim];
                if !(prefix >= prefix_left && prefix <= prefix_right) {
                    all_in_range = false;
                    break;
                }
            }

            println!("    3D Prefix in_range={}, result={}", all_in_range, final_result.val);

            // For FSS L-infinity check with L-infinity metric:
            // Result should be 1 if all dimensions are in range, 0 if any dimension is outside range
            if all_in_range {
                assert_eq!(final_result.val, 1, 
                    "3D Prefix {:?} is in range, result should be 1", query_point);
            } else {
                assert_eq!(final_result.val, 0, 
                    "3D Prefix {:?} is outside range, result should be 0", query_point);
            }
        }

        // Wait for garbler thread to complete
        garbler_handle.join().expect("Garbler thread panicked");
    }

    /// Test Lp garbled circuits check phase with Lp metric
    /// Uses FSS and DistanceFSSL2 for sharing with 3-dimensional points
    #[test]
    fn test_lpgarbledcircuits_check_lp() {
        // Set up configuration for FSS sharing with DistanceFSSL2 and Lp metric
        let share_config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 16, // v = 16 for output values
            dimension: 3,  // 3-dimensional points
            data: ShareData::FSS,
        };

        let share_phase = SharePhase::new(share_config);

        // Generate shares for a range around x with delta
        let x = vec![90u128, 160u128, 200u128];  // 3-dimensional point
        let delta = 12u128;
        let modulus = 1u128 << share_phase.config.output_bit_length;
        let threshold = delta.pow(2) % modulus; // For p=2, threshold is delta^2

        let (share1, share2) = share_phase.share_range(&x, delta)
            .expect("FSS share generation should succeed");

        // Test 3-dimensional points
        let test_points_3d = [
            vec![70u128, 150u128, 180u128],  // Mixed distances
            vec![85u128, 170u128, 210u128],  // Close to x
            vec![90u128, 160u128, 200u128],  // Same as x
            vec![40u128, 120u128, 150u128],  // All dimensions farther away
            vec![110u128, 190u128, 230u128], // All dimensions above x
            vec![90u128, 140u128, 180u128],  // Mixed - dim1 same, others different
            vec![75u128, 165u128, 205u128],  // All dimensions close to x
            vec![120u128, 130u128, 240u128], // Mixed - some close, some far
            vec![255u128, 0u128, 100u128],   // Extreme values
        ];
        
        // Calculate expected ranges for each dimension
        let ranges: Vec<(u128, u128)> = x.iter().map(|&x_val| {
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            (left, right)
        }).collect();

        println!("Lp garbled circuits check Lp 3D: x={:?}, delta={}, ranges={:?}, threshold={}", 
            x, delta, ranges, threshold);

        // Create TCP listener once for all tests
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind TCP listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");
        let share2_clone = share2.clone();
        let share_phase_clone = share_phase.clone();
        let test_points_3d_clone = test_points_3d.clone();

        let (sender, receiver) = mpsc::channel();

        // Spawn garbler thread (Party 1) once
        let garbler_handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50)); // Give server time to start
            let stream1 = TcpStream::connect(local_addr).expect("Failed to connect to TCP server");
            
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream1.try_clone().unwrap());
            let writer = BufWriter::new(stream1);
            let mut channel = CommTrackingChannel::new(reader, writer);

            let check_config_garbler = CheckConfig {
                input_bit_length: 16,
                output_bit_length: 20,
                num_dimensions: 3,
                is_garbler_side: true,
                method: CheckMethod::LpGarbledCircuits,
            };

            let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
            
            // Test only len1=len2=len3=4 for efficiency
            for test_point in &test_points_3d_clone {
                let prefix1 = test_point[0] >> (8 - 4);
                let prefix2 = test_point[1] >> (8 - 4);
                let prefix3 = test_point[2] >> (8 - 4);
                let query_point = vec![prefix1, prefix2, prefix3];
                
                // Convert query point to Vec<Vec<bool>>
                let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                    .map(|&point| u128_to_bits_msb(point, 4))
                    .collect();
                
                let check_data = CheckData::LpGarbledCircuits { threshold };
                
                let result = check_phase_garbler.run_fuzzy_match_check(
                    &share2_clone, // Server 1 gets share2
                    &query_point_bits,
                    &check_data,
                    &mut channel,
                    &mut rng,
                );

                sender.send(result).unwrap();
            }
        });

        // Run evaluator (Party 0) in main thread - accept connection once
        let (stream2, _) = listener.accept().expect("Failed to accept connection");
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream2.try_clone().unwrap());
        let writer = BufWriter::new(stream2);
        let mut channel = CommTrackingChannel::new(reader, writer);

        let check_config_evaluator = CheckConfig {
            input_bit_length: 16,
            output_bit_length: 20,
            num_dimensions: 3,
            is_garbler_side: false,
            method: CheckMethod::LpGarbledCircuits,
        };

        let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
        
        for test_point in &test_points_3d {
            println!("Testing 3D point: {:?}", test_point);
            
            // Test only len1=len2=len3=4 for efficiency
            let prefix1 = test_point[0] >> (8 - 4);
            let prefix2 = test_point[1] >> (8 - 4);
            let prefix3 = test_point[2] >> (8 - 4);
            let query_point = vec![prefix1, prefix2, prefix3];
            
            println!("  Testing 3D prefix: [{:#010b}(4), {:#010b}(4), {:#010b}(4)]", 
                prefix1, prefix2, prefix3);

            // Convert query point to Vec<Vec<bool>>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits_msb(point, 4))
                .collect();
            
            let check_data = CheckData::LpGarbledCircuits { threshold };
            
            let evaluator_result = check_phase_evaluator.run_fuzzy_match_check(
                &share1, // Server 0 gets share1
                &query_point_bits,
                &check_data,
                &mut channel,
                &mut rng,
            );

            // Wait for garbler result for this test point
            let garbler_result = receiver.recv().expect("Failed to receive garbler result");

            // Verify results
            assert!(evaluator_result.is_ok(), "Evaluator check phase should succeed");
            assert!(garbler_result.is_ok(), "Garbler check phase should succeed");

            let eval_share = evaluator_result.unwrap();
            let garb_share = garbler_result.unwrap();

            // Reconstruct the final result by adding the shares
            let final_result = eval_share + garb_share;

            // Calculate L2 distance for 3D point using FSS DistanceFSSL2
            let mut total_distance_squared = 0u128;
            for dim in 0..3 {
                let prefix_left = ranges[dim].0 >> (8 - 4);
                let prefix_right = ranges[dim].1 >> (8 - 4);
                let prefix_x = x[dim] >> (8 - 4);
                let prefix = query_point[dim];

                let dim_distance_squared = if prefix < prefix_left || prefix > prefix_right {
                    // Outside range - FSS returns maximum distance
                    (delta.pow(2) + 1) % modulus
                } else {
                    // Within range - calculate actual squared distance
                    let actual_distance = if prefix < prefix_x {
                        let y_padded = prefix << (8 - 4) | ((1u128 << (8 - 4)) - 1);
                        x[dim] - y_padded
                    } else if prefix > prefix_x {
                        let y_padded = prefix << (8 - 4);
                        y_padded - x[dim]
                    } else {
                        0 // Same prefix, no distance
                    };
                    actual_distance.pow(2) % modulus
                };
                
                total_distance_squared += dim_distance_squared;
            }
            
            let expected_distance = total_distance_squared % modulus;

            // For FSS Lp garbled circuits check: result should be 1 if distance <= threshold, 0 otherwise
            let should_be_within_threshold = expected_distance <= threshold;

            println!("    3D Prefix expected_distance={}, threshold={}, within_threshold={}, result={}", 
                expected_distance, threshold, should_be_within_threshold, final_result.val);

            if should_be_within_threshold {
                assert_eq!(final_result.val, 1, 
                    "3D Prefix {:?} distance {} <= threshold {}, result should be 1", query_point, expected_distance, threshold);
            } else {
                assert_eq!(final_result.val, 0, 
                    "3D Prefix {:?} distance {} > threshold {}, result should be 0", query_point, expected_distance, threshold);
            }
        }

        // Wait for garbler thread to complete
        garbler_handle.join().expect("Garbler thread panicked");
    }

    /// Test Lp IntervalFSS check phase with Lp metric
    /// Uses FSS and DistanceFSSL2 for sharing with 3-dimensional points
    #[test]
    fn test_lpintervalfss_check_lp() {
        // Set up configuration for FSS sharing with DistanceFSSL2 and Lp metric
        let share_config = ShareConfig {
            method: ShareMethod::FSS,
            metric: DistanceMetric::Lp { p: 2 },
            dictionary_type: DictionaryType::Unknown,
            input_bit_length: 8,  // u = 8, max value = 255
            output_bit_length: 16, // v = 16 for output values
            dimension: 3,  // 3-dimensional points
            data: ShareData::FSS,        
        };

        let share_phase = SharePhase::new(share_config);

        // Generate shares for a range around x with delta
        let x = vec![60u128, 190u128, 100u128];  // 3-dimensional point
        let delta = 20u128;
        let threshold = delta.pow(2); // For p=2, threshold is delta^2

        let (share1, share2) = share_phase.share_range(&x, delta)
            .expect("FSS share generation should succeed");

        // Test 3-dimensional points
        let test_points_3d = [
            vec![30u128, 170u128, 80u128],   // Mixed distances
            vec![50u128, 210u128, 120u128],  // Close to x  
            vec![60u128, 190u128, 100u128],  // Same as x
            vec![20u128, 150u128, 60u128],   // All dimensions below x
            vec![100u128, 230u128, 140u128], // All dimensions above x
            vec![60u128, 170u128, 80u128],   // Mixed - dim1 same, others different
            vec![40u128, 200u128, 110u128],  // All dimensions close to x
            vec![80u128, 160u128, 150u128],  // Mixed - some close, some far
            vec![255u128, 0u128, 255u128],   // Extreme corner values
        ];
        
        // Calculate expected ranges for each dimension
        let ranges: Vec<(u128, u128)> = x.iter().map(|&x_val| {
            let left = if x_val > delta { x_val - delta } else { 0 };
            let right = if x_val < (255 - delta) { x_val + delta } else { 255 };
            (left, right)
        }).collect();

        println!("Lp IntervalFSS check Lp 3D: x={:?}, delta={}, ranges={:?}, threshold={}", 
            x, delta, ranges, threshold);

        // Generate FSS keys for IntervalFSS check - one key per test point (simulating dealer exactly)
        let in_modulus = 1u128 << 16; // check_input_bit_length = 16 from CheckConfig
        let out_modulus = 1u128 << 20;
        let num_test_points = test_points_3d.len();
        let mut fss_keys0 = Vec::new();
        let mut fss_keys1 = Vec::new();
        let mut random_pairs = Vec::new();
        
        // Generate FSS keys for each test point (simulating dealer)
        for i in 0..num_test_points {
            // Generate random pair (r0, r1) for this test point using real randomness
            let mut rng = rand::thread_rng();
            let r0 = rng.gen_range(0..in_modulus);
            let r1 = rng.gen_range(0..in_modulus);
            random_pairs.push((r0, r1));
        }
        
        for &(r0, r1) in &random_pairs {
            // Check if distance_threshold + r0 + r1 would wrap around (exact dealer logic)
            let sum = threshold + (r0 + r1) % in_modulus;
            let wraps_around = sum >= in_modulus;
            let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
                // Wrap-around case: interval [distance_threshold+r0+r1 mod modulus, r0+r1]
                // Return 0 in the middle, 1 on left and right
                let interval_start = sum % in_modulus;
                let interval_end = (r0 + r1) % in_modulus;

                let alpha_bits = u128_to_bits_msb(interval_start, 16);
                let beta_bits = u128_to_bits_msb(interval_end, 16);
            
                // For wrap-around: left=1, middle=0, right=1
                let a = RingVec::<1>::new([1], out_modulus); // left
                let b = RingVec::<1>::new([0], out_modulus); // middle
                let c = RingVec::<1>::new([1], out_modulus); // right

                (alpha_bits, beta_bits, a, b, c)
            } else {
                // No wrap-around case: interval [r0+r1, distance_threshold+r0+r1]
                // Return 1 inside interval (distance <= threshold), 0 outside
                let interval_start = (r0 + r1) % in_modulus;
                let interval_end = sum;

                let alpha_bits = u128_to_bits_msb(interval_start, 16);
                let beta_bits = u128_to_bits_msb(interval_end, 16);

                // For no wrap-around: left=0, middle=1, right=0
                let a = RingVec::<1>::new([0], out_modulus); // left
                let b = RingVec::<1>::new([1], out_modulus); // middle
                let c = RingVec::<1>::new([0], out_modulus); // right

                (alpha_bits, beta_bits, a, b, c)
            };
        
            let (fss_key0, fss_key1) = IntervalFSSKey::gen_IntervalFSSKey(
                &alpha_bits,
                &beta_bits,
                &a,
                &b,
                &c,
                out_modulus,
            );
        
            fss_keys0.push(fss_key0);
            fss_keys1.push(fss_key1);
        }

        // Create TCP listener once for all tests
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind TCP listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");
        let share2_clone = share2.clone();
        let share_phase_clone = share_phase.clone();
        let fss_keys1_clone = fss_keys1.clone();
        let random_pairs_clone = random_pairs.clone();
        let test_points_3d_clone = test_points_3d.clone();

        let (sender, receiver) = mpsc::channel();

        // Spawn garbler thread (Party 1) once
        let garbler_handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50)); // Give server time to start
            let stream1 = TcpStream::connect(local_addr).expect("Failed to connect to TCP server");
            
            let mut rng = AesRng::new();
            let reader = BufReader::new(stream1.try_clone().unwrap());
            let writer = BufWriter::new(stream1);
            let mut channel = CommTrackingChannel::new(reader, writer);

            let check_config_garbler = CheckConfig {
                input_bit_length: 16,
                output_bit_length: 20,
                num_dimensions: 3,
                is_garbler_side: true,
                method: CheckMethod::LpIntervalFSS,
            };

            let check_phase_garbler = CheckPhase::new(check_config_garbler, share_phase_clone);
            
            // Test only len1=len2=len3=4 for efficiency
            for (i, test_point) in test_points_3d_clone.iter().enumerate() {
                let prefix1 = test_point[0] >> (8 - 4);
                let prefix2 = test_point[1] >> (8 - 4);
                let prefix3 = test_point[2] >> (8 - 4);
                let query_point = vec![prefix1, prefix2, prefix3];
                
                // Convert query point to Vec<Vec<bool>>
                let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                    .map(|&point| u128_to_bits_msb(point, 4))
                    .collect();
                
                let check_data = CheckData::LpIntervalFSS { 
                    threshold, 
                    fss_key: fss_keys1_clone[i].clone(),
                    random_value: random_pairs_clone[i].1, // r1 for server 1
                };
                
                let result = check_phase_garbler.run_fuzzy_match_check(
                    &share2_clone, // Server 1 gets share2
                    &query_point_bits,
                    &check_data,
                    &mut channel,
                    &mut rng,
                );

                sender.send(result).unwrap();
            }
        });

        // Run evaluator (Party 0) in main thread - accept connection once
        let (stream2, _) = listener.accept().expect("Failed to accept connection");
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream2.try_clone().unwrap());
        let writer = BufWriter::new(stream2);
        let mut channel = CommTrackingChannel::new(reader, writer);

        let check_config_evaluator = CheckConfig {
            input_bit_length: 16,
            output_bit_length: 20,
            num_dimensions: 3,
            is_garbler_side: false,
            method: CheckMethod::LpIntervalFSS,
        };

        let check_phase_evaluator = CheckPhase::new(check_config_evaluator, share_phase.clone());
        
        for (i, test_point) in test_points_3d.iter().enumerate() {
            println!("Testing 3D point {}: {:?}", i, test_point);
            
            // Test only len1=len2=len3=4 for efficiency
            let prefix1 = test_point[0] >> (8 - 4);
            let prefix2 = test_point[1] >> (8 - 4);
            let prefix3 = test_point[2] >> (8 - 4);
            let query_point = vec![prefix1, prefix2, prefix3];
            
            println!("  Testing 3D prefix: [{:#010b}(4), {:#010b}(4), {:#010b}(4)]", 
                prefix1, prefix2, prefix3);

            // Convert query point to Vec<Vec<bool>>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| u128_to_bits_msb(point, 4))
                .collect();
            
            let check_data = CheckData::LpIntervalFSS { 
                threshold, 
                fss_key: fss_keys0[i].clone(),
                random_value: random_pairs[i].0, // r0 for server 0
            };
            
            let evaluator_result = check_phase_evaluator.run_fuzzy_match_check(
                &share1, // Server 0 gets share1
                &query_point_bits,
                &check_data,
                &mut channel,
                &mut rng,
            );

            // Wait for garbler result for this test point
            let garbler_result = receiver.recv().expect("Failed to receive garbler result");

            // Verify results
            assert!(evaluator_result.is_ok(), "Evaluator check phase should succeed");
            assert!(garbler_result.is_ok(), "Garbler check phase should succeed");

            let eval_share = evaluator_result.unwrap();
            let garb_share = garbler_result.unwrap();

            // Reconstruct the final result by adding the shares
            let final_result = (eval_share + garb_share);

            // Calculate the expected result based on FSS IntervalFSS logic (exact dealer matching)
            let (r0, r1) = random_pairs[i];
            
            // Apply the FSS DistanceFSSL2 Lp logic for 3D prefix testing
            let share_modulus = 1u128 << 16;

            // Calculate L2 distance for 3D point using FSS DistanceFSSL2
            let mut total_distance_squared = 0u128;
            for dim in 0..3 {
                let prefix_left = ranges[dim].0 >> (8 - 4);
                let prefix_right = ranges[dim].1 >> (8 - 4);
                let prefix_x = x[dim] >> (8 - 4);
                let prefix = query_point[dim];

                let dim_distance_squared = if prefix < prefix_left || prefix > prefix_right {
                    // Outside range - FSS returns maximum distance
                    (delta).pow(2) + 1 % share_modulus
                } else {
                    let actual_distance = if prefix < prefix_x {
                        let y_padded = prefix << (8 - 4) | ((1u128 << (8 - 4)) - 1);
                        x[dim] - y_padded
                    } else if prefix > prefix_x {
                        let y_padded = prefix << (8 - 4);
                        y_padded - x[dim]
                    } else {
                        0 // Same prefix, no distance
                    };
                    actual_distance.pow(2) % share_modulus
                };
                
                total_distance_squared += dim_distance_squared;
            }
            
            let actual_distance = total_distance_squared % share_modulus;
            
            // FSS was set up for interval [0, threshold + r0 + r1]
            // It returns 1 if masked_distance is in this interval, 0 otherwise
            let should_be_within_threshold = actual_distance <= threshold;

            println!("    3D Prefix i={}, actual_distance={}, r0={}, r1={}, within_threshold={}, result={}", 
                i, actual_distance, r0, r1, should_be_within_threshold, final_result.val);

            // For FSS Lp IntervalFSS check: result should be 1 if masked distance is within FSS interval, 0 otherwise
            if should_be_within_threshold {
                assert_eq!(final_result.val, 1, 
                    "3D Prefix {:?} actual distance {} <= FSS interval end {}, result should be 1", query_point, actual_distance, threshold);
            } else {
                assert_eq!(final_result.val, 0, 
                    "3D Prefix {:?} actual distance {} > FSS interval end {}, result should be 0", query_point, actual_distance, threshold);
            }
        }

        // Wait for garbler thread to complete
        garbler_handle.join().expect("Garbler thread panicked");
    }
}
