//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, SharedRange, ShareData, DistanceMetric};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckData, CheckMethod, CheckProperty};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::fuzzy_match::dealer::{FssKeyBatch, DpfKeyBatch, DealerSignal};
use crate::util::{send_bool_vec, receive_bool_vec, u128_to_bits_msb, get_distance_threshold};
use crate::channel::CommTrackingChannel;
use scuttlebutt::{AbstractChannel, AesRng};
use std::convert::TryInto;
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
pub struct MosaicProtocol {
    config: ProtocolConfig,
    share_phase: SharePhase,
    check_phase: CheckPhase,
    threshold_phase: ThresholdPhase,
    is_server1: bool,
}

impl MosaicProtocol {
    /// Create a new protocol instance
    pub fn new(config: ProtocolConfig, is_server1: bool) -> Self {
        let share_phase = SharePhase::new(config.share_config.clone());
        let check_phase = CheckPhase::new(config.check_config.clone());
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
        let bytes = &shares_data[..];
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
        signal_dealer_channels: &mut [CommTrackingChannel],
        check_dealer_channels: &mut [CommTrackingChannel],
        threshold_dealer_channels: &mut [CommTrackingChannel],
        other_server_channels: &mut [CommTrackingChannel],
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

        let evals = query_point_sets.iter()
            .map(|query_point| {
                client_shares.iter().map(|range| {
                    (0..self.config.share_config.d).map(|i| {
                        self.share_phase.evaluate_at_single_dimension(range, &query_point[i], i).unwrap()
                    }).collect::<Vec<u128>>()
                }).collect::<Vec<Vec<u128>>>()
            }).collect::<Vec<Vec<Vec<u128>>>>();

        // Use parallel batch processing with both dealer and server channels
        println!("Using full parallel processing with {} check dealer channels and {} server channels",
                 check_dealer_channels.len(), other_server_channels.len());
        let final_results = self.batch_check(
            &evals,
            signal_dealer_channels,
            check_dealer_channels,
            threshold_dealer_channels,
            other_server_channels,
        )?;

        // Shutdown dealers using all signal dealer channels
        for dealer_channel in signal_dealer_channels.iter_mut() {
            shutdown_dealer(dealer_channel)?;
        }

        Ok(final_results)
    }

    /// Parallel version of run_server_unknown_dictionary using multiple channels
    pub fn run_server_unknown_dictionary_parallel(
        &self,
        client_shares_list: &[SharedRange],
        signal_dealer_channels: &mut [CommTrackingChannel],
        check_dealer_channels: &mut [CommTrackingChannel],
        threshold_dealer_channels: &mut [CommTrackingChannel],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<Vec<u128>>, String> {
        let max_bit_length = self.config.share_config.h1;
        let dimension = self.config.share_config.d;
        let eval_len = self.config.share_config.h2;
        let eval_modulus = 1u128 << eval_len;

        // Evaluate the empty prefix for each dimension
        let empty_string_data = client_shares_list.iter()
            .map(|shared_range| {
                let data = self.share_phase.share_data_init(shared_range).unwrap();
                data.to_bytes(eval_len)
            })
            .collect::<Vec<Vec<u8>>>();
        let mut current_data = vec![empty_string_data];
        let mut current_prefixes: Vec<Vec<Vec<bool>>> = vec![vec![vec![]; dimension]];

        let num_threads = other_server_channels.len();
        let thread_pool = rayon::ThreadPoolBuilder::new().num_threads(num_threads).build().unwrap();

        for prefix_length in 1..=max_bit_length {
            for dim in 0..dimension {
                let start = std::time::Instant::now();
                let mut new_data= thread_pool.install(|| {
                    current_data.par_iter()
                        .zip(current_prefixes.par_iter())
                        .flat_map(|(data, prefix)| {
                            let mut data0 = Vec::with_capacity(client_shares_list.len());
                            let mut data1 = Vec::with_capacity(client_shares_list.len());
                            for (idx, shared_range) in client_shares_list.iter().enumerate() {
                                let (data_dim, _) = ShareData::from_bytes(&data[idx], eval_len, eval_modulus);
                                let (eval0, eval1) = self.share_phase.expand_prefix(shared_range, &data_dim, &prefix[dim], dim).unwrap();

                                data0.push(eval0.to_bytes(eval_len));
                                data1.push(eval1.to_bytes(eval_len));
                            }
                            vec![data0, data1]
                        })
                        .collect::<Vec<Vec<Vec<u8>>>>()
                });
                println!("Time to collect new data: {:?}", start.elapsed());
                let mut new_eval = thread_pool.install(|| {
                    new_data.par_iter().map(|data_bytes| {
                        data_bytes.iter()
                        .map(|bytes| {
                            let (share_data, _) = ShareData::from_bytes(bytes, eval_len, eval_modulus);
                            match share_data {
                                ShareData::OKVS { eval } => eval.clone(),
                                ShareData::IntervalFSS { eval, .. } => eval.clone(),
                                ShareData::DistanceFSSL1 { eval, .. } => eval.clone(),
                                ShareData::DistanceFSSL2 { eval, .. } => eval.clone(),
                                ShareData::DistanceFSSL3 { eval, .. } => eval.clone(),
                            }
                        })
                        .collect::<Vec<Vec<u128>>>()
                    }).collect::<Vec<Vec<Vec<u128>>>>()
                });

                println!("Time to expand prefixes for all clients: {:?}", start.elapsed());

                let mut new_prefixes = Vec::new();
                for prefix in current_prefixes.iter() {
                    let mut new_prefix = prefix.clone();
                    new_prefix[dim].push(false);
                    new_prefixes.push(new_prefix.clone());
                    new_prefix[dim].pop();
                    new_prefix[dim].push(true);
                    new_prefixes.push(new_prefix);
                }

                println!("Time to expand prefixes: {:?}", start.elapsed());

                let exceeds_threshold_results = self.batch_check(
                    &new_eval,
                    signal_dealer_channels,
                    check_dealer_channels,
                    threshold_dealer_channels,
                    other_server_channels,
                )?;

                println!("Time for batch check: {:?}", start.elapsed());

                let start_copy = std::time::Instant::now();
                // Use std::mem::take to move qualifying entries out without cloning.
                current_data.clear();
                current_prefixes.clear();
                new_eval.clear();
                for ((data, prefix), &exceed) in new_data.iter_mut().zip(new_prefixes.iter_mut()).zip(exceeds_threshold_results.iter()) {
                    if exceed {
                        current_data.push(std::mem::take(data));
                        current_prefixes.push(std::mem::take(prefix));
                    }
                }
                new_data.clear();
                new_prefixes.clear();
                // new_data/new_prefixes now contain empty Vecs for moved entries; they'll be dropped.
                println!("Time to filter data based on threshold (move-based): {:?}", start_copy.elapsed());

                println!("Processed dimension {} with prefix length {} in {:?}", 
                         dim, prefix_length, start.elapsed());
                println!("Exceed threshold results: {:?}", exceeds_threshold_results);
            }
        }
        let final_heavy_hitters = current_prefixes.into_iter()
            .map(|prefix| {
                // Convert each prefix vector (Vec<bool>) back to a byte vector (Vec<u8>)
                prefix.iter()
                    .map(|bits| {
                        bits.iter().fold(0u128, |acc, &bit| (acc << 1) | if bit { 1 } else { 0 })
                    })
                    .collect::<Vec<u128>>()
            })
            .collect();

        // Shutdown dealers using all signal dealer channels
        for dealer_channel in signal_dealer_channels.iter_mut() {
            shutdown_dealer(dealer_channel)?;
        }

        Ok(final_heavy_hitters)
    }

    fn batch_check(
        &self,
        current_eval: &[Vec<Vec<u128>>],
        signal_dealer_channels: &mut [CommTrackingChannel],
        check_dealer_channels: &mut [CommTrackingChannel],
        threshold_dealer_channels: &mut [CommTrackingChannel],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, String> {
        let num_threads = other_server_channels.len();
        println!("Using {} threads for parallel processing {} data chunks", num_threads, current_eval.len());

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
        let chunk_size = (current_eval.len() + num_threads - 1) / num_threads;
        let mut server_bits = vec![false; current_eval.len()];

        // Use rayon to process chunks in parallel
        server_bits
            .par_chunks_mut(chunk_size)
            .zip(current_eval.par_chunks(chunk_size))
            .zip(signal_dealer_channels.par_iter_mut())
            .zip(check_dealer_channels.par_iter_mut())
            .zip(threshold_dealer_channels.par_iter_mut())
            .zip(other_server_channels.par_iter_mut())
            .try_for_each(|(((((result_chunk, eval_chunk), signal_dealer_channel), check_dealer_channel), threshold_dealer_channel), other_server_channel)| {
                // Use the corresponding dealer channels for this thread
                let mut local_rng = AesRng::new();
                let mut aggregated_counts = Vec::new();

                let start = std::time::Instant::now();
                // Process each evaluation chunk in this chunk using the parallel channels
                for eval in eval_chunk.iter() {
                    // Handle CheckData - get from dealer if using LpIntervalFSS, otherwise create locally
                    let check_data_list = match self.config.check_config.property {
                        CheckProperty::Equality => {
                            match self.config.check_config.method {
                                CheckMethod::FSS => {
                                    let batch = 
                                        request_dealer_equality(signal_dealer_channel, check_dealer_channel, 1u128 << self.config.check_config.h3)?;

                                    if batch.keys.len() < self.config.num_clients {
                                        return Err(format!("Dealer provided {} keys but {} are needed",
                                                        batch.keys.len(), self.config.num_clients));
                                    }

                                    // Create CheckData for each client share using corresponding FSS key
                                    let mut check_data_vec = Vec::new();
                                    for i in 0..self.config.num_clients {
                                        check_data_vec.push(CheckData::LinfDpf {
                                            fss_key: batch.keys[i].clone(),
                                            random_value: batch.random_values[i].clone(),
                                        });
                                    }
                                    check_data_vec
                                }
                                CheckMethod::GC => {
                                    vec![CheckData::LinfGarbledCircuits; self.config.num_clients]
                                }
                            }
                        }
                        CheckProperty::MuBounded => {
                            match self.config.check_config.method {
                                CheckMethod::FSS => {
                                    // Request check FSS keys from dealer using the parallel check dealer channel
                                    let batch = request_dealer_check(signal_dealer_channel, check_dealer_channel, 1u128 << self.config.check_config.h3)?;
                                    println!("Received dealer check keys");

                                    if batch.keys.len() < self.config.num_clients {
                                        return Err(format!("Dealer provided {} keys but {} are needed",
                                                         batch.keys.len(), self.config.num_clients));
                                    }

                                    if batch.random_values.len() < self.config.num_clients {
                                        return Err(format!("Dealer provided {} random values but {} are needed",
                                                         batch.random_values.len(), self.config.num_clients));
                                    }

                                    // Create CheckData for each client share using corresponding FSS key
                                    let mut check_data_vec = Vec::new();
                                    for i in 0..self.config.num_clients {
                                        check_data_vec.push(CheckData::LpIntervalFSS {
                                            fss_key: batch.keys[i].clone(),
                                            random_value: batch.random_values[i],
                                        });
                                    }
                                    check_data_vec
                                }
                                CheckMethod::GC => {
                                    vec![CheckData::LpGarbledCircuits { mu: distance_threshold }; self.config.num_clients]
                                }
                            }
                        }
                    };

                    // Run batched check phase for all client shares with this prefix set
                    // This uses garbled circuits that communicate with the other server via the parallel channel

                    let match_results = self.check_phase.run_batch_fuzzy_match_check(
                        &eval,
                        &check_data_list,
                        other_server_channel,
                        &mut local_rng,
                    ).map_err(|e| format!("Batch check phase failed: {:?}", e))?;

                    let aggregated_result = self.threshold_phase.aggregate_match_results(&match_results).map_err(|e| format!("Failed to aggregate match results: {:?}", e))?;

                    aggregated_counts.push(aggregated_result);

                }

                println!("Time to process all prefixes and aggregate results: {:?}", start.elapsed());

                let threshold_data_list = match self.config.threshold_config.method {
                    ThresholdMethod::GC => {
                        // Use garbled circuits for threshold comparison
                        vec![ThresholdData::GarbledCircuits { t: self.config.threshold }; aggregated_counts.len()]
                    }
                    ThresholdMethod::FSS => {
                        // Request threshold FSS keys from dealer using the parallel threshold dealer channel
                        let threshold_data_vec = (0..aggregated_counts.len()).map(|_| {
                            let batch = request_dealer_threshold(signal_dealer_channel, threshold_dealer_channel, 2).unwrap();
                            if batch.keys.len() < 1 {
                                panic!("Dealer provided {} keys but {} are needed", batch.keys.len(), 1);
                            }

                            ThresholdData::IntervalFSS {
                                fss_key: batch.keys[0].clone(),
                                random_value: batch.random_values[0],
                            }
                        }).collect::<Vec<ThresholdData>>();
                        threshold_data_vec
                    }
                };

                // Run threshold phase to check if results exceed threshold
                // This also uses garbled circuits that communicate with the other server
                let start = std::time::Instant::now();
                let results_bool = self.threshold_phase.compare_with_threshold(
                    &aggregated_counts,
                    &threshold_data_list,
                    other_server_channel,
                    &mut local_rng,
                ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

                result_chunk.copy_from_slice(&results_bool);
                println!("Time for threshold comparison: {:?}", start.elapsed());

                Ok::<(), String>(())
            })
            .map_err(|e| format!("Parallel processing failed: {}", e))?;


        // After parallel processing, exchange the final results in parallel chunks
        let chunk_size = (server_bits.len() + num_threads - 1) / num_threads;
        let mut final_results = vec![false; server_bits.len()];

        // Exchange bits in parallel chunks using the other server channels
        let start = std::time::Instant::now();
        final_results
            .par_chunks_mut(chunk_size)
            .zip(server_bits.par_chunks(chunk_size))
            .zip(other_server_channels.par_iter_mut())
            .enumerate()
            .try_for_each(|(chunk_idx, ((result_chunk, bits_chunk), other_server_channel))| {
                // Use the corresponding other server channel for this chunk
                let bits_chunk_vec: Vec<bool> = bits_chunk.to_vec();
                
                if self.is_server1 {
                    // Server 1: receive bits from server 0, then send our bits
                    let server0_bits = receive_bool_vec(other_server_channel)
                        .map_err(|e| format!("Failed to receive bits from server 0 in chunk {}: {:?}", chunk_idx, e))?;
                    send_bool_vec(other_server_channel, &bits_chunk_vec)
                        .map_err(|e| format!("Failed to send bits to server 0 in chunk {}: {:?}", chunk_idx, e))?;
                    
                    if server0_bits.len() != bits_chunk_vec.len() {
                        return Err(format!("Mismatch in chunk {} size: expected {}, got {}", chunk_idx, bits_chunk_vec.len(), server0_bits.len()));
                    }
                    
                    for (i, (bit0, bit1)) in server0_bits.iter().zip(bits_chunk_vec.iter()).enumerate() {
                        result_chunk[i] = bit0 ^ bit1;
                    }
                } else {
                    // Server 0: send our bits, then receive from server 1
                    send_bool_vec(other_server_channel, &bits_chunk_vec)
                        .map_err(|e| format!("Failed to send bits to server 1 in chunk {}: {:?}", chunk_idx, e))?;
                    let server1_bits = receive_bool_vec(other_server_channel)
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
        println!("Time for parallel bit exchange: {:?}", start.elapsed());
        println!("Parallel processing completed successfully");
        Ok(final_results)
    }
}

/// Request FSS keys from dealer
pub fn request_dealer_check(signal_dealer_channel: &mut CommTrackingChannel, check_dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestCheckKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    check_dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    check_dealer_channel.read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = FssKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

pub fn request_dealer_equality(signal_dealer_channel: &mut CommTrackingChannel, check_dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<DpfKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestEqualityKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    check_dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    check_dealer_channel.read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;
    
    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = DpfKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

pub fn request_dealer_threshold(signal_dealer_channel: &mut CommTrackingChannel, threshold_dealer_channel: &mut CommTrackingChannel, modulus: u128) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestThresholdKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel.write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel.flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    threshold_dealer_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    threshold_dealer_channel.read_bytes(&mut batch_data)
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