use crossbeam::thread;

use crate::{
    channel::CommTrackingChannel,
    fss::dpf::DpfKey,
    fuzzy_match::share_phase::ShareConfig, 
    util::u128_to_bits_msb,
};

const BATCH_SIZE_PER_CORE: usize = 10;

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
        query_points: &[Vec<u128>],
        thread_pool: &ThreadPool,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, anyhow::Error> {
        let num_threads = other_server_channels.len();

        let modulus = 1u128 << self.share_config.h2;

        let mut query_points_bits: Vec<Vec<bool>> = vec![vec![]; query_points.len()];
        thread_pool.install(|| {
            query_points_bits
                .par_iter_mut()
                .zip(query_points.par_iter())
                .for_each(|(bits, point)| {
                    let point_bits = point.iter()
                        .map(|x| u128_to_bits_msb(x, self.share_config.h1))
                        .collect::<Vec<Vec<bool>>>();
                    for i in 0..self.share_config.h1 {
                        for dimension in 0..self.share_config.d {
                            bits.push(point_bits[dimension][i]);
                        }
                    }
                });
        });

        let mut server_bits = vec![false; query_points_bits.len()];
        server_bits
            .chunks_mut(num_threads * BATCH_SIZE_PER_CORE)
            .zip(query_points_bits.chunks(num_threads * BATCH_SIZE_PER_CORE))
            .try_for_each(|(server_bits_chunk, query_points_bit_chunk)| {
                let mut evals = Vec::new();
                let chunk_size = (query_points_chunk.len() + num_threads - 1) / num_threads;
                for query_point_bits in query_points_bits_chunk {
                    let mut aggregated_eval = 0u128;
                    thread_pool.install(|| {
                        let point_evals = client_shares
                            .par_iter()
                            .map(|dpf_key| {
                                dpf_key.eval_pdf(&query_point_bits, modulus)
                            })
                            .collect::<Vec<u128>>();
                        aggregated_eval = _point_evals.iter()
                            .fold(0u128, |acc, &x| (acc + x) % modulus);
                    });
                    evals.push(aggregated_eval);
                }
                // TODO batch check
            }).map_err(|e| anyhow::anyhow!("Error during parallel processing: {}", e))?;
    }

    fn batch_check(
        &self,
        evals: &[u128],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, anyhow::Error> {
        // Implement batch checking logic here
        Ok(vec![false; evals.len()]) // Placeholder
    }
}