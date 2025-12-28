use crate::{
    channel::CommTrackingChannel, fuzzy_match::share_phase::SharePhase,
    fuzzy_match::share_types::ShareConfig, fuzzy_match::shared_range::SharedRange,
};
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

        Self { share_phase }
    }

    /// Generate shares for all client points
    /// Returns shares for server 0 and server 1 respectively
    pub fn generate_client_shares(
        &self,
        client_points: &[Vec<u128>],
        delta: u128,
    ) -> Result<(Vec<SharedRange>, Vec<SharedRange>), String> {
        let mut shares_server0 = Vec::new();
        let mut shares_server1 = Vec::new();

        for client_point in client_points {
            let (share0, share1) = self
                .share_phase
                .share_range(client_point, delta)
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
        // Custom serialization for Vec<SharedRange>
        let mut data = Vec::new();
        data.extend_from_slice(&(shares_server0.len() as u64).to_le_bytes());
        for share in &shares_server0 {
            data.extend_from_slice(&share.to_bytes());
        }
        let len_bytes = (data.len() as u64).to_le_bytes();
        channel_server0
            .write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to send length to server 0: {}", e))?;
        channel_server0
            .write_bytes(&data)
            .map_err(|e| format!("Failed to send shares to server 0: {}", e))?;
        channel_server0
            .flush()
            .map_err(|e| format!("Failed to flush to server 0: {}", e))?;

        let mut data = Vec::new();
        data.extend_from_slice(&(shares_server1.len() as u64).to_le_bytes());
        for share in &shares_server1 {
            data.extend_from_slice(&share.to_bytes());
        }
        let len_bytes = (data.len() as u64).to_le_bytes();
        channel_server1
            .write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to send length to server 1: {}", e))?;
        channel_server1
            .write_bytes(&data)
            .map_err(|e| format!("Failed to send shares to server 1: {}", e))?;
        channel_server1
            .flush()
            .map_err(|e| format!("Failed to flush to server 1: {}", e))?;

        Ok(())
    }
}
