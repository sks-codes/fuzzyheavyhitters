//! Client Implementation for Fuzzy Heavy Hitters Protocol
//! 
//! This module implements the client-side functionality for the fuzzy heavy hitters protocol,
//! including generating and distributing shares to servers.

use crate::channel::CommTrackingChannel;
use crate::fuzzy_match::share_phase::{SharePhase, ShareConfig, SharedRange};
use crate::fuzzy_match::protocol::ProtocolConfig;
use serde::{Deserialize, Serialize};
use scuttlebutt::AbstractChannel;

/// Client structure that handles client-side operations
#[derive(Clone)]
pub struct Client {
    share_phase: SharePhase,
}

impl Client {
    /// Create a new client instance
    pub fn new(share_config: ShareConfig) -> Self {
        let share_phase = SharePhase::new(share_config.clone());
        
        Self {
            share_phase,
        }
    }

    /// Generate shares for all client points
    /// Returns shares for server 0 and server 1 respectively
    pub fn generate_client_shares(&self, client_points: &[Vec<u128>], delta: u128) 
        -> Result<(Vec<SharedRange>, Vec<SharedRange>), String> {
        let mut shares_server0 = Vec::new();
        let mut shares_server1 = Vec::new();

        for client_point in client_points {
            let (share0, share1) = self.share_phase.share_range(client_point, delta)
                .map_err(|e| format!("Failed to generate shares: {:?}", e))?;
            
            shares_server0.push(share0);
            shares_server1.push(share1);
        }

        Ok((shares_server0, shares_server1))
    }

    pub fn send_client_shares(
        &self, 
        shares_server0: Vec<SharedRange>,
        shares_server1: Vec<SharedRange>,
        channel_server0: &mut CommTrackingChannel,
        channel_server1: &mut CommTrackingChannel,
    ) -> Result<(), String> {
        // Send shares using binary serialization
        let shares0_data = bincode::serialize(&shares_server0)
            .map_err(|e| format!("Failed to serialize shares for server 0: {}", e))?;
        let shares1_data = bincode::serialize(&shares_server1)
            .map_err(|e| format!("Failed to serialize shares for server 1: {}", e))?;
    
        // Send to server 0
        let len_bytes = (shares0_data.len() as u64).to_le_bytes();
        channel_server0.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to send length to server 0: {}", e))?;
        channel_server0.write_bytes(&shares0_data)
            .map_err(|e| format!("Failed to send shares to server 0: {}", e))?;
        channel_server0.flush()
            .map_err(|e| format!("Failed to flush to server 0: {}", e))?;
    
        // Send to server 1
        let len_bytes = (shares1_data.len() as u64).to_le_bytes();
        channel_server1.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to send length to server 1: {}", e))?;
        channel_server1.write_bytes(&shares1_data)
            .map_err(|e| format!("Failed to send shares to server 1: {}", e))?;
        channel_server1.flush()
            .map_err(|e| format!("Failed to flush to server 1: {}", e))?;
        
        Ok(())
    }
}
