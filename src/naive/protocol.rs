use crate::{
    channel::CommTrackingChannel,
    data_structures::mod2k::Mod2k,
    fss::dpf::DpfKey,
    fuzzy_match::share_phase_types::ShareConfig,
    garbled_circuits::greater_than_or_equal_threshold::{
        multiple_ev_greater_than_ss, multiple_gb_greater_than_ss,
    },
    util::u128_to_bits_msb,
};
use rayon::prelude::*;
use rayon::ThreadPool;
use scuttlebutt::{AbstractChannel, AesRng};
use std::convert::TryInto;

pub struct NaiveProtocol {
    share_config: ShareConfig,
    side: bool,
    threshold: u128,
}

impl NaiveProtocol {
    pub fn new(share_config: ShareConfig, side: bool, threshold: u128) -> Self {
        NaiveProtocol {
            share_config,
            side,
            threshold,
        }
    }

    pub fn receive_client_shares(
        &self,
        client_channel: &mut CommTrackingChannel,
    ) -> Result<Vec<DpfKey>, anyhow::Error> {
        let mut len_bytes = [0u8; 8];
        client_channel
            .read_bytes(&mut len_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to read length of client shares: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;

        let mut shares_data = vec![0u8; len];
        client_channel
            .read_bytes(&mut shares_data)
            .map_err(|e| anyhow::anyhow!("Failed to read client shares: {}", e))?;

        let bytes = &shares_data[..];
        let mut offset = 0;
        let mut dpf_keys = Vec::new();
        let count = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;
        let modulus = 1u128 << self.share_config.h2;
        for _ in 0..count {
            let (key, read_bytes) = DpfKey::from_bytes(&bytes[offset..], modulus)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize DPF key: {}", e))?;
            dpf_keys.push(key);
            offset += read_bytes;
        }
        Ok(dpf_keys)
    }

    pub fn run_server_known_dictionary_parallel(
        &self,
        client_shares: &[DpfKey],
        query_points: &[Vec<u128>],
        thread_pool: &ThreadPool,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<bool>, anyhow::Error> {
        let num_threads = other_server_channels.len();

        let mut query_points_bits: Vec<Vec<bool>> = vec![vec![]; query_points.len()];
        thread_pool.install(|| {
            query_points_bits
                .par_iter_mut()
                .zip(query_points.par_iter())
                .for_each(|(bits, point)| {
                    let point_bits = point
                        .iter()
                        .map(|&x| u128_to_bits_msb(x, self.share_config.h1))
                        .collect::<Vec<Vec<bool>>>();
                    for i in 0..self.share_config.h1 {
                        for dimension in 0..self.share_config.d {
                            bits.push(point_bits[dimension][i]);
                        }
                    }
                });
        });
        println!("Query points converted to bits.");

        let chunk_size = (client_shares.len() + num_threads - 1) / num_threads;
        let mut server_bits = vec![false; query_points_bits.len()];
        let modulus = 1u128 << self.share_config.h2;

        thread_pool.install(|| {
            server_bits.par_chunks_mut(chunk_size)
                .zip(query_points_bits.par_chunks(chunk_size))
                .zip(other_server_channels.par_iter_mut())
                .try_for_each(|((server_bits_chunk, query_points_bits_chunk), other_channel)| -> Result<(), anyhow::Error> {
                    let mut count_shares: Vec<Mod2k> = Vec::with_capacity(server_bits_chunk.len());
                    for (_server_bit, query_bits) in server_bits_chunk.iter_mut().zip(query_points_bits_chunk.iter()) {
                        let mut acc = 0u128;
                        for client_share in client_shares.iter() {
                            let eval = client_share.eval_dpf(query_bits, modulus)?;
                            acc = (acc + eval[0]) % modulus;
                        }
                        let acc_modint = Mod2k::new(acc, modulus);
                        count_shares.push(acc_modint);
                        println!("Computed count share: {}", acc);
                    }

                    let mut local_rng = AesRng::new();
                    let threshold = Mod2k::new(self.threshold, modulus);
                    let comparison_result = if self.side {
                        multiple_gb_greater_than_ss(&mut local_rng, other_channel, &count_shares, &threshold)
                    } else {
                        // Evaluator side - gets the actual comparison result
                        multiple_ev_greater_than_ss(&mut local_rng, other_channel, &count_shares)
                    };
                    for (i, server_bit) in server_bits_chunk.iter_mut().enumerate() {
                        *server_bit = comparison_result[i];
                    }
                    Ok(())
                })
        })?;
        Ok(server_bits)
    }
}
