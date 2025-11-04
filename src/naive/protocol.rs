use crate::{
    channel::CommTrackingChannel,
    fss::dpf::DpfKey,
    fuzzy_match::share_phase::ShareConfig,
};

pub struct NaiveProtocol {
    share_config: ShareConfig,
}

impl NaiveProtocol {
    pub fn new(share_config: ShareConfig) -> Self {
        NaiveProtocol { share_config }
    }

    pub fn receive_client_shares (
        &self, 
        client_channel: &mut CommTrackingChannel,
    ) -> Result<Vec<DpfKey<1>>, anyhow::Error> {
        let mut len_bytes = [0u8; 8];
        client_channel.read_bytes(&mut len_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to read length of client shares: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;

        let mut shares_data = vec![0u8; len];
        client_channel.read_bytes(&mut shares_data)
            .map_err(|e| anyhow::anyhow!("Failed to read client shares: {}", e))?;

        let bytes = &shares_data[..];
        let mut offset = 0;
        let mut dpf_keys = Vec::new();
        let count = u64::from_le_bytes(bytes[offset..offset+8].try_into().unwrap()) as usize;
        offset += 8;
        let modulus = 1u128 << self.share_config.h2;
        for _ in 0..count {
            let (key, read_bytes) = DpfKey::<1>::from_bytes(&bytes[offset..], modulus)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize DPF key: {}", e))?;
            dpf_keys.push(key);
            offset += read_bytes;
        }
        Ok(dpf_keys)
    }

    pub fn run_server_known_dictionary_parallel(
        &self,
        client_shares: &[DpfKey<1>],
        query_ponts: &[Vec<u128>],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, anyhow::Error> {
        
    }

    fn batch_check(
        &self,
        current_points: &[Vec<u128>],
        other_server_channels: &mut [CommTrackingChannel],
    )
}