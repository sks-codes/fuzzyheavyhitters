//! FuzzyMatch Protocol Implementation
//!
//! This module implements a complete fuzzy matching protocol with three phases:
//! 1. Share Phase: Clients share their d-dimensional points using OKVS
//! 2. Check Phase: Servers check if their query point y matches client shares
//! 3. Threshold Phase: Servers aggregate matches and compare against threshold

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::convert::{TryFrom, TryInto};
use scuttlebutt::{AesRng, Channel, Block};
use serde::{Deserialize, Serialize};

use crate::share_phase::{SharePhase, ShareConfig, SharedRange, SharePhaseError};
use crate::check_phase::{CheckPhase, CheckConfig, CheckPhaseError};
use crate::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdPhaseError, MatchResult, ThresholdResult};
use crate::garbled_circuits::equality::{multiple_gb_equality_test, multiple_ev_equality_test};
use crate::data_structures::modint::ModInt;
use crate::{Share, Group};

/// Configuration for FuzzyMatch protocol
#[derive(Clone)]
pub struct FuzzyMatchConfig {
    pub share_config: ShareConfig,
    pub check_config: CheckConfig,
    pub threshold_config: ThresholdConfig,
    pub l2_threshold: f64,
}

/// Error types for FuzzyMatch operations
#[derive(Debug, Clone)]
pub enum FuzzyMatchError {
    /// Error during share phase
    SharePhaseError(SharePhaseError),
    /// Error during check phase
    CheckPhaseError(CheckPhaseError),
    /// Error during threshold phase
    ThresholdPhaseError(ThresholdPhaseError),
    /// Channel communication error
    ChannelError(String),
    /// Invalid configuration
    InvalidConfig(String),
    /// OT error
    OTError(String),
    /// Conversion error
    ConversionError(String),
}

impl From<SharePhaseError> for FuzzyMatchError {
    fn from(error: SharePhaseError) -> Self {
        FuzzyMatchError::SharePhaseError(error)
    }
}

impl From<CheckPhaseError> for FuzzyMatchError {
    fn from(error: CheckPhaseError) -> Self {
        FuzzyMatchError::CheckPhaseError(error)
    }
}

impl From<ThresholdPhaseError> for FuzzyMatchError {
    fn from(error: ThresholdPhaseError) -> Self {
        FuzzyMatchError::ThresholdPhaseError(error)
    }
}

impl std::fmt::Display for FuzzyMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FuzzyMatchError::SharePhaseError(err) => write!(f, "Share phase error: {}", err),
            FuzzyMatchError::CheckPhaseError(err) => write!(f, "Check phase error: {:?}", err),
            FuzzyMatchError::ThresholdPhaseError(err) => write!(f, "Threshold phase error: {:?}", err),
            FuzzyMatchError::ChannelError(msg) => write!(f, "Channel error: {}", msg),
            FuzzyMatchError::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
            FuzzyMatchError::OTError(msg) => write!(f, "OT error: {}", msg),
            FuzzyMatchError::ConversionError(msg) => write!(f, "Conversion error: {}", msg),
        }
    }
}

impl std::error::Error for FuzzyMatchError {}

/// Main FuzzyMatch protocol implementation
pub struct FuzzyMatch {
    config: FuzzyMatchConfig,
    share_phase: SharePhase,
    check_phase: CheckPhase,
    threshold_phase: ThresholdPhase,
}

impl FuzzyMatch {
    /// Create a new FuzzyMatch protocol instance
    pub fn new(config: FuzzyMatchConfig) -> Self {
        let share_phase = SharePhase::new(config.share_config.clone());
        let check_phase = CheckPhase::new(config.check_config.clone(), share_phase.clone());
        let threshold_phase = ThresholdPhase::new(config.threshold_config.clone());
        
        Self {
            config,
            share_phase,
            check_phase,
            threshold_phase,
        }
    }

    /// Share Phase: Client shares their d-dimensional point x
    pub fn share_point(&self, x: &[u128], delta: u128) -> Result<(SharedRange, SharedRange), FuzzyMatchError> {
        if x.len() != self.config.share_config.dimension {
            return Err(FuzzyMatchError::InvalidConfig(
                format!("Input point has {} dimensions but config expects {}", x.len(), self.config.share_config.dimension)
            ));
        }
        
        let (share0, share1) = self.share_phase.share_range(x, delta)?;
        Ok((share0, share1))
    }

    /// Check Phase: Server checks if query point y matches a client's shared point
    /// Returns a ring share representing the match result
    pub fn check_match(
        &self,
        shared_range: &SharedRange,
        y: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ModInt, FuzzyMatchError> {
        if y.len() != self.config.share_config.dimension {
            return Err(FuzzyMatchError::InvalidConfig(
                format!("Query point has {} dimensions but config expects {}", y.len(), self.config.share_config.dimension)
            ));
        }

        // Step 1 & 2: Use check_phase to perform fuzzy match check (evaluation + garbled circuits)
        let ring_share = self.check_phase.run_fuzzy_match_check(
            shared_range,
            y,
            channel,
            rng,
        )?;
        
        Ok(ring_share)
    }

    /// Process multiple clients and return match results for each
    pub fn check_multiple_clients(
        &self,
        client_shares: &[SharedRange],
        y: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<MatchResult<ModInt>>, FuzzyMatchError> {
        let mut results = Vec::new();
        
        for (client_id, shared_range) in client_shares.iter().enumerate() {
            let ring_share = self.check_match(shared_range, y, channel, rng)?;
            results.push(MatchResult {
                ring_share,
                client_id,
            });
        }
        
        Ok(results)
    }

    /// Threshold Phase: Aggregate match results and compare with threshold
    pub fn threshold_check(
        &self,
        match_results: &[MatchResult<ModInt>],
        threshold: u128,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ThresholdResult, FuzzyMatchError> {
        // Convert u128 threshold to ModInt
        let threshold_modint = ModInt::new(threshold, self.config.threshold_config.modulus);
        
        // Delegate to the threshold_phase
        let result = self.threshold_phase.compare_with_threshold(
            match_results,
            threshold_modint,
            channel,
            rng,
        )?;
        
        Ok(result)
    }

    /// Complete protocol: Run all three phases for multiple clients
    pub fn run_complete_protocol(
        &self,
        client_shares: &[SharedRange],
        query_point: &[u128],
        threshold: u128,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ThresholdResult, FuzzyMatchError> {
        // Check phase for all clients
        let match_results = self.check_multiple_clients(client_shares, query_point, channel, rng)?;
        
        // Threshold phase
        let threshold_result = self.threshold_check(&match_results, threshold, channel, rng)?;
        
        Ok(threshold_result)
    }

    /// Get configuration
    pub fn config(&self) -> &FuzzyMatchConfig {
        &self.config
    }
}