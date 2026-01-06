use crate::{
    channel::CommTrackingChannel,
    fuzzy_match::{
        share_phase::SharePhase,
        share_phase_types::{ShareConfig, ShareMethod},
        shared_range::SharedRange,
        sketch_phase::SketchPhase,
        sketch_phase_types::{SketchConfig, SketchDataOwned},
    },
    randomness::prg::PRG,
};
use rand::Rng;
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
    /// Returns shares for server 0 and server 1 respectively, optionally including sketch data
    pub fn generate_client_shares(
        &self,
        client_points: &[Vec<u128>],
        delta: u128,
        enable_sketch: bool,
    ) -> Result<
        (
            Vec<SharedRange>,
            Vec<SharedRange>,
            Option<Vec<Vec<SketchDataOwned>>>,
            Option<Vec<Vec<SketchDataOwned>>>,
        ),
        String,
    > {
        let mut shares_server0 = Vec::new();
        let mut shares_server1 = Vec::new();
        let mut sketches_server0: Option<Vec<Vec<SketchDataOwned>>> = None;
        let mut sketches_server1: Option<Vec<Vec<SketchDataOwned>>> = None;

        let generate_sketch =
            enable_sketch && self.share_phase.config.method == ShareMethod::FSS;
        let sketch_phase = if generate_sketch {
            Some(SketchPhase::new(sketch_config_from_share_config(
                &self.share_phase.config,
            )))
        } else {
            None
        };

        for client_point in client_points {
            let (share0, share1) = self
                .share_phase
                .share_range(client_point, delta)
                .map_err(|e| format!("Failed to generate shares: {:?}", e))?;

            shares_server0.push(share0.clone());
            shares_server1.push(share1.clone());

            if let Some(phase) = &sketch_phase {
                let dims = match &share0 {
                    SharedRange::IntervalFSS { keys, .. } => keys.len(),
                    SharedRange::DistanceFSS { keys, .. } => keys.len(),
                    _ => 0,
                };
                let mut prg =
                    PRG::new(Some(&rand::rng().random::<[u8; 16]>()), 0);
                let mut sketch_vec0 = Vec::with_capacity(dims);
                let mut sketch_vec1 = Vec::with_capacity(dims);
                for _ in 0..dims {
                    let (d0, d1) = phase
                        .get_sketch_data_owned(&mut prg)
                        .map_err(|e| format!("Failed to get sketch data: {}", e))?;
                    sketch_vec0.push(d0);
                    sketch_vec1.push(d1);
                }
                sketches_server0
                    .get_or_insert_with(Vec::new)
                    .push(sketch_vec0);
                sketches_server1
                    .get_or_insert_with(Vec::new)
                    .push(sketch_vec1);
            }
        }

        Ok((shares_server0, shares_server1, sketches_server0, sketches_server1))
    }

    pub fn send_client_shares(
        &self,
        shares_server0: Vec<SharedRange>,
        shares_server1: Vec<SharedRange>,
        sketches_server0: Option<Vec<Vec<SketchDataOwned>>>,
        sketches_server1: Option<Vec<Vec<SketchDataOwned>>>,
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

        let sketch_bytes0 = sketches_server0
            .as_ref()
            .map(|s| bincode::serialize(s).map_err(|e| format!("Failed to serialize sketches0: {}", e)))
            .transpose()?;
        let len0 = sketch_bytes0.as_ref().map(|b| b.len()).unwrap_or(0) as u64;
        channel_server0
            .write_bytes(&len0.to_le_bytes())
            .map_err(|e| format!("Failed to send sketch length to server 0: {}", e))?;
        if let Some(bytes) = sketch_bytes0 {
            channel_server0
                .write_bytes(&bytes)
                .map_err(|e| format!("Failed to send sketches to server 0: {}", e))?;
        }
        channel_server0
            .flush()
            .map_err(|e| format!("Failed to flush sketches to server 0: {}", e))?;

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

        let sketch_bytes1 = sketches_server1
            .as_ref()
            .map(|s| bincode::serialize(s).map_err(|e| format!("Failed to serialize sketches1: {}", e)))
            .transpose()?;
        let len1 = sketch_bytes1.as_ref().map(|b| b.len()).unwrap_or(0) as u64;
        channel_server1
            .write_bytes(&len1.to_le_bytes())
            .map_err(|e| format!("Failed to send sketch length to server 1: {}", e))?;
        if let Some(bytes) = sketch_bytes1 {
            channel_server1
                .write_bytes(&bytes)
                .map_err(|e| format!("Failed to send sketches to server 1: {}", e))?;
        }
        channel_server1
            .flush()
            .map_err(|e| format!("Failed to flush sketches to server 1: {}", e))?;

        Ok(())
    }
}

fn sketch_config_from_share_config(share_config: &ShareConfig) -> SketchConfig {
    SketchConfig {
        h1: share_config.h1,
        h2: share_config.h2,
        q: share_config.sketch_modulus,
        delta: share_config.delta,
        d: share_config.d,
        method: share_config.method.clone(),
        metric: share_config.metric.clone(),
        dictionary_type: share_config.dictionary_type.clone(),
    }
}
