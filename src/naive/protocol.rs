use crate::{
    channel::CommTrackingChannel,
    data_structures::mod2k::Mod2k,
    fss::dpf::{DpfEval, DpfKey},
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
use anyhow::{anyhow, Result};

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
                        let acc_modint = if self.side {
                            Mod2k::new((modulus - acc) % modulus, modulus)
                        } else {
                            Mod2k::new(acc, modulus)
                        };
                        count_shares.push(acc_modint);
                    }

                    let mut local_rng = AesRng::new();
                    let threshold = Mod2k::new(self.threshold, modulus);
                    let comparison_result = if self.side {
                        multiple_gb_greater_than_ss(&mut local_rng, other_channel, &count_shares, &threshold)
                    } else {
                        // Evaluator side - gets the actual comparison result
                        multiple_ev_greater_than_ss(&mut local_rng, other_channel, &count_shares)
                    };
                    let comparison_result =
                        self.reveal_comparison_results(other_channel, &comparison_result)?;
                    for (i, server_bit) in server_bits_chunk.iter_mut().enumerate() {
                        *server_bit = comparison_result[i];
                    }
                    Ok(())
                })
        })?;
        Ok(server_bits)
    }

    pub fn run_server_unknown_dictionary_parallel(
        &self,
        client_shares: &[DpfKey],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<Vec<Vec<u128>>> {
        let max_bit_length = self.share_config.h1;
        let dimension = self.share_config.d;
        let eval_modulus = 1u128 << self.share_config.h2;

        // Evaluate the empty prefix for each dimension
        let empty_string_data = client_shares
            .iter()
            .map(|dpf_key| {
                let data = dpf_key.init_eval(eval_modulus)?;
                data.to_bytes()
            })
            .collect::<Result<Vec<Vec<u8>>>>()
            .map_err(|e| anyhow!("Cannot obtain empty string data: {}", e))?;

        let mut current_data = vec![empty_string_data];
        let mut current_prefixes: Vec<Vec<bool>> = vec![vec![]];

        let num_threads = other_server_channels.len();
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();

        // Since this is just testing equality for dpf, we can flatten a multi-dimensional point into a single long string.
        for prefix_length in 1..=max_bit_length*dimension {
            println!("expanding to prefix length {}", prefix_length);
            let mut new_data: Vec<Vec<Vec<u8>>> = vec![];
            for data in current_data.iter() {
                let mut data0 = Vec::with_capacity(client_shares.len());
                let mut data1 = Vec::with_capacity(client_shares.len());
                for (idx, dpf_key) in client_shares.iter().enumerate() {
                    let (data_dim, _) = DpfEval::from_bytes(&data[idx], eval_modulus)
                        .map_err(|e| anyhow!("Failed to deserialize DPF eval data: {}", e))?;
                    let (eval0, eval1) = dpf_key.expand_prefix(&data_dim, eval_modulus)
                        .map_err(|e| anyhow!("Failed to expand DPF eval data: {}", e))?;
                    data0.push(eval0.to_bytes()?);
                    data1.push(eval1.to_bytes()?);
                }
                new_data.push(data0);
                new_data.push(data1);
            }

            let new_eval = thread_pool.install(|| -> Result<Vec<Vec<u128>>> {
                new_data
                    .par_iter()
                    .map(|data_bytes| {
                        data_bytes
                            .iter()
                            .map(|bytes| {
                                let (eval, _) = DpfEval::from_bytes(bytes, eval_modulus)
                                    .map_err(|e| {
                                        anyhow!("Failed to deserialize DPF eval data: {}", e)
                                    })?;
                                Ok(eval.result()[0])
                            })
                            .collect::<Result<Vec<u128>>>()
                    })
                    .collect::<Result<Vec<Vec<u128>>>>()
            })?;

            let count_shares: Vec<Mod2k> = thread_pool.install(|| {
                new_eval
                    .par_iter()
                    .map(|evals| {
                        let mut acc = 0u128;
                        for &val in evals.iter() {
                            acc = (acc + val) % eval_modulus;
                        }
                        if self.side {
                            Mod2k::new((eval_modulus - acc) % eval_modulus, eval_modulus)
                        } else {
                            Mod2k::new(acc, eval_modulus)
                        }
                    })
                    .collect()
            });
            
            let mut new_prefixes: Vec<Vec<bool>> = vec![];
            for prefix in current_prefixes.iter() {
                let mut prefix0 = prefix.clone();
                prefix0.push(false);
                let mut prefix1 = prefix.clone();
                prefix1.push(true);
                new_prefixes.push(prefix0);
                new_prefixes.push(prefix1); 
            }

            if count_shares.is_empty() {
                current_prefixes = Vec::new();
                break;
            }

            let chunk_size = (count_shares.len() + num_threads - 1) / num_threads;
            let mut server_bits = vec![false; count_shares.len()];

            thread_pool.install(|| {
                server_bits.par_chunks_mut(chunk_size)
                    .zip(count_shares.par_chunks(chunk_size))
                    .zip(other_server_channels.par_iter_mut())
                    .try_for_each(|((server_bits_chunk, count_shares_chunk), other_channel)| -> Result<(), anyhow::Error> {
                        let mut local_rng = AesRng::new();
                        let threshold = Mod2k::new(self.threshold, eval_modulus);
                        let comparison_result = if self.side {
                            multiple_gb_greater_than_ss(&mut local_rng, other_channel, &count_shares_chunk, &threshold)
                        } else {
                            multiple_ev_greater_than_ss(&mut local_rng, other_channel, &count_shares_chunk)
                        };
                        let comparison_result =
                            self.reveal_comparison_results(other_channel, &comparison_result)?;
                        for (i, server_bit) in server_bits_chunk.iter_mut().enumerate() {
                            *server_bit = comparison_result[i];
                        }
                        Ok(())
                    })
            })?;

            current_data = vec![];
            current_prefixes = vec![];
            for (i, &bit) in server_bits.iter().enumerate() {
                if bit {
                    current_data.push(new_data[i].clone());
                    current_prefixes.push(new_prefixes[i].clone());
                }
            }
        }

        let final_heavy_hitters = current_prefixes.iter()
            .map(|bits| {
                let mut point = vec![0u128; dimension];
                for dim in 0..dimension {
                    let mut value = 0u128;
                    for i in 0..max_bit_length {
                        let bit = bits[dim * max_bit_length + i];
                        value = (value << 1) | (bit as u128);
                    }
                    point[dim] = value;
                }
                point
            })
            .collect::<Vec<Vec<u128>>>();

        Ok(final_heavy_hitters)
    }

    fn reveal_comparison_results(
        &self,
        channel: &mut CommTrackingChannel,
        local_results: &[bool],
    ) -> Result<Vec<bool>> {
        if local_results.is_empty() {
            return Ok(Vec::new());
        }

        if self.side {
            let mut mask_bytes = vec![0u8; local_results.len()];
            for (i, &bit) in local_results.iter().enumerate() {
                mask_bytes[i] = bit as u8;
            }
            channel
                .write_bytes(&mask_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to send mask bits: {}", e))?;
            channel
                .flush()
                .map_err(|e| anyhow::anyhow!("Failed to flush mask bits: {}", e))?;
            let mut actual_bytes = vec![0u8; local_results.len()];
            channel
                .read_bytes(&mut actual_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to read actual bits: {}", e))?;
            Ok(actual_bytes.iter().map(|&b| b != 0).collect())
        } else {
            let mut mask_bytes = vec![0u8; local_results.len()];
            channel
                .read_bytes(&mut mask_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to read mask bits: {}", e))?;
            let actual = local_results
                .iter()
                .zip(mask_bytes.iter())
                .map(|(&masked, &mask)| masked ^ (mask != 0))
                .collect::<Vec<bool>>();
            let mut actual_bytes = vec![0u8; actual.len()];
            for (i, &bit) in actual.iter().enumerate() {
                actual_bytes[i] = bit as u8;
            }
            channel
                .write_bytes(&actual_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to send actual bits: {}", e))?;
            channel
                .flush()
                .map_err(|e| anyhow::anyhow!("Failed to flush actual bits: {}", e))?;
            Ok(actual)
        }
    }
}
