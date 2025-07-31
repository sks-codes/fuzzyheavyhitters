//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckData, CheckMethod};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::fuzzy_match::dealer::{FssKeyBatch, DealerSignal};
use crate::data_structures::modint::ModInt;
use crate::util::{send_bool_vec, receive_bool_vec, u128_to_bits, u128_to_bits_msb};
use crate::channel::CommTrackingChannel;
use scuttlebutt::AesRng;
use std::thread::current;
use std::time::Instant;
use std::sync::mpsc;
use serde::{Deserialize, Serialize};

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
        mut channel: CommTrackingChannel,
    ) -> Result<Vec<bool>, String> {
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
                    u128_to_bits_msb(point, self.config.share_config.input_bit_length)
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
    /// * `fss_dealer_signal_sender` - Optional sender to signal the dealer for keys or shutdown
    pub fn run_server_unknown_dictionary(
        &self,
        client_shares_list: &[SharedRange],
        threshold_data_list: &[ThresholdData],
        mut channel: CommTrackingChannel,
        is_server1: bool,
        fss_dealer_receiver: Option<mpsc::Receiver<FssKeyBatch>>,
        fss_dealer_signal_sender: Option<mpsc::Sender<DealerSignal>>,
    ) -> Result<Vec<Vec<u128>>, String> {
        // Helper closure to shutdown dealer on both success and error
        let shutdown_dealer = |is_server1: bool, sender: Option<mpsc::Sender<DealerSignal>>| {
            if !is_server1 {
                if let Some(signal_sender) = sender {
                    if let Err(_) = signal_sender.send(DealerSignal::Shutdown) {
                        println!("Warning: Failed to send shutdown signal to dealer");
                    } else {
                        println!("Server 0: Sent shutdown signal to FSS dealer");
                    }
                }
            }
        };

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
                &fss_dealer_signal_sender,
            ) {
                Ok(results) => results,
                Err(e) => {
                    shutdown_dealer(is_server1, fss_dealer_signal_sender);
                    return Err(e);
                }
            };

            println!("Exceeds threshold results: {:?}", exceeds_threshold_results);

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
        shutdown_dealer(is_server1, fss_dealer_signal_sender);

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
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
        is_server1: bool,
        fss_dealer_receiver: &Option<mpsc::Receiver<FssKeyBatch>>,
        fss_dealer_signal_sender: &Option<mpsc::Sender<DealerSignal>>,
    ) -> Result<Vec<bool>, String> {
        let mut server_bits = Vec::new();

        // Process each prefix set and collect our server's results
        for (prefix_idx, prefix_set) in prefix_sets.iter().enumerate() {
            // Get FSS keys from dealer if using LpIntervalFSS check method
            let check_data_list = if let Some(receiver) = fss_dealer_receiver {
                
                // Only server 0 should request keys from dealer (as per dealer coordinator model)
                if !is_server1 {
                    if let Some(signal_sender) = fss_dealer_signal_sender {
                        signal_sender.send(DealerSignal::RequestKeys)
                            .map_err(|_| "Failed to send key request signal to dealer")?;
                    }
                }
                
                // Get a batch of FSS keys from the dealer
                let batch = receiver.recv()
                    .map_err(|_| "Failed to receive FSS keys from dealer")?;
                
                if batch.keys.len() < client_shares.len() {
                    return Err(format!("Dealer provided {} keys but {} are needed", 
                                     batch.keys.len(), client_shares.len()));
                }
                
                // Create a CheckData for each client share using corresponding FSS key
                let mut check_data_vec = Vec::new();
                for i in 0..client_shares.len() {
                    check_data_vec.push(CheckData::LpIntervalFSS {
                        threshold: self.config.delta, // Use configured delta as threshold
                        fss_key: batch.keys[i].clone(),
                        random_value: batch.random_values[i],
                    });
                }
                check_data_vec
            } else {
                // Create the same CheckData for all client shares when not using FSS dealer
                let single_check_data = match self.config.check_config.method {
                    CheckMethod::Linf => CheckData::Linf,
                    CheckMethod::LpGarbledCircuits => CheckData::LpGarbledCircuits {
                        threshold: self.config.delta,
                    },
                    CheckMethod::LpIntervalFSS => return Err("FSS dealer is required for LpIntervalFSS check method".to_string()),
                };
                vec![single_check_data; client_shares.len()]
            };

            // Run check phase for all client shares with this prefix set
            let mut match_results = Vec::new();
            
            for (i, share) in client_shares.iter().enumerate() {
                let result = check_phase.run_fuzzy_match_check(
                    share,
                    prefix_set,
                    &check_data_list[i], // Use the corresponding CheckData for this client share
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

        println!("Server {} computed bits: {:?}", if is_server1 { 1 } else { 0 }, server_bits);

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
    fn send_single_bit(&self, channel: &mut CommTrackingChannel, bit: bool) -> Result<(), String> {
        send_bool_vec(channel, &[bit]).map_err(|e| format!("Failed to send bit: {:?}", e))
    }

    /// Helper method to receive a single bit
    fn receive_single_bit(&self, channel: &mut CommTrackingChannel) -> Result<bool, String> {
        let bits = receive_bool_vec(channel).map_err(|e| format!("Failed to receive bit: {:?}", e))?;
        if bits.len() != 1 {
            return Err(format!("Expected 1 bit, got {}", bits.len()));
        }
        Ok(bits[0])
    }
}
