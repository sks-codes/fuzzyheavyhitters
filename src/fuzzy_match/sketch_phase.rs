use crate::data_structures::modp::BarrettCtx;

pub struct Sketch {
    h1: usize, // FSS phase input bit length
    h2: usize, // FSS phase output bit length
    q: u128, // sketching values will be in Zq
    delta: u128, // Distance threshold
    d: usize, // Number of dimensions
    method: ShareMethod, // The sharing method used. Only can sketch for FSS now
    metric: DistanceMetric, // Distance metric. Can support sketching both Linf and Lp
    dictionary_type: DictionaryType, // Known or Unknown
}

impl Sketch {
    pub fn sketch(
        &self,
        shared_ranges: &[SharedRange],
        sketch_data: &[SketchData],
        thread_pool: &ThreadPool,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool, SharePhaseError> {
        // Check the type of all shared ranges
        let first_type = match &shared_ranges[0] {
            SharedRange::OKVS { .. } => "OKVS",
            SharedRange::IntervalFSS { .. } => "IntervalFSS",
            SharedRange::DistanceFSSL1 { .. } => "DistanceFSSL1",
            SharedRange::DistanceFSSL2 { .. } => "DistanceFSSL2",
            SharedRange::DistanceFSSL3 { .. } => "DistanceFSSL3",
        };
        for sr in shared_ranges.iter() {
            let sr_type = match sr {
                SharedRange::OKVS { .. } => "OKVS",
                SharedRange::IntervalFSS { .. } => "IntervalFSS",
                SharedRange::DistanceFSSL1 { .. } => "DistanceFSSL1",
                SharedRange::DistanceFSSL2 { .. } => "DistanceFSSL2",
                SharedRange::DistanceFSSL3 { .. } => "DistanceFSSL3",
            };
            if sr_type != first_type {
                return Err(SharePhaseError::InvalidRange(
                    "All shared ranges must be of the same type for sketching".to_string()
                ));
            }
        }

        let role = match &shared_ranges[0] {
            SharedRange::OKVS { role, .. } => *role,
            SharedRange::IntervalFSS { role, .. } => *role,
            SharedRange::DistanceFSSL1 { role, .. } => *role,
            SharedRange::DistanceFSSL2 { role, .. } => *role,
            SharedRange::DistanceFSSL3 { role, .. } => *role,
        };

        let mut seed = [0u8; AES_KEY_SIZE];
        if role {
            seed = rand::rng().random::<[u8; AES_KEY_SIZE]>();
            other_server_channels[0].write_bytes(&seed).expect("Failed to send seed to other server");
        } else {
            other_server_channels[0].read_bytes(&mut seed).expect("Failed to receive seed from other server");
        }

        // Now sketch based on the type
        match first_type {
            "OKVS" => {
                println!("Sketching for OKVS not implemented yet, so we skip it.");
                Ok(true)
            },
            "IntervalFSS" => self.parallel_sketch_interval_fss(shared_ranges, sketch_data, seed, thread_pool, other_server_channels),
            "DistanceFSSL1" => self.sketch_distance_fss::<2>(shared_ranges),
            "DistanceFSSL2" => self.sketch_distance_fss::<3>(shared_ranges),
            "DistanceFSSL3" => self.sketch_distance_fss::<4>(shared_ranges),
            _ => Err(SharePhaseError::InvalidRange(
                "Unknown shared range type for sketching".to_string()
            )),
        }
    }

    fn get_sketch_values_interval_fss_one_dimension(
        &self,
        key: IntervalFSSKey<1>,
        role: bool,
        seed: [u8; AES_KEY_SIZE],
    ) -> Modp {
        // We do not parallelize at this level. We only parallelize through multiple key pairs
        let domain_range = 1u128 << self.h1;
        let delta = self.delta;
        let barrett_ctx = BarrettCtx::new(self.q);
        
        let ldcf_key = key.ldcf_key();
        let rdcf_key = key.rdcf_key();

        let ldcf_full_evals = ldcf_key.full_domain_incremental_eval();
        let rdcf_full_evals = rdcf_key.full_domain_incremental_eval();

        // Checking whether each level is dcf
        for level in 0..self.h1 {
            let z = self.get_sketch_value_dcf(&ldcf_full_evals[level], 1 << level);
            let z = self.get_sketch_value_dcf(&rdcf_full_evals[level], 1 << level);
        }

        // Checking whether each two consecutive levels are consistent
        for level in 0..(self.h1 - 1) {
            let z = self.get_sketch_value_incremental_ldcf_consistency(&ldcf_full_evals[level], ldcf_full_evals[level+1], 1 << level);
            let z = self.get_sketch_value_incremental_rdcf_consistency(&rdcf_full_evals[level], rdcf_full_evals[level+1], 1 << level);
        }

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let z = self.get_sketch_value_shift_dcf_consistency(&ldcf_full_evals[self.h1-1], &rdcf_full_evals[self.h1-1], delta as usize, domain_range as usize);
    }

    // This function is NOT READY to be used!!!
    fn parallel_sketch_interval_fss(
        &self,
        shared_ranges: &[SharedRange],
        sketch_data: &[SketchData],
        seed: [u8; AES_KEY_SIZE],
        thread_pool: &ThreadPool,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool, SharePhaseError> {
        // First just do full domain evaluation
        // Check dcf by shifting by one, and subtract, to reduce to dpf. Don't need to pad one to the left, since we only want to check if the whole interval is equal to each other, don't care about the payload.
        // Check interval equals delta by shifting with delta and minus. The lefter dcf does not need to pad to the left, since we already checked that the payload inside the interval is the same. 
        // In this code, we just use Aes256 in ctr mode as random oracle
        // WARNING! Only works when the input range is smaller than 60 bits

        let domain_range = 1u128 << self.config.h1;
        let role = shared_ranges[0].role();
        let delta = self.config.delta;
        let sketch_modulus = self.config.sketch_modulus;
        let out_modulus = 1 << self.config.h2;

        let mut z1s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z2s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z3s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z4s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z5s = vec![vec![0u128; self.config.d]; shared_ranges.len()];

        // Compute all the sketches first. Will exchange message and prove things later.
        thread_pool.install(|| {
            shared_ranges.par_iter()
                .zip(z1s.par_iter_mut())
                .zip(z2s.par_iter_mut())
                .zip(z3s.par_iter_mut())
                .zip(z4s.par_iter_mut())
                .zip(z5s.par_iter_mut())
                .enumerate()
                .for_each(|(i, (((((shared_range, z1_row), z2_row), z3_row), z4_row), z5_row))| {
                    match shared_range {
                        SharedRange::IntervalFSS { keys, .. } => {
                            // Create a seed for this index only, by xoring the global seed with the index
                            let mut blocks = vec![[0u8; 16]; domain_range as usize * self.config.d];
                            let mut prg = PRG::new(Some(&seed), (i+5) as u64);
                            prg.random_16byte_block(&mut blocks);
                            // Generate random values and their powers in Zp
                            let rs = blocks.iter().map(|&b| {
                                let num = u128::from_le_bytes(b);
                                num % sketch_modulus
                            }).collect::<Vec<u128>>();
                            let rs2 = rs.iter()
                                .map(|&r| mul_mod(r, r, sketch_modulus))
                                .collect::<Vec<u128>>(); // ri^2
                            let rs3 = rs.iter().zip(rs2.iter())
                                .map(|(&r, &r2)| mul_mod(r, r2, sketch_modulus))
                                .collect::<Vec<u128>>(); // ri^3
                            let rs4 = rs2.iter()
                                .map(|&r2| mul_mod(r2, r2, sketch_modulus))
                                .collect::<Vec<u128>>(); // ri^4
                            let rs6 = rs3.iter()
                                .map(|&r3| mul_mod(r3, r3, sketch_modulus))
                                .collect::<Vec<u128>>(); // ri^6
                            // There are d dimensions

                            for dimension in 0..self.config.d {
                                let key = &keys[dimension];
                                let ldcf_evals = key.full_domain_eval_ldcf(out_modulus, self.config.h1).iter()
                                    .map(|eval| eval[0])
                                    .collect::<Vec<u128>>();
                                let rdcf_evals = key.full_domain_eval_rdcf(out_modulus, self.config.h1).iter()
                                    .map(|eval| eval[0])
                                    .collect::<Vec<u128>>();

                                // CHECK LDCF
                                // Shift by one and subtract to turn into DPF, still working in Z_2
                                // The difference of shares of two parties should be 0 at everywhere except one point.
                                // At that point, the difference would be either -1 or 1 in Zp.
                                let ldcf_evals1 = (1..ldcf_evals.len())
                                    .map(|j| {
                                        (ldcf_evals[j] + out_modulus - ldcf_evals[j - 1]) % out_modulus
                                    })
                                    .collect::<Vec<u128>>();
                                let range_start = domain_range as usize * dimension;
                                let range_end = domain_range as usize * (dimension + 1);
                                // Multiply with the sketching random values, now working in Zp
                                z1_row[dimension] = ldcf_evals1.iter()
                                    .zip(rs[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r)| {
                                        (acc + eval * r) % sketch_modulus
                                    }); // sum of ri * evali. This sum should be equal to either ri or -ri, where i is the non-zero position.
                                z2_row[dimension] = ldcf_evals1.iter()
                                    .zip(rs2[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r2)| {
                                        (acc + eval * r2) % sketch_modulus
                                    }); // sum of ri^2 * evali. This sum should be equal to either ri^2 or -ri^2, where i is the non-zero position.
                                // We will want to check that z1^4 == z2^2 later.
                                
                                // CHECK RDCF
                                // Shift by one and subtract to turn into DPF, still working in Z_2
                                let rdcf_evals1 = (1..rdcf_evals.len())
                                    .map(|j| {
                                        (rdcf_evals[j] + out_modulus - rdcf_evals[j - 1]) % out_modulus
                                    })
                                    .collect::<Vec<u128>>();
                                // Multiply with the sketching random values, now working in Zp
                                z3_row[dimension] = rdcf_evals1.iter()
                                    .zip(rs3[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r3)| {
                                        (acc + eval * r3) % sketch_modulus
                                    }); // sum of ri^3 * evali. This sum should be equal to either ri^3 or -ri^3, where i is the non-zero position.
                                // Multiply with the sketching random values, now working in Zp
                                z4_row[dimension] = rdcf_evals1.iter()
                                    .zip(rs6[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r6)| {
                                        (acc + eval * r6) % sketch_modulus 
                                    }); // sum of ri^6 * evali. This sum should be equal to either ri^6 or -ri^6, where i is the non-zero position.
                                // We will want to check that z3^2 == z4 later.

                                // Check whether the interval is smaller than or equal to 2 * delta by shifting rdcf by 2 * delta and subtracting
                                // Shift rdcf by 2 * delta and subtract by ldcf. Working in Z2.
                                let rdcf_ldcf_evals = (0..rdcf_evals1.len() - (2 * delta as usize))
                                    .map(|j| {
                                        (rdcf_evals[j + 2 * delta as usize] + out_modulus - ldcf_evals1[j]) % out_modulus
                                    })
                                    .collect::<Vec<u128>>();
                                // Multiply with the sketching random values, now working in Zp
                                z5_row[dimension] = rdcf_ldcf_evals.iter()
                                    .zip(rs4[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r4)| {
                                        (acc + eval * r4) % sketch_modulus
                                    }); // sum of ri * evali. This sum should just be equal to 0.
                                // We will want to check that z5 == 0 later.
                            }
                        }
                        _ => {
                            panic!("Sketching failed: Expected IntervalFSS shared range");
                        }
                    }
                })
            });
        
        // Now that we have all the sketches, exchange messages and prove things
        // 1. z1^4 == z2^2
        // 2. z3^4 == z4^2
        // 3. z5 == 0

        // First step, obtain noised values from given client's sketch data
        // We don't need to noise z5s since the two server's shares should just be equal
        let mut noised_z1s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut noised_z2s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut noised_z3s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut noised_z4s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut my_z5s = z5s.clone(); // Noising not needed for z5s
        
        thread_pool.install(|| {
            sketch_data.par_iter()
                .zip(noised_z1s.par_iter_mut())
                .zip(noised_z2s.par_iter_mut())
                .zip(noised_z3s.par_iter_mut())
                .zip(noised_z4s.par_iter_mut())
                .enumerate()
                .for_each(|(i, ((((sketch, noised_z1_row), noised_z2_row), noised_z3_row), noised_z4_row))| {
                    match sketch {
                        SketchData::IntervalFSS { a, b, .. } => { 
                            for dimension in 0..self.config.d {
                                noised_z1_row[dimension] = (z1s[i][dimension] + a[dimension]) % sketch_modulus;
                                noised_z2_row[dimension] = (z2s[i][dimension] + b[dimension]) % sketch_modulus;
                                noised_z3_row[dimension] = (z3s[i][dimension] + a[dimension + self.config.d]) % sketch_modulus;
                                noised_z4_row[dimension] = (z4s[i][dimension] + b[dimension + self.config.d]) % sketch_modulus;
                            }
                        }
                    }
                });
        });

        // Exchange noised values
        let mut plain_z1s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut plain_z2s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut plain_z3s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut plain_z4s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut their_z5s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        if role {
            send_array(&mut other_server_channels[0], &noised_z1s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z2s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z3s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z4s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &my_z5s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z1s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z2s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z3s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z4s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut their_z5s, shared_ranges.len(), self.config.d)?;
        } else {
            recv_array(&mut other_server_channels[0], &mut plain_z1s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z2s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z3s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut plain_z4s, shared_ranges.len(), self.config.d)?;
            recv_array(&mut other_server_channels[0], &mut their_z5s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z1s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z2s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z3s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &noised_z4s, shared_ranges.len(), self.config.d)?;
            send_array(&mut other_server_channels[0], &my_z5s, shared_ranges.len(), self.config.d)?;
        }

        // The plaintexts obtained in this step are:
        // z1 + a
        // z2 + b
        // z3 + a
        // z4 + b
        // z5 + 0
        thread_pool.install(|| {
            plain_z1s.par_iter_mut()
                .zip(plain_z2s.par_iter_mut())
                .zip(plain_z3s.par_iter_mut())
                .zip(plain_z4s.par_iter_mut())
                .zip(my_z5s.par_iter_mut())
                .zip(z1s.par_iter())
                .zip(z2s.par_iter())
                .zip(z3s.par_iter())
                .zip(z4s.par_iter())
                .zip(their_z5s.par_iter())
                .for_each(|(((((((((plain_z1_row, plain_z2_row), plain_z3_row), plain_z4_row), my_z5_row), z1_row), z2_row), z3_row), z4_row), their_z5_row)| {
                    for dimension in 0..self.config.d {
                        if role {
                            // Server 1: subtract own share
                            plain_z1_row[dimension] = (plain_z1_row[dimension] + sketch_modulus - z1_row[dimension]) % sketch_modulus;
                            plain_z2_row[dimension] = (plain_z2_row[dimension] + sketch_modulus - z2_row[dimension]) % sketch_modulus;
                            plain_z3_row[dimension] = (plain_z3_row[dimension] + sketch_modulus - z3_row[dimension]) % sketch_modulus;
                            plain_z4_row[dimension] = (plain_z4_row[dimension] + sketch_modulus - z4_row[dimension]) % sketch_modulus;
                            my_z5_row[dimension] = (my_z5_row[dimension] + sketch_modulus - their_z5_row[dimension]) % sketch_modulus;
                        } else {
                            // Server 0: subtract own share
                            plain_z1_row[dimension] = (z1_row[dimension] + sketch_modulus - plain_z1_row[dimension]) % sketch_modulus;
                            plain_z2_row[dimension] = (z2_row[dimension] + sketch_modulus - plain_z2_row[dimension]) % sketch_modulus;
                            plain_z3_row[dimension] = (z3_row[dimension] + sketch_modulus - plain_z3_row[dimension]) % sketch_modulus;
                            plain_z4_row[dimension] = (z4_row[dimension] + sketch_modulus - plain_z4_row[dimension]) % sketch_modulus;
                            my_z5_row[dimension] = (their_z5_row[dimension] + sketch_modulus - my_z5_row[dimension]) % sketch_modulus;
                        }
                    }
                });
        });

        // Now start doing local computation
        // 1. z1^4 == z2^2
        // z1^4 - z2^2 = (Z1-a)^4 - (Z2-b)^2 = Z1^4 - 4a Z1^3 + a2 Z1^2 + a3 Z1 + a4 - Z2^2 + 2b Z2
        // Just store the computed result back to plain_z1s
        thread_pool.install(|| { 
            plain_z1s.par_iter_mut()
                .zip(plain_z2s.par_iter())
                .zip(sketch_data.par_iter())
                .for_each(|((plain_z1_row, plain_z2_row), sketch)| {
                    match sketch {
                        SketchData::IntervalFSS { a, a2, a3, a4, b } => {
                            for dimension in 0..self.config.d {
                                let z1 = plain_z1_row[dimension];
                                let z12 = mul_mod(z1, z1, sketch_modulus);
                                let z13 = mul_mod(z12, z1, sketch_modulus);
                                let z14 = mul_mod(z13, z1, sketch_modulus);
                                let z2 = plain_z2_row[dimension];
                                let z22 = mul_mod(z2, z2, sketch_modulus);

                                let term1 = z14;
                                let term2 = (sketch_modulus - mul_mod((4 * a[dimension]) % sketch_modulus, z13, sketch_modulus)) % sketch_modulus;
                                let term3 = mul_mod(a2[dimension], z12, sketch_modulus);
                                let term4 = mul_mod(a3[dimension], z1, sketch_modulus);
                                let term5 = a4[dimension];
                                let term6 = (sketch_modulus - z22) % sketch_modulus;
                                let term7 = (2 * b[dimension]) % sketch_modulus;
                                plain_z1_row[dimension] = (term1 + term2 + term3 + term4 + term5 + term6 + term7) % sketch_modulus;
                            }
                        }
                    }
                });
        });

        // 2. z3^4 == z4^2
        // z3^4 - z4^2 = (Z3-a)^4 - (Z4-b)^2 = Z3^4 - 4a Z3^3 + a2 Z3^2 + a3 Z3 + a4 - Z4^2 + 2b Z4
        // Just store the computed result back to plain_z3s
        thread_pool.install(|| { 
            plain_z3s.par_iter_mut()
                .zip(plain_z4s.par_iter())
                .zip(sketch_data.par_iter())
                .for_each(|((plain_z3_row, plain_z4_row), sketch)| {
                    match sketch {
                        SketchData::IntervalFSS { a, a2, a3, a4, b } => {
                            for dimension in 0..self.config.d {
                                let z3 = plain_z3_row[dimension];
                                let z32 = mul_mod(z3, z3, sketch_modulus);
                                let z33 = mul_mod(z32, z3, sketch_modulus);
                                let z34 = mul_mod(z33, z3, sketch_modulus);
                                let z4 = plain_z4_row[dimension];
                                let z42 = mul_mod(z4, z4, sketch_modulus);

                                let term1 = z34;
                                let term2 = (sketch_modulus - mul_mod((4 * a[dimension + self.config.d]) % sketch_modulus, z33, sketch_modulus)) % sketch_modulus;
                                let term3 = mul_mod(a2[dimension + self.config.d], z32, sketch_modulus);
                                let term4 = mul_mod(a3[dimension + self.config.d], z3, sketch_modulus);
                                let term5 = a4[dimension + self.config.d];
                                let term6 = (sketch_modulus - z42) % sketch_modulus;
                                let term7 = (2 * b[dimension + self.config.d]) % sketch_modulus;
                                plain_z3_row[dimension] = (term1 + term2 + term3 + term4 + term5 + term6 + term7) % sketch_modulus;
                            }
                        }
                    }
                });
        });

        let mut blocks = vec![[0u8; 16]; self.config.d * shared_ranges.len() * 3];
        let mut prg = PRG::new(Some(&seed), 0u64);
        prg.random_16byte_block(&mut blocks);

        // Finally, put everything into one big linear combination and check
        let mut combined = vec![0u128; shared_ranges.len()];
        thread_pool.install(|| {
            combined.par_iter_mut()
                .zip(plain_z1s.par_iter())
                .zip(plain_z3s.par_iter())
                .zip(my_z5s.par_iter())
                .enumerate()
                .for_each(|(i, (((combined_elem, plain_z1_row), plain_z3_row), my_z5_row))| {
                    for dimension in 0..self.config.d {
                        let block_start = i * self.config.d * 3 + dimension * 3;
                        let r1 = u128::from_le_bytes(blocks[block_start]);
                        let r2 = u128::from_le_bytes(blocks[block_start + 1]);
                        let r3 = u128::from_le_bytes(blocks[block_start + 2]);

                        *combined_elem = (*combined_elem
                            + mul_mod(plain_z1_row[dimension], r1, sketch_modulus)
                            + mul_mod(plain_z3_row[dimension], r2, sketch_modulus)
                            + mul_mod(my_z5_row[dimension], r3, sketch_modulus)
                        ) % sketch_modulus;
                    }
                });
        
        let final_check: u128 = combined.iter().fold(0u128, |acc, &x| (acc + x) % sketch_modulus);
        let other_check = if role {
                let other_check = recv_u128(&mut other_server_channels[0]).expect("Failed to receive final check from other server");
                send_u128(&mut other_server_channels[0], final_check).expect("Failed to send final check to other server");
                other_check
            } else {
                send_u128(&mut other_server_channels[0], final_check).expect("Failed to send final check to other server");
                recv_u128(&mut other_server_channels[0]).expect("Failed to receive final check from other server")
            };
        if final_check != other_check {
            panic!("Sketching verification failed: final check values do not match");
        }
        });

        Ok(true)
    }

    fn sketch_distance_fss<const N: usize>(
        &self,
        _shared_ranges: &[SharedRange],
    ) -> Result<bool, SharePhaseError> {
        unimplemented!()
    }

    fn difference(
        &self,
        evals: &[u128],
        target_length: usize,
    ) -> Vec<u128> {

    }
}