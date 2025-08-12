use std::convert::{TryFrom, TryInto};
use scuttlebutt::{AesRng, Block, AbstractChannel};
use crate::channel::CommTrackingChannel;
use crate::fuzzy_match::share_phase::{SharePhase, SharePhaseError, SharedRange};
use crate::garbled_circuits::{
    equality_full::{multiple_gb_equality_test, multiple_ev_equality_test},
    batch_equality_full::{batch_gb_equality_test, batch_ev_equality_test},
    less_than_or_equal_threshold::{multiple_gb_less_than_ss, multiple_ev_less_than_ss},
};
use crate::data_structures::modint::ModInt;
use crate::fss::{
    ldcf::LdcfKey,
    rdcf::RdcfKey,
    dpf::DpfKey,
};
use crate::util::{query_point_to_u128s, u128_to_bits, u128_to_bits_msb};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use ocelot::ot::{Receiver, Sender};

/// Method for check phase comparison
#[derive(Debug, Clone)]
pub enum CheckMethod {
    /// Use L-infinity distance comparison with garbled circuits
    LinfGarbledCircuits,
    /// Use L-infinity distance comparison with DPF
    LinfDpf,
    /// Use Lp distance comparison with garbled circuits
    LpGarbledCircuits,
    /// Use Lp distance comparison with IntervalFSS
    LpIntervalFSS,
}

/// Data for check phase configuration
#[derive(Debug, Clone)]
pub enum CheckData {
    LinfGarbledCircuits,
    LinfDpf {
        fss_key: DpfKey<1>,
        random_value: u128,
    },
    /// Threshold value for Lp distance comparison with garbled circuits
    LpGarbledCircuits {
        /// Threshold value for comparison
        threshold: u128,
    },
    /// FSS key, random value, and threshold for Lp distance comparison with IntervalFSS
    LpIntervalFSS {
        fss_key: (LdcfKey<1>, RdcfKey<1>),
        random_value: u128,
    },
}

/// Configuration for the check phase
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub input_bit_length: usize,
    pub output_bit_length: usize,
    /// Number of dimensions for evaluation
    pub num_dimensions: usize,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
    /// Method to use for check phase
    pub method: CheckMethod,
}

/// Error types for check phase operations
#[derive(Debug, Clone)]
pub enum CheckPhaseError {
    /// Error during share phase evaluation
    SharePhaseError(SharePhaseError),
    /// Mismatched input lengths
    InputLengthMismatch(String),
    /// Channel communication error
    ChannelError(String),
    /// Invalid configuration
    InvalidConfig(String),
}

impl From<SharePhaseError> for CheckPhaseError {
    fn from(error: SharePhaseError) -> Self {
        CheckPhaseError::SharePhaseError(error)
    }
}

/// Check phase handler
#[derive(Clone)]
pub struct CheckPhase {
    config: CheckConfig,
    share_phase: SharePhase,
}

impl CheckPhase {
    /// Create a new check phase with the given configuration
    pub fn new(config: CheckConfig, share_phase: SharePhase) -> Self {
        Self { config, share_phase }
    }
    /// Run fuzzy match check for a d-dimensional query point against multiple shared ranges
    /// This reduces communication rounds by batching multiple range checks
    /// Input: query_point is a slice of Vec<bool> where each Vec<bool> represents the bits for one dimension
    /// Input: shared_ranges is a slice of SharedRange objects to check against
    /// Input: check_data_list is a slice of CheckData objects, one for each shared range
    /// Returns: Vector of ModInt results, one for each shared range
    pub fn run_batch_fuzzy_match_check(
        &self,
        shared_ranges: &[SharedRange],
        query_point: &[Vec<bool>],
        check_data_list: &[CheckData],
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        if query_point.len() != self.config.num_dimensions {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Query point has {} dimensions but config expects {}", query_point.len(), self.config.num_dimensions)
            ));
        }

        if shared_ranges.len() != check_data_list.len() {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Number of shared ranges ({}) must match number of check data items ({})", 
                        shared_ranges.len(), check_data_list.len())
            ));
        }

        if shared_ranges.is_empty() {
            return Ok(Vec::new());
        }

        // Ensure all CheckData items are the same type for efficient batching
        let first_check_data = &check_data_list[0];
        let all_same_type = check_data_list.iter().all(|cd| std::mem::discriminant(cd) == std::mem::discriminant(first_check_data));
        
        if !all_same_type {
            return Err(CheckPhaseError::InvalidConfig(
                "All CheckData items must be the same type for batch processing".to_string()
            ));
        }

        // All CheckData items are the same type, we can batch efficiently
        match (&self.config.method, first_check_data) {
            (CheckMethod::LinfGarbledCircuits, CheckData::LinfGarbledCircuits) => {
                self.run_batch_linf_check_gc(shared_ranges, query_point, channel, rng)
            }
            (CheckMethod::LinfDpf, CheckData::LinfDpf { .. }) => {
                let mut fss_keys = Vec::new();
                let mut random_values = Vec::new();

                for check_data in check_data_list {
                    if let CheckData::LinfDpf { fss_key, random_value } = check_data {
                        fss_keys.push(fss_key.clone());
                        random_values.push(*random_value);
                    }
                }

                self.run_batch_linf_check_dpf(shared_ranges, query_point, &random_values, &fss_keys, channel)
            }
            (CheckMethod::LpGarbledCircuits, CheckData::LpGarbledCircuits { threshold }) => {
                let threshold_modint = ModInt::new(*threshold, 1 << self.config.input_bit_length);
                self.run_batch_lp_distance_check_gc(shared_ranges, query_point, threshold_modint, channel, rng)
            }
            (CheckMethod::LpIntervalFSS, CheckData::LpIntervalFSS { .. }) => {
                // For FSS, extract all the keys and random values for batch processing
                let mut fss_keys = Vec::new();
                let mut random_values = Vec::new();
                
                for check_data in check_data_list {
                    if let CheckData::LpIntervalFSS { fss_key, random_value } = check_data {
                        fss_keys.push(fss_key.clone());
                        random_values.push(*random_value);
                    }
                }
                
                self.run_batch_lp_distance_check_intervalfss(
                    shared_ranges, 
                    query_point, 
                    &random_values, 
                    &fss_keys, 
                    channel
                )
            }
            _ => {
                Err(CheckPhaseError::InvalidConfig(
                    format!("Mismatch between check method {:?} and check data type", self.config.method)
                ))
            }
        }
    }

    /// Batch L-infinity distance equality test method
    fn run_batch_linf_check_gc(
        &self,
        shared_ranges: &[SharedRange],
        query_point: &[Vec<bool>],
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        // Step 1: For each shared range and each dimension, evaluate query_point with OKVS
        let mut all_dimension_evals = Vec::new();
        
        for shared_range in shared_ranges {
            let mut dimension_eval_for_range = Vec::<ModInt>::new();
            
            for dim in 0..self.config.num_dimensions {
                // Use the bits directly from query_point
                let point_bits = &query_point[dim];
                
                // Evaluate at the specific dimension for this shared range
                let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
                dimension_eval_for_range.push(ModInt::new(res, 1 << self.config.input_bit_length));
            }
            
            all_dimension_evals.push(dimension_eval_for_range);
        }

        // Step 2: Run batch equality test for all ranges at once
        let all_equality_results = if self.config.is_garbler_side {
            batch_gb_equality_test(rng, channel, &all_dimension_evals)
        } else {
            batch_ev_equality_test(rng, channel, &all_dimension_evals)
        };

        // Step 3: Convert boolean results to ring shares using batched OT
        let ring_shares = self.batch_boolean_to_ring_share_modint(
            &all_equality_results,
            1 << self.config.output_bit_length,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;
        
        Ok(ring_shares)
    }

    fn run_batch_linf_check_dpf(
        &self,
        shared_ranges: &[SharedRange],
        query_point: &[Vec<bool>],
        random_values: &[u128],
        fss_keys: &[LdcfKey<1>],
        channel: &mut CommTrackingChannel,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        unimplemented!()
    }

    /// Batch Lp distance check using garbled circuits
    fn run_batch_lp_distance_check_gc(
        &self,
        shared_ranges: &[SharedRange],
        query_point: &[Vec<bool>],
        threshold: ModInt,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        // Step 1: For each shared range, evaluate all dimensions and aggregate
        let mut aggregated_shares = Vec::new();
        let modulus = 1u128 << self.config.input_bit_length;
        
        for shared_range in shared_ranges {
            let mut all_dimension_eval = Vec::<ModInt>::new();
            
            for dim in 0..self.config.num_dimensions {
                let point_bits = &query_point[dim];
                let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
                all_dimension_eval.push(ModInt::new(res, modulus));
            }

            // Aggregate dimensions for this range
            let mut aggregated_share = ModInt::new(0, modulus);
            for dimension_result in &all_dimension_eval {
                aggregated_share = aggregated_share + *dimension_result;
            }
            aggregated_shares.push(aggregated_share);
        }

        // Step 2: Create threshold vector for batch comparison
        let thresholds = vec![threshold; shared_ranges.len()];

        // Step 3: Run batch comparison using garbled circuits
        let comparison_results = if self.config.is_garbler_side {
            multiple_gb_less_than_ss(rng, channel, &aggregated_shares, &thresholds)
        } else {
            multiple_ev_less_than_ss(rng, channel, &aggregated_shares)
        };

        // Step 4: Convert boolean results to ring shares using batched OT
        let ring_shares = self.batch_boolean_to_ring_share_modint(
            &comparison_results,
            1 << self.config.output_bit_length,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;
        
        Ok(ring_shares)
    }

    /// Batch Lp distance check using IntervalFSS
    fn run_batch_lp_distance_check_intervalfss(
        &self,
        shared_ranges: &[SharedRange],
        query_point: &[Vec<bool>],
        random_values: &[u128],
        fss_keys: &[(LdcfKey<1>, RdcfKey<1>)],
        channel: &mut CommTrackingChannel,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        let in_modulus = 1u128 << self.config.input_bit_length;
        let out_modulus = 1u128 << self.config.output_bit_length;

        // Step 1: For each shared range, evaluate all dimensions and aggregate
        let mut aggregated_shares = Vec::new();
        
        for shared_range in shared_ranges {
            let mut all_dimension_eval = Vec::<ModInt>::new();
            
            for dim in 0..self.config.num_dimensions {
                let point_bits = &query_point[dim];
                let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
                all_dimension_eval.push(ModInt::new(res, in_modulus));
            }

            // Aggregate dimensions for this range
            let mut aggregated_share = ModInt::new(0, in_modulus);
            for dimension_result in &all_dimension_eval {
                aggregated_share = aggregated_share + *dimension_result;
            }
            aggregated_shares.push(aggregated_share);
        }

        // Step 2: Add random values and exchange with other server (batched)
        let masked_shares: Vec<ModInt> = aggregated_shares.iter().zip(random_values.iter())
            .map(|(&share, &random_value)| share + ModInt::new(random_value, in_modulus))
            .collect();

        // Step 3: Exchange all masked shares in one communication round
        let reconstructed_masked_distances = if self.config.is_garbler_side {
            // Server 1 (garbler) sends all shares first, then receives all
            for masked_share in &masked_shares {
                let share_bytes = masked_share.val().to_le_bytes();
                channel.write_bytes(&share_bytes)
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            }
            channel.flush()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;

            let mut reconstructed = Vec::new();
            for masked_share in &masked_shares {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes)
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
                let other_masked_share = u128::from_le_bytes(received_bytes);
                let other_masked_share_modint = ModInt::new(other_masked_share, in_modulus);
                reconstructed.push(*masked_share + other_masked_share_modint);
            }
            reconstructed
        } else {
            // Server 0 (evaluator) receives all shares first, then sends all
            let mut other_masked_shares = Vec::new();
            for _ in &masked_shares {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes)
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
                let other_masked_share = u128::from_le_bytes(received_bytes);
                other_masked_shares.push(ModInt::new(other_masked_share, in_modulus));
            }
            
            for masked_share in &masked_shares {
                let share_bytes = masked_share.val().to_le_bytes();
                channel.write_bytes(&share_bytes)
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            }
            channel.flush()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;
            
            masked_shares.iter().zip(other_masked_shares.iter())
                .map(|(&m1, &m2)| m1 + m2)
                .collect()
        };

        // Step 4: Evaluate each reconstructed distance using FSS
        let mut results = Vec::new();
        for (reconstructed_distance, (fss_key0, fss_key1)) in reconstructed_masked_distances.iter().zip(fss_keys.iter()) {
            let mut distance_bits = u128_to_bits_msb(reconstructed_distance.val(), self.config.input_bit_length);
            let fss_result = fss_key0.eval_ldcf(&distance_bits, out_modulus) + fss_key1.eval_rdcf(&distance_bits, out_modulus);

            let result = if self.config.is_garbler_side {
                ModInt::new(out_modulus - fss_result[0], out_modulus)
            } else {
                ModInt::new(fss_result[0], out_modulus)
            };
            results.push(result);
        }
        
        Ok(results)
    }

    /// Convert boolean share to ModInt ring share using OT
    pub fn boolean_to_ring_share_modint(
        &self,
        boolean_share: bool,
        modulus: u128,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
        is_garbler_side: bool,
    ) -> Result<ModInt, CheckPhaseError> {
        if is_garbler_side {
            // Garbler side: generate random shares and send via OT
            let ring_share = ModInt::random(modulus);
            let r0 = ModInt::zero(modulus) - ring_share;
            let r1 = ModInt::one(modulus) - ring_share; 
            
            let r0_block: Block = r0.clone().try_into()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r0 to Block: {:?}", e)))?;
            let r1_block: Block = r1.clone().try_into()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r1 to Block: {:?}", e)))?;
            
            let shares = if !boolean_share {
                (r0_block, r1_block)
            } else {
                (r1_block, r0_block)
            };
            
            let mut ot = OtSender::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT sender init failed: {:?}", e)))?;
            
            ot.send(channel, &[shares], rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT send failed: {:?}", e)))?;
            
            // Return r1 as the garbler's share
            Ok(ring_share)
        } else {
            // Receiver side: receive share via OT
            let mut ot = OtReceiver::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receiver init failed: {:?}", e)))?;
            
            let out_blocks = ot.receive(channel, &[boolean_share], rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receive failed: {:?}", e)))?;
            
            // Convert block to u128 and create ModInt with correct modulus
            let raw_value: u128 = unsafe { std::mem::transmute(out_blocks[0]) };
            let ring_share = ModInt::new(raw_value, modulus);
            
            Ok(ring_share)
        }
    }

    /// Convert multiple boolean shares to ModInt ring shares using batched OT
    /// This is more efficient than calling boolean_to_ring_share_modint multiple times
    pub fn batch_boolean_to_ring_share_modint(
        &self,
        boolean_shares: &[bool],
        modulus: u128,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
        is_garbler_side: bool,
    ) -> Result<Vec<ModInt>, CheckPhaseError> {
        if boolean_shares.is_empty() {
            return Ok(Vec::new());
        }

        if is_garbler_side {
            // Garbler side: generate random shares and send via batched OT
            let mut ring_shares = Vec::new();
            let mut ot_pairs = Vec::new();
            
            for &boolean_share in boolean_shares {
                let ring_share = ModInt::random(modulus);
                let r0 = ModInt::zero(modulus) - ring_share;
                let r1 = ModInt::one(modulus) - ring_share;
                
                let r0_block: Block = r0.clone().try_into()
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r0 to Block: {:?}", e)))?;
                let r1_block: Block = r1.clone().try_into()
                    .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r1 to Block: {:?}", e)))?;
                
                let shares = if !boolean_share {
                    (r0_block, r1_block)
                } else {
                    (r1_block, r0_block)
                };
                
                ot_pairs.push(shares);
                ring_shares.push(ring_share);
            }
            
            let mut ot = OtSender::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT sender init failed: {:?}", e)))?;
            
            ot.send(channel, &ot_pairs, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT send failed: {:?}", e)))?;
            
            Ok(ring_shares)
        } else {
            // Receiver side: receive shares via batched OT
            let mut ot = OtReceiver::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receiver init failed: {:?}", e)))?;
            
            let out_blocks = ot.receive(channel, boolean_shares, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receive failed: {:?}", e)))?;
            
            // Convert blocks to ModInts
            let mut ring_shares = Vec::new();
            for block in out_blocks {
                let raw_value: u128 = unsafe { std::mem::transmute(block) };
                let ring_share = ModInt::new(raw_value, modulus);
                ring_shares.push(ring_share);
            }
            
            Ok(ring_shares)
        }
    }
}
