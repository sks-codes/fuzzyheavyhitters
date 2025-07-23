//! Check Phase Implementation
//!
//! This module provides functionality for checking the outputs from the share phase
//! using garbled circuit equality tests from equality.rs
//!
//! ## Protocol Flow:
//! 1. Client shares a secret value x using SharePhase
//! 2. Server 1 has evaluation points y1 and evaluates the shared range at these points
//! 3. Server 2 has evaluation points y2 and evaluates the shared range at these points  
//! 4. The two servers use garbled circuits to compare their evaluation results
//!    without revealing the actual values to each other
//! 5. The result indicates whether their evaluations are equal at corresponding positions

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::convert::{TryFrom, TryInto};
use scuttlebutt::{AesRng, Channel, Block};
use crate::share_phase::{u128_to_bits, SharePhase, SharePhaseError, SharedRange};
use crate::garbled_circuits::equality_full::{multiple_gb_equality_test, multiple_ev_equality_test};
use crate::data_structures::modint::ModInt;
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use ocelot::ot::{Receiver, Sender};
use crate::{Group, Share};
use serde::{Deserialize, Serialize};

/// Configuration for the check phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConfig {
    pub input_bit_length: usize,
    pub output_bit_length: usize,
    /// Number of dimensions for evaluation
    pub num_dimensions: usize,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
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

    /// Run fuzzy match check for a d-dimensional query point against a shared range
    /// This is specifically designed for the fuzzy matching protocol
    pub fn run_fuzzy_match_check(
        &self,
        shared_range: &SharedRange,
        query_point: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ModInt, CheckPhaseError> {
        if query_point.len() != self.config.num_dimensions {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Query point has {} dimensions but config expects {}", query_point.len(), self.config.num_dimensions)
            ));
        }

        // Step 1: For each dimension i from 0 to d-1, evaluate query_point[i] with OKVS for dimension i
        let mut all_dimension_eval = Vec::<ModInt>::new();
        
        for dim in 0..self.config.num_dimensions {
            // Evaluate at the specific dimension
            let res = self.share_phase.evaluate_at_single_dimension(shared_range, query_point[dim], dim)?;
            all_dimension_eval.push(ModInt::new(res, 1 << self.config.input_bit_length));
        }

        println!("Done evaluating all dimensions");

        // Step 2: Run equality test with the concatenated boolean vector
        let equality_result = if self.config.is_garbler_side {
            let result = multiple_gb_equality_test(rng, channel, &all_dimension_eval);
            result
        } else {
            let result = multiple_ev_equality_test(rng, channel, &all_dimension_eval);
            result
        };
        
        // Step 3: Convert boolean result to ring share using OT
        let ring_share = self.boolean_to_ring_share_modint(
            equality_result,
            1 << self.config.output_bit_length,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;

        println!("Ring share modulo: {}", ring_share.modulus());
        
        Ok(ring_share)
    }


    /// Convert boolean share to ModInt ring share using OT
    pub fn boolean_to_ring_share_modint(
        &self,
        boolean_share: bool,
        modulus: u128,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
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
}
