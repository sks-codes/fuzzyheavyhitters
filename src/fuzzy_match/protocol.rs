//! Fuzzy Heavy Hitters Protocol Implementation
//!
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::{
    channel::CommTrackingChannel,
    data_structures::modp::Modp,
    fuzzy_match::{
        dealer::{DealerSignal, DpfKeyBatch, FssKeyBatch},
        share_phase::SharePhase,
        share_phase_types::{DistanceMetric, ShareConfig, ShareMethod},
        shared_range::{ShareData, SharedRange},
        sketch_phase::SketchPhase,
        sketch_phase_types::{SketchConfig, SketchDataOwned, VerifyValues},
        check_phase::CheckPhase,
        check_phase_types::{CheckConfig, CheckData, CheckMethod, CheckProperty},
        threshold_phase::ThresholdPhase,
        threshold_phase_types::{ThresholdConfig, ThresholdData, ThresholdMethod},
    },
    randomness::prg::PRG,
    util::{get_distance_threshold, receive_bool_vec, send_bool_vec, u128_to_bits_msb},
};
use rayon::prelude::*;
use scuttlebutt::{AbstractChannel, AesRng};
use std::convert::TryInto;

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
    /// Whether to enable sketching verification
    pub enable_sketch: bool,
}

/// Main protocol structure that encapsulates the entire fuzzy heavy hitters protocol
#[derive(Clone)]
pub struct MosaicProtocol {
    share_phase: SharePhase,
    sketch_phase: SketchPhase,
    check_phase: CheckPhase,
    threshold_phase: ThresholdPhase,
    role: bool, 
}

impl MosaicProtocol {
    /// Create a new protocol instance
    pub fn new<C: Into<ShareConfig> + Into<SketchConfig> + Into<CheckConfig> + Into<ThresholdConfig> + Clone>(config: C, role: bool) -> Self {
        let share_phase = SharePhase::new(config.clone());
        let check_phase = CheckPhase::new(config.clone(), role);
        let sketch_phase = SketchPhase::new(config.clone());
        let threshold_phase = ThresholdPhase::new(config, role);
        Self {
            share_phase,
            sketch_phase,
            check_phase,
            threshold_phase,
            role,
        }
    }

    fn extract_role(shared_range: &SharedRange) -> bool {
        match shared_range {
            SharedRange::OKVS { role, .. } => *role,
            SharedRange::IntervalFSS { role, .. } => *role,
            SharedRange::DistanceFSS { role, .. } => *role,
        }
    }

    fn extract_sketch_seed(shared_range: &SharedRange) -> [u8; 16] {
        shared_range.sketch_seed()
    }

    fn open_modp_shares(
        &self,
        shares: &[Modp<'_>],
        channel: &mut CommTrackingChannel,
        role: bool,
    ) -> Result<Vec<u128>, String> {
        let modulus = self.config.share_config.sketch_modulus;
        let my_values: Vec<u128> = shares.iter().map(|m| m.value()).collect();
        let len = my_values.len() as u64;

        if role {
            channel
                .write_bytes(&len.to_le_bytes())
                .map_err(|e| format!("Failed to send length: {}", e))?;
            for v in &my_values {
                channel
                    .write_bytes(&v.to_le_bytes())
                    .map_err(|e| format!("Failed to send value: {}", e))?;
            }
            channel.flush().map_err(|e| format!("Flush failed: {}", e))?;
        }

        let mut other_len_bytes = [0u8; 8];
        channel
            .read_bytes(&mut other_len_bytes)
            .map_err(|e| format!("Failed to read length: {}", e))?;
        let other_len = u64::from_le_bytes(other_len_bytes) as usize;
        let mut other_values = vec![0u128; other_len];
        for val in other_values.iter_mut() {
            let mut buf = [0u8; 16];
            channel
                .read_bytes(&mut buf)
                .map_err(|e| format!("Failed to read value: {}", e))?;
            *val = u128::from_le_bytes(buf);
        }

        if !role {
            channel
                .write_bytes(&len.to_le_bytes())
                .map_err(|e| format!("Failed to send length: {}", e))?;
            for v in &my_values {
                channel
                    .write_bytes(&v.to_le_bytes())
                    .map_err(|e| format!("Failed to send value: {}", e))?;
            }
            channel.flush().map_err(|e| format!("Flush failed: {}", e))?;
        }

        if other_len != my_values.len() {
            return Err(format!(
                "Sketch verify length mismatch: local {} remote {}",
                my_values.len(),
                other_len
            ));
        }

        Ok(my_values
            .iter()
            .zip(other_values.iter())
            .map(|(a, b)| (a + b) % modulus)
            .collect())
    }

    fn flatten_verify_values<'a>(
        &self,
        verify_values: &'a [VerifyValues<'a>],
    ) -> Vec<Modp<'a>> {
        let mut out = Vec::new();
        for v in verify_values {
            self.flatten_single_verify_value(v, &mut out);
        }
        out
    }

    fn flatten_single_verify_value<'a>(
        &self,
        verify_value: &'a VerifyValues<'a>,
        output: &mut Vec<Modp<'a>>,
    ) {
        match verify_value {
            VerifyValues::Dcf {
                last_layer,
                consistency,
            } => {
                output.push(*last_layer);
                output.extend(consistency.iter().copied());
            }
            VerifyValues::Linf {
                ldcf,
                rdcf,
                consistency,
            } => {
                self.flatten_single_verify_value(ldcf, output);
                self.flatten_single_verify_value(rdcf, output);
                output.push(*consistency);
            }
            VerifyValues::DcfPayload {
                last_layer_consistency,
                consistency,
                ..
            } => {
                output.extend(last_layer_consistency.iter().copied());
                for level in consistency.iter() {
                    for z in level.iter() {
                        output.push(*z);
                    }
                }
            }
            VerifyValues::Lp {
                ldcf0,
                ldcf1,
                rdcf0,
                rdcf1,
                reference_dpf,
                ..
            } => {
                self.flatten_single_verify_value(ldcf0, output);
                self.flatten_single_verify_value(ldcf1, output);
                self.flatten_single_verify_value(rdcf0, output);
                self.flatten_single_verify_value(rdcf1, output);
                output.push(*reference_dpf);
            }
        }
    }

    fn verify_client_sketches(
        &self,
        client_shares: &[SharedRange],
        client_sketches: Option<&[Vec<SketchDataOwned>]>,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, String> {
        if self.config.share_config.method != ShareMethod::FSS || !self.config.enable_sketch {
            return Ok(vec![false; client_shares.len()]);
        }
        if client_shares.is_empty() {
            return Ok(Vec::new());
        }
        if other_server_channels.is_empty() {
            return Err("No server channels available for sketch verification".to_string());
        }
        let sketches = client_sketches
            .ok_or_else(|| "Sketch data missing from client".to_string())?;
        if sketches.len() != client_shares.len() {
            return Err(format!(
                "Sketch data length mismatch: shares {} sketches {}",
                client_shares.len(),
                sketches.len()
            ));
        }
        let sketch_config =
            Self::sketch_config_from_share_config(&self.config.share_config);

        let num_threads = other_server_channels.len().max(1);
        let chunk_size = (client_shares.len() + num_threads - 1) / num_threads.max(1);
        let mut malicious_flags = vec![false; client_shares.len()];
        malicious_flags
            .par_chunks_mut(chunk_size)
            .zip(client_shares.par_chunks(chunk_size))
            .zip(sketches.par_chunks(chunk_size))
            .zip(other_server_channels.par_iter_mut())
            .try_for_each(|(((flag_chunk, range_chunk), sketch_chunk), channel)| {
                let sketch_phase = SketchPhase::new(sketch_config.clone());
                let sketch_helper = sketch_phase
                    .get_sketch_helper()
                    .map_err(|e| format!("Failed to build sketch helper: {}", e))?;

                for ((flag, shared_range), sketches_one) in
                    flag_chunk.iter_mut().zip(range_chunk.iter()).zip(sketch_chunk.iter())
                {
                    let role = Self::extract_role(shared_range);
                    let expected_len = match shared_range {
                        SharedRange::IntervalFSS { keys, .. } => keys.len(),
                        SharedRange::DistanceFSS { keys, .. } => keys.len(),
                        _ => 0,
                    };
                    if sketches_one.len() != expected_len {
                        return Err(format!(
                            "Sketch data dimension mismatch: expected {} got {}",
                            expected_len,
                            sketches_one.len()
                        ));
                    }

                    let seed = Self::extract_sketch_seed(shared_range);
                    let mut prg_sketch = PRG::new(Some(&seed), 0);
                    let sketch_values = sketch_phase
                        .sketch(shared_range, &sketch_helper, &mut prg_sketch)
                        .map_err(|e| format!("Sketch failed: {}", e))?;

                    let sketch_data_vec: Vec<_> = sketches_one
                        .iter()
                        .map(|owned| sketch_phase.sketch_data_from_owned(owned))
                        .collect();

                    let verify_shares = sketch_phase
                        .batch_verify(&sketch_values, &sketch_data_vec, channel, role)
                        .map_err(|e| format!("Sketch verify failed: {}", e))?;
                    let flattened = self.flatten_verify_values(&verify_shares);
                    let opened =
                        self.open_modp_shares(&flattened, channel, self.role)?;
                    if opened.iter().any(|v| *v != 0) {
                        *flag = true;
                    }
                }
                Ok::<(), String>(())
            })
            .map_err(|e| format!("Parallel sketch verification failed: {}", e))?;

        Ok(malicious_flags)
    }

    pub fn receive_client_shares(
        &self,
        client_channel: &mut CommTrackingChannel,
    ) -> Result<(Vec<SharedRange>, Option<Vec<Vec<SketchDataOwned>>>), String> {
        // Receive shares using custom serialization
        let mut len_bytes = [0u8; 8];
        client_channel
            .read_bytes(&mut len_bytes)
            .map_err(|e| format!("Failed to read length from client: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;
        let mut shares_data = vec![0u8; len];
        client_channel
            .read_bytes(&mut shares_data)
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
        if !self.config.enable_sketch {
            return Ok((shares, None));
        }

        let mut sketch_len_bytes = [0u8; 8];
        client_channel
            .read_bytes(&mut sketch_len_bytes)
            .map_err(|e| format!("Failed to read sketch length from client: {}", e))?;
        let sketch_len = u64::from_le_bytes(sketch_len_bytes) as usize;
        if sketch_len == 0 {
            return Ok((shares, None));
        }
        let mut sketch_buf = vec![0u8; sketch_len];
        client_channel
            .read_bytes(&mut sketch_buf)
            .map_err(|e| format!("Failed to read sketches from client: {}", e))?;
        let sketches: Vec<Vec<SketchDataOwned>> = bincode::deserialize(&sketch_buf)
            .map_err(|e| format!("Failed to deserialize sketches: {}", e))?;

        Ok((shares, Some(sketches)))
    }

    /// Parallel version of run_server_known_dictionary using multiple channels for both dealer and other server
    pub fn run_server_known_dictionary_parallel(
        &self,
        client_shares: &[SharedRange],
        client_sketches: Option<&[Vec<SketchDataOwned>]>,
        query_points: &[Vec<u128>],
        signal_dealer_channels: &mut [CommTrackingChannel],
        check_dealer_channels: &mut [CommTrackingChannel],
        threshold_dealer_channels: &mut [CommTrackingChannel],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, String> {
        let sketch_flags =
            self.verify_client_sketches(client_shares, client_sketches, other_server_channels)?;
        let malicious_indices: Vec<usize> = sketch_flags
            .iter()
            .enumerate()
            .filter_map(|(idx, bad)| if *bad { Some(idx) } else { None })
            .collect();
        if !malicious_indices.is_empty() {
            println!(
                "Detected potentially malicious shared ranges (sketch verification failed): {:?}",
                malicious_indices
            );
        }
        let client_shares: Vec<SharedRange> = client_shares
            .iter()
            .zip(sketch_flags.iter())
            .filter_map(|(share, ok)| if !*ok { Some(share.clone()) } else { None })
            .collect();

        // Convert query points from u128 to Vec<Vec<bool>>
        let query_point_sets: Vec<Vec<Vec<bool>>> = query_points
            .iter()
            .map(|query_point| {
                query_point
                    .iter()
                    .map(|&point| u128_to_bits_msb(point, self.config.share_config.h1))
                    .collect()
            })
            .collect();

        let evals = query_point_sets
            .iter()
            .map(|query_point| {
                client_shares
                    .iter()
                    .map(|range| {
                        (0..self.config.share_config.d)
                            .map(|i| {
                                self.share_phase
                                    .evaluate_at_single_dimension(range, &query_point[i], i)
                                    .unwrap()
                            })
                            .collect::<Vec<u128>>()
                    })
                    .collect::<Vec<Vec<u128>>>()
            })
            .collect::<Vec<Vec<Vec<u128>>>>();

        // Use parallel batch processing with both dealer and server channels
        println!(
            "Using full parallel processing with {} check dealer channels and {} server channels",
            check_dealer_channels.len(),
            other_server_channels.len()
        );
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
        client_sketches: Option<&[Vec<SketchDataOwned>]>,
        signal_dealer_channels: &mut [CommTrackingChannel],
        check_dealer_channels: &mut [CommTrackingChannel],
        threshold_dealer_channels: &mut [CommTrackingChannel],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<Vec<u128>>, String> {
        let sketch_flags =
            self.verify_client_sketches(client_shares_list, client_sketches, other_server_channels)?;
        let malicious_indices: Vec<usize> = sketch_flags
            .iter()
            .enumerate()
            .filter_map(|(idx, bad)| if *bad { Some(idx) } else { None })
            .collect();
        if !malicious_indices.is_empty() {
            println!(
                "Detected potentially malicious shared ranges (sketch verification failed): {:?}",
                malicious_indices
            );
        }
        let client_shares_list: Vec<SharedRange> = client_shares_list
            .iter()
            .zip(sketch_flags.iter())
            .filter_map(|(share, ok)| if !*ok { Some(share.clone()) } else { None })
            .collect();

        let max_bit_length = self.config.share_config.h1;
        let dimension = self.config.share_config.d;
        let eval_len = self.config.share_config.h2;
        let eval_modulus = 1u128 << eval_len;

        // Evaluate the empty prefix for each dimension
        let empty_string_data = client_shares_list
            .iter()
            .map(|shared_range| {
                let data = self.share_phase.share_data_init(shared_range).unwrap();
                data.to_bytes(eval_len)
            })
            .collect::<Result<Vec<Vec<u8>>, _>>()
            .map_err(|e| e.to_string())?;
        let mut current_data = vec![empty_string_data];
        let mut current_prefixes: Vec<Vec<Vec<bool>>> = vec![vec![vec![]; dimension]];

        let num_threads = other_server_channels.len();
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();

        for prefix_length in 1..=max_bit_length {
            for dim in 0..dimension {
                let start = std::time::Instant::now();
                let mut new_data: Vec<Vec<Vec<u8>>> = Vec::new();
                for (data, prefix) in current_data.iter().zip(current_prefixes.iter()) {
                    let mut data0 = Vec::with_capacity(client_shares_list.len());
                    let mut data1 = Vec::with_capacity(client_shares_list.len());
                    for (idx, shared_range) in client_shares_list.iter().enumerate() {
                        let (data_dim, _) =
                            ShareData::from_bytes(&data[idx], eval_len, eval_modulus)
                                .map_err(|e| e.to_string())?;
                        let (eval0, eval1) = self
                            .share_phase
                            .expand_prefix(shared_range, &data_dim, &prefix[dim], dim)
                            .map_err(|e| e.to_string())?;

                        data0.push(eval0.to_bytes(eval_len).map_err(|e| e.to_string())?);
                        data1.push(eval1.to_bytes(eval_len).map_err(|e| e.to_string())?);
                    }
                    new_data.push(data0);
                    new_data.push(data1);
                }
                println!("Time to collect new data: {:?}", start.elapsed());
                println!("Number of new data stored: {}", new_data.len());
                println!(
                    "Memory consumption of new_data: {:?} bytes",
                    new_data
                        .iter()
                        .map(|data_bytes| data_bytes.iter().map(|b| b.len()).sum::<usize>())
                        .sum::<usize>()
                );

                crate::util::print_memory_usage();

                let new_eval = thread_pool.install(|| {
                    new_data
                        .par_iter()
                        .map(|data_bytes| {
                            data_bytes
                                .iter()
                                .map(|bytes| {
                                    let (share_data, _) =
                                        ShareData::from_bytes(bytes, eval_len, eval_modulus)
                                            .map_err(|e| e.to_string())?;
                                    let evals = match share_data {
                                        ShareData::OKVS { eval } => eval,
                                        ShareData::IntervalFSS { data } => data
                                            .iter()
                                            .map(|eval| eval.result()[0])
                                            .collect::<Vec<u128>>(),
                                        ShareData::DistanceFSS { eval, .. } => eval,
                                    };
                                    Ok(evals)
                                })
                                .collect::<Result<Vec<Vec<u128>>, String>>()
                        })
                        .collect::<Result<Vec<Vec<Vec<u128>>>, String>>()
                });
                let new_eval = new_eval?;

                println!(
                    "Time to expand prefixes for all clients: {:?}",
                    start.elapsed()
                );

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

                crate::util::print_memory_usage();

                // Use std::mem::take to move qualifying entries out without cloning.
                current_data.clear();
                current_prefixes.clear();
                for ((data, prefix), &exceed) in new_data
                    .iter_mut()
                    .zip(new_prefixes.iter_mut())
                    .zip(exceeds_threshold_results.iter())
                {
                    if exceed {
                        current_data.push(std::mem::take(data));
                        current_prefixes.push(std::mem::take(prefix));
                    }
                }
                // new_eval.clear();
                // new_data.clear();
                // new_prefixes.clear();
                drop(new_eval);
                drop(new_data);
                drop(new_prefixes);

                crate::util::print_memory_usage();

                // new_data/new_prefixes now contain empty Vecs for moved entries; they'll be dropped.
                println!(
                    "Time to filter data based on threshold (move-based): {:?}",
                    start_copy.elapsed()
                );

                println!(
                    "Processed dimension {} with prefix length {} in {:?}",
                    dim,
                    prefix_length,
                    start.elapsed()
                );
                println!("Exceed threshold results: {:?}", exceeds_threshold_results);
            }
        }
        let final_heavy_hitters = current_prefixes
            .into_iter()
            .map(|prefix| {
                // Convert each prefix vector (Vec<bool>) back to a byte vector (Vec<u8>)
                prefix
                    .iter()
                    .map(|bits| {
                        bits.iter()
                            .fold(0u128, |acc, &bit| (acc << 1) | if bit { 1 } else { 0 })
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
        println!(
            "Using {} threads for parallel processing {} data chunks",
            num_threads,
            current_eval.len()
        );

        // Calculate distance threshold once
        let distance_threshold = if self.config.share_config.metric == DistanceMetric::LInfinity {
            get_distance_threshold(self.config.delta, "Linf")
        } else {
            match self.config.share_config.metric {
                DistanceMetric::Lp { p } => {
                    get_distance_threshold(self.config.delta, &format!("L{}", p))
                }
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
            .try_for_each(
                |(chunk_idx, ((result_chunk, bits_chunk), other_server_channel))| {
                    // Use the corresponding other server channel for this chunk
                    let bits_chunk_vec: Vec<bool> = bits_chunk.to_vec();

                    if self.role {
                        // Server 1: receive bits from server 0, then send our bits
                        let server0_bits = receive_bool_vec(other_server_channel).map_err(|e| {
                            format!(
                                "Failed to receive bits from server 0 in chunk {}: {:?}",
                                chunk_idx, e
                            )
                        })?;
                        send_bool_vec(other_server_channel, &bits_chunk_vec).map_err(|e| {
                            format!(
                                "Failed to send bits to server 0 in chunk {}: {:?}",
                                chunk_idx, e
                            )
                        })?;

                        if server0_bits.len() != bits_chunk_vec.len() {
                            return Err(format!(
                                "Mismatch in chunk {} size: expected {}, got {}",
                                chunk_idx,
                                bits_chunk_vec.len(),
                                server0_bits.len()
                            ));
                        }

                        for (i, (bit0, bit1)) in
                            server0_bits.iter().zip(bits_chunk_vec.iter()).enumerate()
                        {
                            result_chunk[i] = bit0 ^ bit1;
                        }
                    } else {
                        // Server 0: send our bits, then receive from server 1
                        send_bool_vec(other_server_channel, &bits_chunk_vec).map_err(|e| {
                            format!(
                                "Failed to send bits to server 1 in chunk {}: {:?}",
                                chunk_idx, e
                            )
                        })?;
                        let server1_bits = receive_bool_vec(other_server_channel).map_err(|e| {
                            format!(
                                "Failed to receive bits from server 1 in chunk {}: {:?}",
                                chunk_idx, e
                            )
                        })?;

                        if server1_bits.len() != bits_chunk_vec.len() {
                            return Err(format!(
                                "Mismatch in chunk {} size: expected {}, got {}",
                                chunk_idx,
                                bits_chunk_vec.len(),
                                server1_bits.len()
                            ));
                        }

                        for (i, (bit0, bit1)) in
                            bits_chunk_vec.iter().zip(server1_bits.iter()).enumerate()
                        {
                            result_chunk[i] = bit0 ^ bit1;
                        }
                    }

                    Ok::<(), String>(())
                },
            )
            .map_err(|e| format!("Parallel bit exchange failed: {}", e))?;
        println!("Time for parallel bit exchange: {:?}", start.elapsed());
        println!("Parallel processing completed successfully");
        Ok(final_results)
    }
}

/// Request FSS keys from dealer
pub fn request_dealer_check(
    signal_dealer_channel: &mut CommTrackingChannel,
    check_dealer_channel: &mut CommTrackingChannel,
    modulus: u128,
) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestCheckKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel
        .write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel
        .write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel
        .flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    check_dealer_channel
        .read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    check_dealer_channel
        .read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) =
        FssKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

pub fn request_dealer_equality(
    signal_dealer_channel: &mut CommTrackingChannel,
    check_dealer_channel: &mut CommTrackingChannel,
    modulus: u128,
) -> Result<DpfKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestEqualityKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel
        .write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel
        .write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel
        .flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    check_dealer_channel
        .read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    check_dealer_channel
        .read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) = DpfKeyBatch::from_bytes(&batch_data, modulus)?;
    Ok(fss_key_batch)
}

pub fn request_dealer_threshold(
    signal_dealer_channel: &mut CommTrackingChannel,
    threshold_dealer_channel: &mut CommTrackingChannel,
    modulus: u128,
) -> Result<FssKeyBatch, String> {
    // Send DealerSignal using custom serialization
    let signal = DealerSignal::RequestThresholdKeys;
    let signal_bytes = signal.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    signal_dealer_channel
        .write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    signal_dealer_channel
        .write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    signal_dealer_channel
        .flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;

    // Receive FSS key batch from dealer
    let mut len_bytes = [0u8; 8];
    threshold_dealer_channel
        .read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read key batch length: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;

    let mut batch_data = vec![0u8; len];
    threshold_dealer_channel
        .read_bytes(&mut batch_data)
        .map_err(|e| format!("Failed to read key batch data: {}", e))?;

    // Use output modulus from threshold config for deserialization
    let (fss_key_batch, _) =
        FssKeyBatch::from_bytes(&batch_data, modulus).expect("Failed to deserialize FssKeyBatch");
    Ok(fss_key_batch)
}

/// Send shutdown signal to dealer
pub fn shutdown_dealer(dealer_channel: &mut CommTrackingChannel) -> Result<(), String> {
    // Send shutdown signal using custom serialization
    let signal_bytes = DealerSignal::Shutdown.to_bytes();
    let len_bytes = (signal_bytes.len() as u64).to_le_bytes();
    dealer_channel
        .write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to write DealerSignal length: {}", e))?;
    dealer_channel
        .write_bytes(&signal_bytes)
        .map_err(|e| format!("Failed to write DealerSignal: {}", e))?;
    dealer_channel
        .flush()
        .map_err(|e| format!("Failed to flush DealerSignal: {}", e))?;
    Ok(())
}
