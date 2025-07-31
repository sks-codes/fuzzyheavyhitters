//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckData, CheckMethod};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::fuzzy_match::dealer::{FssKeyBatch, DealerSignal};
use crate::fuzzy_match::client::{Client, ClientConfig};
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
    /// Number of clients participating in the protocol
    pub num_clients: usize,
}

/// Main protocol structure that encapsulates the entire fuzzy heavy hitters protocol
#[derive(Clone)]
pub struct FuzzyHeavyHittersProtocol {
    config: ProtocolConfig,
    share_phase: SharePhase,
    check_phase: CheckPhase,
    threshold_phase: ThresholdPhase,
    is_server1: bool,
}

impl FuzzyHeavyHittersProtocol {
    /// Create a new protocol instance
    pub fn new(config: ProtocolConfig, is_server1: bool) -> Self {
        let share_phase = SharePhase::new(config.share_config.clone());
        let check_phase = CheckPhase::new(config.check_config.clone(), share_phase.clone());
        let threshold_phase = ThresholdPhase::new(config.threshold_config.clone());
        Self {
            config,
            share_phase,
            check_phase,
            threshold_phase,
            is_server1,
        }
    }

    pub fn receive_client_shares(
        &self,
        client_channel: &mut CommTrackingChannel,
    ) -> Result<(Vec<SharedRange>), String> {
        // Receive shares using binary deserialization
        let mut len_bytes = [0u8; 8];
        client_channel.read_bytes(&mut len_bytes)
            .map_err(|e| format!("Failed to read length from client: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;
    
        let mut shares_data = vec![0u8; len];
        client_channel.read_bytes(&mut shares_data)
            .map_err(|e| format!("Failed to receive shares from client: {}", e))?;
    
        let shares: Vec<SharedRange> = bincode::deserialize(&shares_data)
            .map_err(|e| format!("Failed to deserialize shares: {}", e))?;

        Ok(shares)
    }

    /// Run the protocol as a specific server (0 or 1)
    pub fn run_server_known_dictionary(
        &self,
        client_shares: &[SharedRange],
        query_points: &[Vec<u128>],
        other_server_channel: &mut CommTrackingChannel,
        dealer_channel: &mut CommTrackingChannel,
    ) -> Result<Vec<bool>, String> {
        let mut rng = AesRng::new();
        
        // Convert query points from u128 to Vec<Vec<bool>>
        let query_point_sets: Vec<Vec<Vec<bool>>> = query_points.iter()
            .map(|query_point| {
                query_point.iter()
                    .map(|&point| {
                        u128_to_bits_msb(point, self.config.share_config.input_bit_length)
                    })
                    .collect()
            })
            .collect();
        
        // Use batch_test_prefix_sets_threshold to process all query points
        let final_results = self.batch_test_prefix_sets_threshold(
            &query_point_sets,
            client_shares,
            other_server_channel,
            dealer_channel,
            &mut rng,
        )?;

        // Shutdown dealer at the end
        self.shutdown_dealer(dealer_channel)?;

        Ok(final_results)
    }

    pub fn run_server_unknown_dictionary(
        &self,
        client_shares_list: &[SharedRange],
        is_server1: bool,
        other_server_channel: &mut CommTrackingChannel,
        dealer_channel: &mut CommTrackingChannel,
    ) -> Result<Vec<Vec<u128>>, String> {
        let mut rng = AesRng::new();
        
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
            let exceeds_threshold_results = self.batch_test_prefix_sets_threshold(
                &candidate_prefix_sets,
                client_shares_list,
                other_server_channel,
                dealer_channel,
                &mut rng,
            )?;

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

        // Shutdown dealer at the end
        self.shutdown_dealer(dealer_channel)?;

        Ok(final_heavy_hitters)
    }

    fn batch_test_prefix_sets_threshold(
        &self,
        prefix_sets: &[Vec<Vec<bool>>],
        client_shares: &[SharedRange],
        other_server_channel: &mut CommTrackingChannel,
        dealer_channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, String> {
        let mut server_bits = Vec::new();

        // Process each prefix set and collect our server's results
        for (prefix_idx, prefix_set) in prefix_sets.iter().enumerate() {
            // Handle CheckData - get from dealer if using LpIntervalFSS, otherwise create locally
            let check_data_list = if matches!(self.config.check_config.method, CheckMethod::LpIntervalFSS) {
                // Request check FSS keys from dealer
                let batch = self.request_dealer(DealerSignal::RequestCheckKeys, dealer_channel)?;
                
                if batch.keys.len() < client_shares.len() {
                    return Err(format!("Dealer provided {} keys but {} are needed", 
                                     batch.keys.len(), client_shares.len()));
                }
                
                // Create CheckData for each client share using corresponding FSS key
                let mut check_data_vec = Vec::new();
                for i in 0..client_shares.len() {
                    check_data_vec.push(CheckData::LpIntervalFSS {
                        threshold: self.config.delta,
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
                    CheckMethod::LpIntervalFSS => unreachable!(), // Already handled above
                };
                vec![single_check_data; client_shares.len()]
            };

            // Run check phase for all client shares with this prefix set
            let mut match_results = Vec::new();
            
            for (i, share) in client_shares.iter().enumerate() {
                let result = self.check_phase.run_fuzzy_match_check(
                    share,
                    prefix_set,
                    &check_data_list[i], // Use the corresponding CheckData for this client share
                    other_server_channel,
                    rng,
                ).map_err(|e| format!("Check phase failed: {:?}", e))?;
                
                match_results.push(result);
            }

            // Handle ThresholdData - get from dealer if using IntervalFSS, otherwise use provided data
            let threshold_data_to_use = if matches!(self.config.threshold_config.method, ThresholdMethod::IntervalFSS) {
                // Request threshold FSS keys from dealer
                let batch = self.request_dealer(DealerSignal::RequestThresholdKeys, dealer_channel)?;
                
                if batch.keys.is_empty() {
                    return Err("Dealer provided no threshold keys".to_string());
                }
                
                // Use the first key for threshold comparison (typically only need one per query)
                ThresholdData::IntervalFSS {
                    fss_key: batch.keys[0].clone(),
                    random_value: batch.random_values[0],
                }
            } else {
                // Use garbled circuits threshold data
                ThresholdData::GarbledCircuits
            };

            // Run threshold phase to check if results exceed threshold
            let server_bit = self.threshold_phase.compare_with_threshold(
                &match_results,
                self.config.threshold,
                &threshold_data_to_use,
                other_server_channel,
                rng,
            ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

            server_bits.push(server_bit);
        }

        println!("Server {} computed bits: {:?}", if self.is_server1 { 1 } else { 0 }, server_bits);

        // Batch exchange bits between servers to get final results
        let final_results = if self.is_server1 {
            // Server 1: receive bits from server 0, then send our bits
            let server0_bits = receive_bool_vec(other_server_channel)
                .map_err(|e| format!("Failed to receive bits from server 0: {:?}", e))?;
            send_bool_vec(other_server_channel, &server_bits)
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
            send_bool_vec(other_server_channel, &server_bits)
                .map_err(|e| format!("Failed to send bits to server 1: {:?}", e))?;
            let server1_bits = receive_bool_vec(other_server_channel)
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

    /// Request FSS keys from dealer
    fn request_dealer(&self, signal: DealerSignal, dealer_channel: &mut CommTrackingChannel) -> Result<FssKeyBatch, String> {
        // Send request signal to dealer
        let signal_data = bincode::serialize(&signal)
            .map_err(|e| format!("Failed to serialize dealer request: {}", e))?;
        
        // Write length first, then data
        let len_bytes = (signal_data.len() as u64).to_le_bytes();
        dealer_channel.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to write request length: {}", e))?;
        dealer_channel.write_bytes(&signal_data)
            .map_err(|e| format!("Failed to write request data: {}", e))?;
        dealer_channel.flush()
            .map_err(|e| format!("Failed to flush request: {}", e))?;
        
        // Receive FSS key batch from dealer
        let mut len_bytes = [0u8; 8];
        dealer_channel.read_bytes(&mut len_bytes)
            .map_err(|e| format!("Failed to read key batch length: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;
        
        let mut batch_data = vec![0u8; len];
        dealer_channel.read_bytes(&mut batch_data)
            .map_err(|e| format!("Failed to read key batch data: {}", e))?;
        
        let batch: FssKeyBatch = bincode::deserialize(&batch_data)
            .map_err(|e| format!("Failed to deserialize key batch: {}", e))?;
        
        Ok(batch)
    }

    /// Send shutdown signal to dealer
    fn shutdown_dealer(&self, dealer_channel: &mut CommTrackingChannel) -> Result<(), String> {
        self.request_dealer(DealerSignal::Shutdown, dealer_channel)
            .map(|_| ()) // Ignore response for shutdown
            .or_else(|e| {
                println!("Warning: Failed to send shutdown signal to dealer: {}", e);
                Ok(()) // Don't fail on shutdown errors
            })
    }
}
