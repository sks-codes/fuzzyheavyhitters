//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, SharedRange, DistanceMetric};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckData, CheckMethod, CheckProperty};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::fuzzy_match::dealer::{FssKeyBatch, DpfKeyBatch, DealerSignal};
use crate::util::{send_bool_vec, receive_bool_vec, u128_to_bits_msb, get_distance_threshold};
use crate::channel::CommTrackingChannel;
use scuttlebutt::{AbstractChannel, AesRng};
use std::convert::TryInto;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;

/// Configuration for the entire fuzzy heavy hitters protocol
#[derive(Debug, Clone)]
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
    ) -> Result<Vec<SharedRange>, String> {
        // Receive shares using custom serialization
        let mut len_bytes = [0u8; 8];
        client_channel.read_bytes(&mut len_bytes)
            .map_err(|e| format!("Failed to read length from client: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;
        let mut shares_data = vec![0u8; len];
        client_channel.read_bytes(&mut shares_data)
            .map_err(|e| format!("Failed to receive shares from client: {}", e))?;

        // Custom deserialization for Vec<SharedRange>
        let mut bytes = &shares_data[..];
        if bytes.len() < 4 {
            return Err("Too short for Vec<SharedRange> length".to_string());
        }
        let mut offset = 0;
        let count = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;
        let modulus = 1u128 << self.config.share_config.h2;
        let mut shares = Vec::with_capacity(count);
        for _ in 0..count {
            let (share, share_used) = SharedRange::from_bytes(&bytes[offset..], modulus)?;
            shares.push(share);
            offset += share_used;
        }
        Ok(shares)
    }

    /// Parallel version of run_server_known_dictionary using multiple channels for both dealer and other server
    pub fn run_server_known_dictionary_parallel(
        &self,
        client_shares: &[SharedRange],
        query_points: &[Vec<u128>],
        dealer_channels: &[Arc<Mutex<CommTrackingChannel>>],
        other_server_channels: &[Arc<Mutex<CommTrackingChannel>>],
    ) -> Result<Vec<bool>, String> {
        // Convert query points from u128 to Vec<Vec<bool>>
        let query_point_sets: Vec<Vec<Vec<bool>>> = query_points.iter()
            .map(|query_point| {
                query_point.iter()
                    .map(|&point| {
                        u128_to_bits_msb(point, self.config.share_config.h1)
                    })
                    .collect()
            })
            .collect();

        // Use parallel batch processing with both dealer and server channels
        println!("Using full parallel processing with {} dealer channels and {} server channels", 
                 dealer_channels.len(), other_server_channels.len());
        let final_results = self.batch_test_prefix_sets_threshold_parallel(
            &query_point_sets,
            client_shares,
            dealer_channels,
            other_server_channels,
        )?;

        // Shutdown dealers using all dealer channels
        for (i, dealer_channel) in dealer_channels.iter().enumerate() {
            let mut locked_dealer_channel = dealer_channel.lock().map_err(|e| format!("Failed to lock dealer channel {}: {}", i, e))?;
            shutdown_dealer(&mut *locked_dealer_channel)?;
        }

        Ok(final_results)
    }

    /// Parallel version of run_server_unknown_dictionary using multiple channels
    pub fn run_server_unknown_dictionary_parallel(
        &self,
        client_shares_list: &[SharedRange],
        is_server1: bool,
        dealer_channels: &[Arc<Mutex<CommTrackingChannel>>],
        other_server_channels: &[Arc<Mutex<CommTrackingChannel>>],
    ) -> Result<Vec<Vec<u128>>, String> {
        let max_bit_length = self.config.share_config.h1;
        let dimension = self.config.share_config.d;

        // Initialize with empty prefix for each dimension
        let mut current_heavy_hitters = vec![vec![vec![]; dimension]];
        let mut check_data_count = 0;
        let mut threshold_data_count = 0;

        // Iteratively extend prefixes until we reach maximum length
        while !current_heavy_hitters.is_empty() {
            let mut candidate_prefix_sets = Vec::new();

            use std::time::Instant;
            let start = Instant::now();
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
            println!("Extended prefixes in {:?}, count: {}", start.elapsed(), candidate_prefix_sets.len());

            if candidate_prefix_sets.is_empty() {
                // No more prefixes to extend, we are done
                break;
            }

            // Use parallel batch processing if channels are available
            let start = Instant::now();
            let exceeds_threshold_results = {
                println!("Processing {} candidates with {} dealer channels and {} server channels", 
                         candidate_prefix_sets.len(), dealer_channels.len(), other_server_channels.len());
                self.batch_test_prefix_sets_threshold_parallel(
                    &candidate_prefix_sets,
                    client_shares_list,
                    dealer_channels,
                    other_server_channels,
                )?
            };
            println!("Batch processing completed in {:?}", start.elapsed());

            println!("Exceeds threshold results: {:?}", exceeds_threshold_results);

            // Collect the next heavy hitters based on results
            let mut next_heavy_hitters = Vec::new();
            for (candidate, exceeds_threshold) in candidate_prefix_sets.iter().zip(exceeds_threshold_results.iter()) {
                if *exceeds_threshold {
                    next_heavy_hitters.push(candidate.clone());
                }
            }

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

        // Shutdown dealers using all dealer channels
        for (i, dealer_channel) in dealer_channels.iter().enumerate() {
            let mut locked_dealer_channel = dealer_channel.lock().map_err(|e| format!("Failed to lock dealer channel {}: {}", i, e))?;
            shutdown_dealer(&mut *locked_dealer_channel)?;
        }

        Ok(final_heavy_hitters)
    }

    fn batch_test_prefix_sets_threshold_parallel(
        &self,
        prefix_sets: &[Vec<Vec<bool>>],
        client_shares: &[SharedRange],
        dealer_channels: &[Arc<Mutex<CommTrackingChannel>>],
        other_server_channels: &[Arc<Mutex<CommTrackingChannel>>],
    ) -> Result<Vec<bool>, String> {
        if prefix_sets.is_empty() {
            return Ok(Vec::new());
        }

        let num_threads = other_server_channels.len();
        println!("Parallel processing {} prefix sets across {} threads", prefix_sets.len(), num_threads);

        // Calculate distance threshold once
        let distance_threshold = if self.config.share_config.metric == DistanceMetric::LInfinity {
            get_distance_threshold(self.config.delta, "Linf")
        } else {
            match self.config.share_config.metric {
                DistanceMetric::Lp { p } => get_distance_threshold(self.config.delta, &format!("L{}", p)),
                _ => return Err("Unsupported distance metric for threshold".to_string()),
            }
        };

        // Process prefix sets in parallel chunks
        let chunk_size = (prefix_sets.len() + num_threads - 1) / num_threads;
        let mut server_bits = vec![false; prefix_sets.len()];

        // Use rayon to process chunks in parallel
        server_bits
            .par_chunks_mut(chunk_size)
            .zip(prefix_sets.par_chunks(chunk_size))
            .zip(dealer_channels.par_iter())
            .zip(other_server_channels.par_iter())
            .enumerate()
            .try_for_each(|(chunk_idx, (((result_chunk, prefix_chunk), dealer_channel), other_server_channel))| {
                // Use the corresponding dealer channel for this thread
                let mut dealer_channel_locked = dealer_channel.lock().map_err(|e| format!("Failed to lock dealer channel: {}", e))?;
                let mut other_server_channel_locked = other_server_channel.lock().map_err(|e| format!("Failed to lock other server channel: {}", e))?;
                let mut local_rng = AesRng::new();
                let mut aggregated_counts = Vec::new();

                // Process each prefix set in this chunk using the parallel channels
                for (local_idx, prefix_set) in prefix_chunk.iter().enumerate() {
                    // Handle CheckData - get from dealer if using LpIntervalFSS, otherwise create locally
                    let check_data_list = match self.config.check_config.property {
                        CheckProperty::Equality => {
                            match self.config.check_config.method {
                                CheckMethod::FSS => {
                                    let batch = 
                                        request_dealer_equality(&mut dealer_channel_locked, 1u128 << self.config.check_config.h3)?;

                                    if batch.keys.len() < client_shares.len() {
                                        return Err(format!("Dealer provided {} keys but {} are needed", 
                                                        batch.keys.len(), client_shares.len()));
                                    }

                                    // Create CheckData for each client share using corresponding FSS key
                                    let mut check_data_vec = Vec::new();
                                    for i in 0..client_shares.len() {
                                        check_data_vec.push(CheckData::LinfDpf {
                                            fss_key: batch.keys[i].clone(),
                                            random_value: batch.random_values[i].clone(),
                                        });
                                    }
                                    check_data_vec
                                }
                                CheckMethod::GC => {
                                    vec![CheckData::LinfGarbledCircuits; client_shares.len()]
                                }
                                _ => {
                                    return Err("Unsupported check method for equality check. Currently only support GC and FSS".to_string());
                                }
                            }
                        }
                        CheckProperty::MuBounded => {
                            match self.config.check_config.method {
                                CheckMethod::FSS => {
                                    // Request check FSS keys from dealer using the parallel dealer channel
                                    let batch = request_dealer_check(&mut dealer_channel_locked, 1u128 << self.config.check_config.h3)?;

                                    if batch.keys.len() < client_shares.len() {
                                        return Err(format!("Dealer provided {} keys but {} are needed", 
                                                         batch.keys.len(), client_shares.len()));
                                    }
                                    
                                    // Create CheckData for each client share using corresponding FSS key
                                    let mut check_data_vec = Vec::new();
                                    for i in 0..client_shares.len() {
                                        check_data_vec.push(CheckData::LpIntervalFSS {
                                            fss_key: batch.keys[i].clone(),
                                            random_value: batch.random_values[i],
                                        });
                                    }
                                    check_data_vec
                                }
                                CheckMethod::GC => {
                                    vec![CheckData::LpGarbledCircuits { mu: distance_threshold }; client_shares.len()]
                                }
                                _ => {
                                    return Err("Unsupported check method for mu-bounded check. Currently only support GC and FSS".to_string());
                                }
                            }
                        }
                        _ => {
                            return Err("Unsupported property for testing. Currently only support Equality and MuBounded.".to_string());
                        }
                    };

                    // Run batched check phase for all client shares with this prefix set
                    // This uses garbled circuits that communicate with the other server via the parallel channel

                    let start = std::time::Instant::now();
                    let match_results = self.check_phase.run_batch_fuzzy_match_check(
                        client_shares,
                        prefix_set,
                        &check_data_list,
                        &mut other_server_channel_locked,
                        &mut local_rng,
                    ).map_err(|e| format!("Batch check phase failed: {:?}", e))?;

                    println!("Batch check phase took: {:?}", start.elapsed());

                    let aggregated_result = self.threshold_phase.aggregate_match_results(&match_results).map_err(|e| format!("Failed to aggregate match results: {:?}", e))?;

                    println!("Aggregated result for chunk took: {:?}", aggregated_result);

                    aggregated_counts.push(aggregated_result);

                }

                let threshold_data_list = match self.config.threshold_config.method {
                    ThresholdMethod::GC => {
                        // Use garbled circuits for threshold comparison
                        vec![ThresholdData::GarbledCircuits { t: self.config.threshold }; aggregated_counts.len()]
                    }
                    ThresholdMethod::FSS => {
                        // Request threshold FSS keys from dealer using the parallel dealer channel
                        let batch = request_dealer_threshold(&mut dealer_channel_locked, 1u128 << self.config.threshold_config.h3)?;
                        if batch.keys.len() < aggregated_counts.len() {
                            return Err(format!("Dealer provided {} keys but {} are needed", 
                                               batch.keys.len(), aggregated_counts.len()));
                        }
                        let mut threshold_data_vec = Vec::new();
                        for i in 0..aggregated_counts.len() {
                            threshold_data_vec.push(ThresholdData::IntervalFSS {
                                fss_key: batch.keys[i].clone(),
                                random_value: batch.random_values[i],
                            });
                        }
                        threshold_data_vec
                    }
                };

                // Run threshold phase to check if results exceed threshold
                // This also uses garbled circuits that communicate with the other server
                let start = std::time::Instant::now();
                let results_bool = self.threshold_phase.compare_with_threshold(
                    &aggregated_counts,
                    &threshold_data_list,
                    &mut other_server_channel_locked,
                    &mut local_rng,
                ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

                result_chunk.copy_from_slice(&results_bool);

                println!("Threshold phase took: {:?}", start.elapsed());

                Ok::<(), String>(())
            })
            .map_err(|e| format!("Parallel processing failed: {}", e))?;


        // After parallel processing, exchange the final results in parallel chunks
        let chunk_size = (server_bits.len() + num_threads - 1) / num_threads;
        let mut final_results = vec![false; server_bits.len()];

        // Exchange bits in parallel chunks using the other server channels
        final_results
            .par_chunks_mut(chunk_size)
            .zip(server_bits.par_chunks(chunk_size))
            .enumerate()
            .try_for_each(|(chunk_idx, (result_chunk, bits_chunk))| {
                // Use the corresponding other server channel for this chunk
                let other_server_channel = if chunk_idx < other_server_channels.len() {
                    other_server_channels[chunk_idx].clone()
                } else {
                    // Fallback to round-robin if more chunks than channels
                    other_server_channels[chunk_idx % other_server_channels.len()].clone()
                };

                let mut locked_other_server_channel = other_server_channel.lock().map_err(|e| format!("Failed to lock other server channel for bit exchange: {}", e))?;
                
                let bits_chunk_vec: Vec<bool> = bits_chunk.to_vec();
                
                if self.is_server1 {
                    // Server 1: receive bits from server 0, then send our bits
                    let server0_bits = receive_bool_vec(&mut *locked_other_server_channel)
                        .map_err(|e| format!("Failed to receive bits from server 0 in chunk {}: {:?}", chunk_idx, e))?;
                    send_bool_vec(&mut *locked_other_server_channel, &bits_chunk_vec)
                        .map_err(|e| format!("Failed to send bits to server 0 in chunk {}: {:?}", chunk_idx, e))?;
                    
                    if server0_bits.len() != bits_chunk_vec.len() {
                        return Err(format!("Mismatch in chunk {} size: expected {}, got {}", chunk_idx, bits_chunk_vec.len(), server0_bits.len()));
                    }
                    
                    for (i, (bit0, bit1)) in server0_bits.iter().zip(bits_chunk_vec.iter()).enumerate() {
                        result_chunk[i] = bit0 ^ bit1;
                    }
                } else {
                    // Server 0: send our bits, then receive from server 1
                    send_bool_vec(&mut *locked_other_server_channel, &bits_chunk_vec)
                        .map_err(|e| format!("Failed to send bits to server 1 in chunk {}: {:?}", chunk_idx, e))?;
                    let server1_bits = receive_bool_vec(&mut *locked_other_server_channel)
                        .map_err(|e| format!("Failed to receive bits from server 1 in chunk {}: {:?}", chunk_idx, e))?;
                    
                    if server1_bits.len() != bits_chunk_vec.len() {
                        return Err(format!("Mismatch in chunk {} size: expected {}, got {}", chunk_idx, bits_chunk_vec.len(), server1_bits.len()));
                    }
                    
                    for (i, (bit0, bit1)) in bits_chunk_vec.iter().zip(server1_bits.iter()).enumerate() {
                        result_chunk[i] = bit0 ^ bit1;
                    }
                }

                Ok::<(), String>(())
            })
            .map_err(|e| format!("Parallel bit exchange failed: {}", e))?;

        println!("Parallel processing completed successfully");
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

/// Request FSS keys from dealer
pub fn request_dealer_check(dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestCheckKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    dealer_channel.read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = FssKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

pub fn request_dealer_equality(dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<DpfKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestEqualityKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    dealer_channel.read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = DpfKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

pub fn request_dealer_threshold(dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestThresholdKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    dealer_channel.read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = FssKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

/// Send shutdown signal to dealer
pub fn shutdown_dealer(dealer_channel: &mut CommTrackingChannel) -> Result<(), String> {
    // Send shutdown signal using custom serialization
    let signal_bytes = DealerSignal::Shutdown.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;
    Ok(())
}