//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckData, CheckMethod};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::data_structures::modint::ModInt;
use crate::data_structures::payload::RingVec;
use crate::fss::interval::IntervalFSSKey;
use crate::util::{send_bool_vec, receive_bool_vec, u128_to_bits};
use scuttlebutt::{AesRng, Channel};
use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
use std::thread::current;
use std::sync::mpsc;
use serde::{Deserialize, Serialize};
use rand::Rng;

/// FSS key pair batch for check phase
#[derive(Clone, Debug)]
pub struct FssKeyBatch {
    pub keys0: Vec<IntervalFSSKey<1>>,
    pub keys1: Vec<IntervalFSSKey<1>>, 
    pub random_pairs: Vec<(u128, u128)>,
}

/// Configuration for the entire fuzzy heavy hitters protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Configuration for the share phase
    pub share_config: ShareConfig,
    /// Configuration for the check phase
    pub check_config: CheckConfig,
    /// Configuration for the threshold phase
    pub threshold_config: ThresholdConfig,
    /// The threshold value for heavy hitters detection
    pub threshold: u128,
    /// The delta value for fuzzy matching (L-infinity distance)
    pub delta: u128,
}

/// Main protocol structure that encapsulates the entire fuzzy heavy hitters protocol
#[derive(Clone)]
pub struct FuzzyHeavyHittersProtocol {
    config: ProtocolConfig,
    share_phase: SharePhase,
}

impl FuzzyHeavyHittersProtocol {
    /// Create a new protocol instance
    pub fn new(config: ProtocolConfig) -> Self {
        let share_phase = SharePhase::new(config.share_config.clone());
        
        Self {
            config,
            share_phase,
        }
    }

    /// Generate shares for all client points
    pub fn generate_client_shares(&self, client_points: &[Vec<u128>]) 
        -> Result<(Vec<SharedRange>, Vec<SharedRange>), String> {
        let mut shares_server0 = Vec::new();
        let mut shares_server1 = Vec::new();

        for client_point in client_points {
            let (share0, share1) = self.share_phase.share_range(client_point, self.config.delta)
                .map_err(|e| format!("Failed to generate shares: {:?}", e))?;
            
            shares_server0.push(share0);
            shares_server1.push(share1);
        }

        Ok((shares_server0, shares_server1))
    }

    /// Run the protocol as a specific server (0 or 1)
    pub fn run_server_known_dictionary(
        &self,
        server_id: u8,
        client_shares: &[SharedRange],
        query_points: &[Vec<u128>],
        threshold_data_list: &[ThresholdData],
        check_data_list: &[CheckData],
        stream: UnixStream,
    ) -> Result<Vec<bool>, String> {
        let mut channel = Channel::new(BufReader::new(stream.try_clone().unwrap()), BufWriter::new(stream));
        let mut rng = AesRng::new();
        
        let share_phase = SharePhase::new(self.config.share_config.clone());
        let check_phase = CheckPhase::new(self.config.check_config.clone(), share_phase.clone());
        let threshold_phase = ThresholdPhase::new(self.config.threshold_config.clone());
        
        let mut server_bits = Vec::new();
        
        for (query_idx, query_point) in query_points.iter().enumerate() {
            // Run check phase for all client shares
            let mut match_results = Vec::new();
            
            // Convert query point from u128 to Vec<bool>
            let query_point_bits: Vec<Vec<bool>> = query_point.iter()
                .map(|&point| {
                    let mut key_bits = u128_to_bits(point, self.config.share_config.input_bit_length);
                    key_bits.reverse(); // Reverse bits for server 1 to match server 0's order
                    key_bits
                })
                .collect();
            
            for share in client_shares {
                let result = check_phase.run_fuzzy_match_check(
                    share,
                    &query_point_bits,
                    &check_data_list[query_idx],
                    &mut channel,
                    &mut rng,
                ).map_err(|e| format!("Check phase failed: {:?}", e))?;
                
                match_results.push(result);
            }

            // Run threshold phase
            let server_bit = threshold_phase.compare_with_threshold(
                &match_results,
                self.config.threshold,
                &threshold_data_list[query_idx],
                &mut channel,
                &mut rng,
            ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;
            
            server_bits.push(server_bit);
        }

        // Exchange threshold bits between servers
        let final_results = if server_id == 0 {
            // Server 0: send bits first, then receive
            send_bool_vec(&mut channel, &server_bits).map_err(|e| format!("Failed to send bits to server 1: {:?}", e))?;
            let server1_bits = receive_bool_vec(&mut channel).map_err(|e| format!("Failed to receive bits from server 1: {:?}", e))?;
            
            // Compute final results (XOR the bits)
            server_bits.iter()
                .zip(server1_bits.iter())
                .map(|(bit0, bit1)| bit0 ^ bit1)
                .collect()
        } else {
            // Server 1: receive bits first, then send
            let server0_bits = receive_bool_vec(&mut channel).map_err(|e| format!("Failed to receive bits from server 0: {:?}", e))?;
            send_bool_vec(&mut channel, &server_bits).map_err(|e| format!("Failed to send bits to server 0: {:?}", e))?;
            
            // Compute final results (XOR the bits)
            server0_bits.iter()
                .zip(server_bits.iter())
                .map(|(bit0, bit1)| bit0 ^ bit1)
                .collect()
        };

        Ok(final_results)
    }

    /// Run the protocol for unknown dictionary case using binary search approach
    /// This method finds the "frontier" of prefixes that exceed the threshold
    /// Uses iterative extension: start with empty prefixes, then extend each heavy hitter by one bit at a time
    /// 
    /// # Parameters
    /// * `client_shares_list` - Client shares for this server
    /// * `threshold_data_list` - Threshold data for this server
    /// * `stream` - Communication stream between servers
    /// * `is_server1` - True if this is server 1 (garbler), false if server 0 (evaluator)
    /// * `fss_dealer_receiver` - Optional receiver to get FSS key batches from dealer
    /// * `fss_dealer_shutdown` - Optional sender to shutdown the dealer when done
    pub fn run_server_unknown_dictionary(
        &self,
        client_shares_list: &[SharedRange],
        threshold_data_list: &[ThresholdData],
        stream: UnixStream,
        is_server1: bool,
        fss_dealer_receiver: Option<mpsc::Receiver<FssKeyBatch>>,
        fss_dealer_shutdown: Option<mpsc::Sender<()>>,
    ) -> Result<Vec<Vec<u128>>, String> {
        // Helper closure to shutdown dealer on both success and error
        let shutdown_dealer = |is_server1: bool, sender: Option<mpsc::Sender<()>>| {
            if !is_server1 {
                if let Some(shutdown_sender) = sender {
                    if let Err(_) = shutdown_sender.send(()) {
                        println!("Warning: Failed to send shutdown signal to dealer");
                    } else {
                        println!("Server 0: Sent shutdown signal to FSS dealer");
                    }
                }
            }
        };

        let mut channel = Channel::new(BufReader::new(stream.try_clone().unwrap()), BufWriter::new(stream));
        let mut rng = AesRng::new();
        
        let share_phase = SharePhase::new(self.config.share_config.clone());
        let check_phase = CheckPhase::new(self.config.check_config.clone(), share_phase.clone());
        let threshold_phase = ThresholdPhase::new(self.config.threshold_config.clone());
        
        let max_bit_length = self.config.share_config.input_bit_length;
        let dimension = self.config.share_config.dimension;

        // Initialize with empty prefix for each dimension
        let mut current_heavy_hitters = vec![vec![vec![]; dimension]];
        let mut check_data_count = 0;
        let mut threshold_data_count = 0;

        // Iteratively extend prefixes until we reach maximum length
        while !current_heavy_hitters.is_empty() {
            let mut candidate_prefix_sets = Vec::new();

            // Collect all potential next heavy hitters
            for prefix_set in &current_heavy_hitters {
                // Try extending each dimension that hasn't reached max length
                for dim in 0..dimension {
                    if prefix_set[dim].len() < max_bit_length {
                        // Try both 0 and 1 for this dimension
                        for bit_value in [false, true] {
                            let mut extended_prefix_set = prefix_set.clone();
                            extended_prefix_set[dim].push(bit_value);
                            candidate_prefix_sets.push(extended_prefix_set);
                        }
                        break;
                    }
                }
            }

            println!("Candidate prefixes to test: {:?}", candidate_prefix_sets);

            if candidate_prefix_sets.is_empty() {
                // No more prefixes to extend, we are done
                break;
            }

            // Batch process all candidates
            let exceeds_threshold_results = match self.batch_test_prefix_sets_threshold(
                &candidate_prefix_sets,
                client_shares_list,
                &threshold_data_list[0], // For simplicity, using same threshold data for all tests
                &check_phase,
                &threshold_phase,
                &mut channel,
                &mut rng,
                is_server1,
                &fss_dealer_receiver,
            ) {
                Ok(results) => results,
                Err(e) => {
                    shutdown_dealer(is_server1, fss_dealer_shutdown);
                    return Err(e);
                }
            };

            // Collect the next heavy hitters based on results
            let mut next_heavy_hitters = Vec::new();
            for (candidate, exceeds_threshold) in candidate_prefix_sets.iter().zip(exceeds_threshold_results.iter()) {
                if *exceeds_threshold {
                    next_heavy_hitters.push(candidate.clone());
                }
            }

            println!("Next heavy hitters found: {:?}", next_heavy_hitters);

            current_heavy_hitters = next_heavy_hitters;
            check_data_count += candidate_prefix_sets.len() * client_shares_list.len();
            threshold_data_count += candidate_prefix_sets.len();
        }

        let final_heavy_hitters = current_heavy_hitters.into_iter()
            .map(|prefix_set| {
                // Convert each prefix set back to u128 representation
                prefix_set.iter()
                    .map(|bits| {
                        bits.iter().fold(0u128, |acc, &bit| (acc << 1) | if bit { 1 } else { 0 })
                    })
                    .collect::<Vec<u128>>()
            })
            .collect();

        // Shutdown the dealer if we have a shutdown sender (only server 0 should do this)
        shutdown_dealer(is_server1, fss_dealer_shutdown);

        Ok(final_heavy_hitters)
    }

    /// Batch test if multiple prefix sets exceed the threshold when evaluated against all client shares
    /// This processes all candidates in a batch and exchanges results efficiently
    fn batch_test_prefix_sets_threshold(
        &self,
        prefix_sets: &[Vec<Vec<bool>>],
        client_shares: &[SharedRange],
        threshold_data: &ThresholdData,
        check_phase: &CheckPhase,
        threshold_phase: &ThresholdPhase,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
        is_server1: bool,
        fss_dealer_receiver: &Option<mpsc::Receiver<FssKeyBatch>>,
    ) -> Result<Vec<bool>, String> {
        let mut server_bits = Vec::new();

        // Process each prefix set and collect our server's results
        for prefix_set in prefix_sets {
            // Get FSS keys from dealer if using LpIntervalFSS check method
            let check_data = if let Some(receiver) = fss_dealer_receiver {
                // Get a batch of FSS keys from the dealer
                let batch = receiver.recv()
                    .map_err(|_| "Failed to receive FSS keys from dealer")?;
                
                if batch.keys0.len() < client_shares.len() {
                    return Err(format!("Dealer provided {} keys but {} are needed", 
                                     batch.keys0.len(), client_shares.len()));
                }
                
                // For now, we'll use the first key from the batch
                // In a real implementation, you'd want to distribute the keys properly
                CheckData::LpIntervalFSS {
                    threshold: self.config.delta, // Use configured delta as threshold
                    fss_key: batch.keys0[0].clone(),
                    random_value: batch.random_pairs[0].0,
                }
            } else {
                match self.config.check_config.method {
                    CheckMethod::Linf => CheckData::Linf,
                    CheckMethod::LpGarbledCircuits => CheckData::LpGarbledCircuits {
                        threshold: self.config.delta,
                    },
                    CheckMethod::LpIntervalFSS => return Err("FSS dealer is required for LpIntervalFSS check method".to_string()),
                }
            };
            
            // Run check phase for all client shares with this prefix set
            let mut match_results = Vec::new();
            
            for share in client_shares {
                let result = check_phase.run_fuzzy_match_check(
                    share,
                    prefix_set,
                    &check_data,
                    channel,
                    rng,
                ).map_err(|e| format!("Check phase failed: {:?}", e))?;
                
                match_results.push(result);
            }

            // Run threshold phase to check if results exceed threshold
            let server_bit = threshold_phase.compare_with_threshold(
                &match_results,
                self.config.threshold,
                threshold_data,
                channel,
                rng,
            ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

            server_bits.push(server_bit);
        }

        // Batch exchange bits between servers to get final results
        let final_results = if is_server1 {
            // Server 1: receive bits from server 0, then send our bits
            let server0_bits = receive_bool_vec(channel)
                .map_err(|e| format!("Failed to receive bits from server 0: {:?}", e))?;
            send_bool_vec(channel, &server_bits)
                .map_err(|e| format!("Failed to send bits to server 0: {:?}", e))?;
            
            if server0_bits.len() != server_bits.len() {
                return Err(format!("Mismatch in batch size: expected {}, got {}", server_bits.len(), server0_bits.len()));
            }
            
            server0_bits.iter()
                .zip(server_bits.iter())
                .map(|(bit0, bit1)| bit0 ^ bit1)
                .collect()
        } else {
            // Server 0: send our bits, then receive from server 1
            send_bool_vec(channel, &server_bits)
                .map_err(|e| format!("Failed to send bits to server 1: {:?}", e))?;
            let server1_bits = receive_bool_vec(channel)
                .map_err(|e| format!("Failed to receive bits from server 1: {:?}", e))?;
            
            if server1_bits.len() != server_bits.len() {
                return Err(format!("Mismatch in batch size: expected {}, got {}", server_bits.len(), server1_bits.len()));
            }
            
            server_bits.iter()
                .zip(server1_bits.iter())
                .map(|(bit0, bit1)| bit0 ^ bit1)
                .collect()
        };

        Ok(final_results)
    }

    /// Helper method to send a single bit
    fn send_single_bit(&self, channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>, bit: bool) -> Result<(), String> {
        send_bool_vec(channel, &[bit]).map_err(|e| format!("Failed to send bit: {:?}", e))
    }

    /// Helper method to receive a single bit
    fn receive_single_bit(&self, channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>) -> Result<bool, String> {
        let bits = receive_bool_vec(channel).map_err(|e| format!("Failed to receive bit: {:?}", e))?;
        if bits.len() != 1 {
            return Err(format!("Expected 1 bit, got {}", bits.len()));
        }
        Ok(bits[0])
    }
}

/// Generate FSS keys for interval FSS threshold comparison
/// This simulates a trusted dealer generating FSS keys
pub fn generate_fss_keys_for_threshold(
    threshold: u128,
    bit_length: usize,
    num_queries: usize,
) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
    let modulus = 1u128 << bit_length;
    
    let mut keys_server0 = Vec::new();
    let mut keys_server1 = Vec::new();
    let mut random_pairs = Vec::new();
    
    for _ in 0..num_queries {
        // Generate random pair (r0, r1) for this query using standard rand
        let mut std_rng = rand::thread_rng();
        let r0 = std_rng.gen_range(0..modulus);
        let r1 = std_rng.gen_range(0..modulus);
        random_pairs.push((r0, r1));
        
        // Check if threshold + r0 + r1 would wrap around
        let sum = threshold + r0 + r1;
        let wraps_around = sum >= modulus;
        let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
            // Wrap-around case: interval [threshold+r0+r1 mod modulus, r0+r1]
            // Return 1 in the middle, 0 on left and right
            let interval_start = sum % modulus;
            let interval_end = (r0 + r1) % modulus;
            
            let alpha_bits: Vec<bool> = (0..bit_length)
                .map(|i| ((interval_start >> i) & 1) == 1)
                .collect();
            let beta_bits: Vec<bool> = (0..bit_length)
                .map(|i| ((interval_end >> i) & 1) == 1)
                .collect();
            
            // For wrap-around: left=0, middle=1, right=0
            let a = RingVec::<1>::new([0], 2); // left
            let b = RingVec::<1>::new([1], 2); // middle
            let c = RingVec::<1>::new([0], 2); // right
            
            (alpha_bits, beta_bits, a, b, c)
        } else {
            // No wrap-around case: interval [r0+r1, threshold+r0+r1]
            // Return 1 on left and right, 0 in the middle
            let interval_start = (r0 + r1) % modulus;
            let interval_end = sum;
            
            let alpha_bits: Vec<bool> = (0..bit_length)
                .map(|i| ((interval_start >> i) & 1) == 1)
                .collect();
            let beta_bits: Vec<bool> = (0..bit_length)
                .map(|i| ((interval_end >> i) & 1) == 1)
                .collect();
            
            // For no wrap-around: left=1, middle=0, right=1
            let a = RingVec::<1>::new([1], 2); // left
            let b = RingVec::<1>::new([0], 2); // middle
            let c = RingVec::<1>::new([1], 2); // right
            
            (alpha_bits, beta_bits, a, b, c)
        };
        
        let (key0, key1) = IntervalFSSKey::gen_IntervalFSSKey(
            &alpha_bits,
            &beta_bits,
            &a,
            &b,
            &c,
            modulus,
        );
        
        keys_server0.push(key0);
        keys_server1.push(key1);
    }
    
    Ok((keys_server0, keys_server1, random_pairs))
}

/// Generate FSS keys for check phase comparison (Lp distance with IntervalFSS)
/// This simulates a trusted dealer generating FSS keys for distance threshold comparison
pub fn generate_fss_keys_for_check(
    distance_threshold: u128,
    input_bit_length: usize,
    output_bit_length: usize,
    num_checks: usize,
) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
    let in_modulus = 1u128 << input_bit_length;
    let out_modulus = 1u128 << output_bit_length;
    
    let mut keys_server0 = Vec::new();
    let mut keys_server1 = Vec::new();
    let mut random_pairs = Vec::new();
    
    for _ in 0..num_checks {
        // Generate random pair (r0, r1) for this check using standard rand
        let mut std_rng = rand::thread_rng();
        let r0 = std_rng.gen_range(0..in_modulus);
        let r1 = std_rng.gen_range(0..in_modulus);
        random_pairs.push((r0, r1));
        
        // Check if distance_threshold + r0 + r1 would wrap around
        let sum = distance_threshold + r0 + r1;
        let wraps_around = sum >= in_modulus;
        let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
            // Wrap-around case: interval [distance_threshold+r0+r1 mod modulus, r0+r1]
            // Return 0 in the middle, 1 on left and right
            let interval_start = sum % in_modulus;
            let interval_end = (r0 + r1) % in_modulus;
            
            let alpha_bits: Vec<bool> = (0..input_bit_length)
                .map(|i| ((interval_start >> i) & 1) == 1)
                .collect();
            let beta_bits: Vec<bool> = (0..input_bit_length)
                .map(|i| ((interval_end >> i) & 1) == 1)
                .collect();
            
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

            let alpha_bits: Vec<bool> = (0..input_bit_length)
                .map(|i| ((interval_start >> i) & 1) == 1)
                .collect();
            let beta_bits: Vec<bool> = (0..input_bit_length)
                .map(|i| ((interval_end >> i) & 1) == 1)
                .collect();
            
            // For no wrap-around: left=0, middle=1, right=0
            let a = RingVec::<1>::new([0], out_modulus); // left
            let b = RingVec::<1>::new([1], out_modulus); // middle
            let c = RingVec::<1>::new([0], out_modulus); // right

            (alpha_bits, beta_bits, a, b, c)
        };
        
        let (key0, key1) = IntervalFSSKey::gen_IntervalFSSKey(
            &alpha_bits,
            &beta_bits,
            &a,
            &b,
            &c,
            out_modulus,
        );
        
        keys_server0.push(key0);
        keys_server1.push(key1);
    }
    
    Ok((keys_server0, keys_server1, random_pairs))
}
