//! Fuzzy Heavy Hitters Protocol Implementation
//! 
//! This module provides a high-level interface for running the complete fuzzy heavy hitters protocol
//! including share phase, check phase, and threshold phase.

use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange};
use crate::fuzzy_match::check_phase::{CheckPhase, CheckConfig};
use crate::fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::data_structures::modint::ModInt;
use crate::fss::interval::IntervalFSSKey;
use scuttlebutt::{AesRng, Channel};
use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
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

/// Results from running the protocol
#[derive(Debug, Clone)]
pub struct ProtocolResult {
    /// Whether the threshold was exceeded for each query point
    pub threshold_exceeded: Vec<bool>,
    /// Number of matches for each query point (for debugging)
    pub match_counts: Vec<usize>,
    /// Details about which clients matched for each query (for debugging)
    pub client_matches: Vec<Vec<bool>>,
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

    /// Run the protocol as server 0 (evaluator)
    pub fn run_server0(
        &self,
        client_shares: &[SharedRange],
        query_points: &[Vec<u128>],
        fss_keys: Option<&[IntervalFSSKey<1>]>,
        stream: UnixStream,
    ) -> Result<ProtocolResult, String> {
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let writer = BufWriter::new(stream);
        let mut channel = Channel::new(reader, writer);

        let check_phase = CheckPhase::new(self.config.check_config.clone(), self.share_phase.clone());
        let threshold_phase = ThresholdPhase::new(self.config.threshold_config.clone());

        let mut threshold_exceeded = Vec::new();
        let mut match_counts = Vec::new();
        let mut client_matches = Vec::new();

        for (query_idx, query_point) in query_points.iter().enumerate() {
            // Run check phase for all client shares
            let mut match_results = Vec::new();
            
            for share in client_shares {
                let result = check_phase.run_fuzzy_match_check(
                    share,
                    query_point,
                    &mut channel,
                    &mut rng,
                ).map_err(|e| format!("Check phase failed: {:?}", e))?;
                
                match_results.push(result);
            }

            // Run threshold phase
            let fss_key = if let Some(keys) = fss_keys {
                Some(&keys[query_idx])
            } else {
                None
            };

            let server_bit = threshold_phase.compare_with_threshold(
                &match_results,
                self.config.threshold,
                fss_key,
                &mut channel,
                &mut rng,
            ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

            // For debugging, we'll store intermediate results
            // Note: In a real deployment, you wouldn't reconstruct these for privacy
            let mut client_match_results = Vec::new();
            let mut actual_matches = 0;
            
            // This is just for debugging - normally you wouldn't reconstruct intermediate results
            for result in &match_results {
                // We can't actually reconstruct without the other server's shares
                // This is just a placeholder for the structure
                client_match_results.push(false); // Placeholder
            }

            threshold_exceeded.push(server_bit);
            match_counts.push(actual_matches);
            client_matches.push(client_match_results);
        }

        Ok(ProtocolResult {
            threshold_exceeded,
            match_counts,
            client_matches,
        })
    }

    /// Run the protocol as server 1 (garbler)
    pub fn run_server1(
        &self,
        client_shares: &[SharedRange],
        query_points: &[Vec<u128>],
        fss_keys: Option<&[IntervalFSSKey<1>]>,
        stream: UnixStream,
    ) -> Result<ProtocolResult, String> {
        let mut rng = AesRng::new();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let writer = BufWriter::new(stream);
        let mut channel = Channel::new(reader, writer);

        let check_phase = CheckPhase::new(self.config.check_config.clone(), self.share_phase.clone());
        let threshold_phase = ThresholdPhase::new(self.config.threshold_config.clone());

        let mut threshold_exceeded = Vec::new();
        let mut match_counts = Vec::new();
        let mut client_matches = Vec::new();

        for (query_idx, query_point) in query_points.iter().enumerate() {
            // Run check phase for all client shares
            let mut match_results = Vec::new();
            
            for share in client_shares {
                let result = check_phase.run_fuzzy_match_check(
                    share,
                    query_point,
                    &mut channel,
                    &mut rng,
                ).map_err(|e| format!("Check phase failed: {:?}", e))?;
                
                match_results.push(result);
            }

            // Run threshold phase
            let fss_key = if let Some(keys) = fss_keys {
                Some(&keys[query_idx])
            } else {
                None
            };

            let server_bit = threshold_phase.compare_with_threshold(
                &match_results,
                self.config.threshold,
                fss_key,
                &mut channel,
                &mut rng,
            ).map_err(|e| format!("Threshold phase failed: {:?}", e))?;

            // For debugging, we'll store intermediate results
            let mut client_match_results = Vec::new();
            let mut actual_matches = 0;
            
            // This is just for debugging - normally you wouldn't reconstruct intermediate results
            for result in &match_results {
                // We can't actually reconstruct without the other server's shares
                // This is just a placeholder for the structure
                client_match_results.push(false); // Placeholder
            }

            threshold_exceeded.push(server_bit);
            match_counts.push(actual_matches);
            client_matches.push(client_match_results);
        }

        Ok(ProtocolResult {
            threshold_exceeded,
            match_counts,
            client_matches,
        })
    }
}

/// Generate FSS keys for interval FSS threshold comparison
/// This simulates a trusted dealer generating FSS keys
pub fn generate_fss_keys_for_threshold(
    threshold: u128,
    random_values: (u128, u128), // (r0, r1) - random values for each server
    bit_length: usize,
    num_queries: usize,
) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>), String> {
    let modulus = 1u128 << bit_length;
    let (r0, r1) = random_values;
    
    let mut keys_server0 = Vec::new();
    let mut keys_server1 = Vec::new();
    
    // Check if threshold + r0 + r1 would wrap around
    let sum = threshold + r0 + r1;
    let wraps_around = sum >= modulus;
    
    use crate::data_structures::payload::RingVec;
    
    for _ in 0..num_queries {
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
            let a = RingVec::<1>::new([0], modulus); // left
            let b = RingVec::<1>::new([1], modulus); // middle
            let c = RingVec::<1>::new([0], modulus); // right
            
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
            let a = RingVec::<1>::new([1], modulus); // left
            let b = RingVec::<1>::new([0], modulus); // middle
            let c = RingVec::<1>::new([1], modulus); // right
            
            (alpha_bits, beta_bits, a, b, c)
        };
        
        let (key0, key1) = IntervalFSSKey::gen_IntervalFSSKey(
            &alpha_bits,
            &beta_bits,
            a,
            b,
            c,
            modulus,
        );
        
        keys_server0.push(key0);
        keys_server1.push(key1);
    }
    
    Ok((keys_server0, keys_server1))
}
