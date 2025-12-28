use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use crate::bytes_to_u128;
use crate::data_structures::mod2k::Mod2k;
use crate::data_structures::ringvec::RingVec;
use crate::util::{and_bit, xor};

use rand::Rng;
use rand_core::RngCore;
use std::cell::RefCell;
use std::convert::TryInto;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

#[derive(Clone, Debug, PartialEq)]
pub struct RdcfCW<const N: usize> {
    pub seed: [u8; AES_BLOCK_SIZE],
    pub seed_bit: (bool, bool),
    pub ys: (RingVec, RingVec),
    pub y_bits: (Mod2k, Mod2k),
}

impl<const N: usize> RdcfCW<N> {
    pub fn bytes_size(modulus: u128) -> usize {
        let mut size = 0;
        size += AES_BLOCK_SIZE;
        size += 1; // for seed_bit
        size += 2 * RingVec::byte_size_for_modulus_len(N, modulus);
        size += RingVec::byte_size_for_modulus_len(2, modulus);
        size
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.seed);
        let mut bits_byte = 0u8;
        bits_byte |= (self.seed_bit.0 as u8) << 0;
        bits_byte |= (self.seed_bit.1 as u8) << 1;
        out.push(bits_byte);
        out.extend_from_slice(&self.ys.0.to_bytes());
        out.extend_from_slice(&self.ys.1.to_bytes());
        let y_bits_ringvec = RingVec::new(
            vec![self.y_bits.0.val, self.y_bits.1.val],
            self.ys.0.modulus(),
        )
        .expect("Failed to create RingVec from y_bits");
        out.extend_from_slice(&y_bits_ringvec.to_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let mut seed = [0u8; AES_BLOCK_SIZE];
        seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;

        let bits_byte = bytes[offset];
        offset += 1;
        let seed_bits = (bits_byte & 0x1 != 0, bits_byte & 0x2 != 0);

        let (ys0, used0) = RingVec::from_bytes(&bytes[offset..], modulus)
            .expect("Failed to create RingVec from ys0");
        offset += used0;

        let (ys1, used1) = RingVec::from_bytes(&bytes[offset..], modulus)
            .expect("Failed to create RingVec from ys1");
        offset += used1;

        let (y_bits_ringvec, used_y_bits) = RingVec::from_bytes(&bytes[offset..], modulus)
            .expect("Failed to create RingVec from y_bits");
        offset += used_y_bits;
        assert_eq!(ys0.len(), N, "Unexpected ys0 length");
        assert_eq!(ys1.len(), N, "Unexpected ys1 length");
        assert_eq!(y_bits_ringvec.len(), 2, "Unexpected y_bits length");
        let y_bits = (
            Mod2k::new(y_bits_ringvec[0], modulus),
            Mod2k::new(y_bits_ringvec[1], modulus),
        );

        (
            RdcfCW {
                seed,
                seed_bit: seed_bits,
                ys: (ys0, ys1),
                y_bits,
            },
            offset,
        )
    }
}

#[derive(Clone, Debug)]
pub struct RdcfData<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: (bool, bool),
    pub ys: (RingVec, RingVec),
    pub y_bits: (Mod2k, Mod2k),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RdcfKey<const N: usize> {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<RdcfCW<N>>,
}

impl<const N: usize> RdcfKey<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.key_idx as u8);
        out.extend_from_slice(&self.root_seed);
        let num_words = self.cor_words.len() as u32;
        out.extend_from_slice(&num_words.to_le_bytes());
        for cw in &self.cor_words {
            out.extend_from_slice(&cw.to_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let key_idx = bytes[offset] != 0;
        offset += 1;
        let mut root_seed = [0u8; AES_BLOCK_SIZE];
        root_seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let num_words = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("Failed to read num_words"),
        ) as usize;
        offset += 4;
        let mut cor_words = Vec::with_capacity(num_words);
        for _ in 0..num_words {
            let (cw, used) = RdcfCW::from_bytes(&bytes[offset..], modulus);
            cor_words.push(cw);
            offset += used;
        }
        (
            RdcfKey {
                key_idx,
                root_seed,
                cor_words,
            },
            offset,
        )
    }
}

#[derive(Clone, Debug)]
pub struct RdcfEval<const N: usize> {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: bool,
    y: RingVec,
    pub y_bit: Mod2k,
}

impl<const N: usize> RdcfEval<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.level.to_le_bytes());
        out.extend_from_slice(&self.seed);
        out.push(self.bit as u8);
        out.extend_from_slice(&self.y.to_bytes());
        out.extend_from_slice(&self.y_bit.to_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let level = usize::from_le_bytes(
            bytes[offset..offset + std::mem::size_of::<usize>()]
                .try_into()
                .expect("Failed to read level"),
        );
        offset += std::mem::size_of::<usize>();
        let mut seed = [0u8; AES_BLOCK_SIZE];
        seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let bit = bytes[offset] != 0;
        offset += 1;
        let (y, used_y) = RingVec::from_bytes(&bytes[offset..], modulus)
            .expect("Failed to create RingVec from y");
        offset += used_y;
        assert_eq!(y.len(), N, "Unexpected y length");
        let (y_bit, used_y_bit) = Mod2k::from_bytes(&bytes[offset..], modulus);
        offset += used_y_bit;

        (
            RdcfEval {
                level,
                seed,
                bit,
                y,
                y_bit,
            },
            offset,
        )
    }

    pub(crate) fn y(&self) -> &RingVec {
        &self.y
    }
}

fn gen_layer_data<const N: usize>(key: [u8; AES_BLOCK_SIZE], modulus: u128) -> RdcfData<N> {
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = ((num_payload_bits + 7) / 8) as usize;
    let modulus_mask = modulus - 1;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = RdcfData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: (false, false),
            ys: (
                RingVec::zero_with_len(N, modulus).expect("Failed to create left ringvec"),
                RingVec::zero_with_len(N, modulus).expect("Failed to create right ringvec"),
            ),
            y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
        };

        let mut payload_rnd = vec![0u8; num_payload_bytes * (N + 1) * 2 + AES_BLOCK_SIZE * 2];
        s.fill_bytes(&mut payload_rnd);

        out.seeds.0.copy_from_slice(
            &payload_rnd
                [payload_rnd.len() - 2 * AES_BLOCK_SIZE..payload_rnd.len() - AES_BLOCK_SIZE],
        );
        out.seeds
            .1
            .copy_from_slice(&payload_rnd[payload_rnd.len() - AES_BLOCK_SIZE..]);

        out.bits.0 = (out.seeds.0[0] & 0x1) == 0;
        out.bits.1 = (out.seeds.1[0] & 0x1) == 0;
        out.seeds.0[0] &= 0xFE; // Zero out first bit
        out.seeds.1[0] &= 0xFE; // Zero out first bit

        for i in 0..N {
            out.ys.0[i] =
                bytes_to_u128(&payload_rnd[i * num_payload_bytes..(i + 1) * num_payload_bytes])
                    & modulus_mask;
        }
        out.y_bits.0 = Mod2k::new(
            bytes_to_u128(&payload_rnd[N * num_payload_bytes..(N + 1) * num_payload_bytes]),
            modulus,
        );

        for i in 0..N {
            out.ys.1[i] = bytes_to_u128(
                &payload_rnd[(i + N + 1) * num_payload_bytes..(i + N + 2) * num_payload_bytes],
            ) & modulus_mask;
        }
        out.y_bits.1 = Mod2k::new(
            bytes_to_u128(
                &payload_rnd[(2 * N + 1) * num_payload_bytes..(2 * N + 2) * num_payload_bytes],
            ),
            modulus,
        );
        out
    })
}

fn gen_cor_word<const N: usize>(
    alpha_bit: bool,
    left: RingVec,
    right: RingVec,
    modulus: u128,
    eval: &mut (RdcfEval<N>, RdcfEval<N>),
) -> RdcfCW<N> {
    let data = (
        gen_layer_data::<N>(eval.0.seed, modulus),
        gen_layer_data::<N>(eval.1.seed, modulus),
    );
    let delta_seed = (
        xor::<AES_BLOCK_SIZE>(&data.0.seeds.0, &data.1.seeds.0),
        xor::<AES_BLOCK_SIZE>(&data.0.seeds.1, &data.1.seeds.1),
    );
    let delta_bits = (data.0.bits.0 ^ data.1.bits.0, data.0.bits.1 ^ data.1.bits.1);
    let delta_ys = (data.1.ys.0 - data.0.ys.0, data.1.ys.1 - data.0.ys.1);
    let delta_y_bits = (
        data.1.y_bits.0 - data.0.y_bits.0,
        data.1.y_bits.1 - data.0.y_bits.1,
    );

    let mut cw = RdcfCW {
        seed: [0u8; 16],
        seed_bit: (false, false),
        ys: (
            RingVec::zero_with_len(N, modulus).expect("Failed to create left ringvec"),
            RingVec::zero_with_len(N, modulus).expect("Failed to create right ringvec"),
        ),
        y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
    };

    if !alpha_bit {
        cw.seed = delta_seed.1;
        cw.seed_bit = (delta_bits.0 ^ true, delta_bits.1 ^ false);
        cw.ys = (delta_ys.0 + left.clone(), delta_ys.1 + right);
        cw.y_bits = (
            delta_y_bits.0 + Mod2k::one(modulus),
            delta_y_bits.1 + Mod2k::zero(modulus),
        );
    } else {
        cw.seed = delta_seed.0;
        cw.seed_bit = (delta_bits.0 ^ false, delta_bits.1 ^ true);
        cw.ys = (delta_ys.0 + left.clone(), delta_ys.1 + left);
        cw.y_bits = (
            delta_y_bits.0 + Mod2k::zero(modulus),
            delta_y_bits.1 + Mod2k::one(modulus),
        );
    }

    let new_seed = if !alpha_bit {
        (
            xor::<AES_BLOCK_SIZE>(
                &data.0.seeds.0,
                &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.0.bit),
            ),
            xor::<AES_BLOCK_SIZE>(
                &data.1.seeds.0,
                &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.1.bit),
            ),
        )
    } else {
        (
            xor::<AES_BLOCK_SIZE>(
                &data.0.seeds.1,
                &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.0.bit),
            ),
            xor::<AES_BLOCK_SIZE>(
                &data.1.seeds.1,
                &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.1.bit),
            ),
        )
    };

    let new_bits = if !alpha_bit {
        (
            data.0.bits.0 ^ (cw.seed_bit.0 & eval.0.bit),
            data.1.bits.0 ^ (cw.seed_bit.0 & eval.1.bit),
        )
    } else {
        (
            data.0.bits.1 ^ (cw.seed_bit.1 & eval.0.bit),
            data.1.bits.1 ^ (cw.seed_bit.1 & eval.1.bit),
        )
    };

    *eval = (
        RdcfEval {
            level: 0,
            seed: new_seed.0,
            bit: new_bits.0,
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create left eval y"),
            y_bit: Mod2k::zero(modulus),
        },
        RdcfEval {
            level: 0,
            seed: new_seed.1,
            bit: new_bits.1,
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create right eval y"),
            y_bit: Mod2k::zero(modulus),
        },
    );

    cw
}

/// All-prefix DPF implementation.
impl<const N: usize> RdcfKey<N> {
    // Need alpha < beta
    pub fn gen_rdcf_key(
        alpha_bits: &[bool],
        a: &RingVec,
        b: &RingVec,
        modulus: u128,
    ) -> (RdcfKey<N>, RdcfKey<N>) {
        assert!(
            modulus > 0 && (modulus & (modulus - 1)) == 0,
            "Modulus must be a power of 2"
        );

        let u = alpha_bits.len();
        let zero = RingVec::zero_with_len(N, modulus).expect("Failed to create zero ringvec");
        let mut payload_left = vec![zero.clone(); u];
        payload_left[0] = a.clone();
        let mut payload_right = vec![b.clone() - a.clone(); u];
        payload_right[0] = b.clone();

        let root_seeds = (
            rand::rng().random::<[u8; 16]>(),
            rand::rng().random::<[u8; 16]>(),
        );

        let eval0 = RdcfEval {
            level: 0,
            seed: root_seeds.0,
            bit: true,
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create eval0 y"),
            y_bit: Mod2k::one(modulus),
        };

        let eval1 = RdcfEval {
            level: 0,
            seed: root_seeds.1,
            bit: false,
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create eval1 y"),
            y_bit: Mod2k::zero(modulus),
        };

        let mut eval = (eval0.clone(), eval1.clone());

        let mut cor_words: Vec<RdcfCW<N>> = Vec::new();

        for (i, &alpha_bit) in alpha_bits.iter().enumerate() {
            let cw = gen_cor_word(
                alpha_bit,
                payload_left[i].clone(),
                payload_right[i].clone(),
                modulus,
                &mut eval,
            );
            cor_words.push(cw);
        }

        (
            RdcfKey {
                key_idx: false,
                root_seed: root_seeds.0,
                cor_words: cor_words.clone(),
            },
            RdcfKey {
                key_idx: true,
                root_seed: root_seeds.1,
                cor_words,
            },
        )
    }

    pub fn eval_bit(&self, state: &RdcfEval<N>, modulus: u128, dir: bool) -> RdcfEval<N> {
        let data: RdcfData<N> = gen_layer_data(state.seed, modulus);
        let cw = self.cor_words[state.level].clone();

        let seed = if !dir {
            xor::<16>(&data.seeds.0, &and_bit::<16>(cw.seed, state.bit))
        } else {
            xor::<16>(&data.seeds.1, &and_bit::<16>(cw.seed, state.bit))
        };
        let new_bit = if !dir {
            data.bits.0 ^ (cw.seed_bit.0 & state.bit)
        } else {
            data.bits.1 ^ (cw.seed_bit.1 & state.bit)
        };
        let mut new_y = if !dir {
            data.ys.0 + (cw.ys.0 * state.y_bit.val)
        } else {
            data.ys.1 + (cw.ys.1 * state.y_bit.val)
        };
        let new_y_bit = if !dir {
            data.y_bits.0 + (cw.y_bits.0 * state.y_bit)
        } else {
            data.y_bits.1 + (cw.y_bits.1 * state.y_bit)
        };

        new_y = new_y + state.y.clone();

        RdcfEval {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        }
    }

    pub fn expand_prefix(&self, state: &RdcfEval<N>, modulus: u128) -> (RdcfEval<N>, RdcfEval<N>) {
        let data: RdcfData<N> = gen_layer_data(state.seed, modulus);
        let cw = self.cor_words[state.level].clone();

        let seeds = (
            xor::<16>(&data.seeds.0, &and_bit::<16>(cw.seed, state.bit)),
            xor::<16>(&data.seeds.1, &and_bit::<16>(cw.seed, state.bit)),
        );
        let new_bits = (
            data.bits.0 ^ (cw.seed_bit.0 & state.bit),
            data.bits.1 ^ (cw.seed_bit.1 & state.bit),
        );
        let mut new_ys = (
            data.ys.0 + (cw.ys.0 * state.y_bit.val),
            data.ys.1 + (cw.ys.1 * state.y_bit.val),
        );
        new_ys.0 = new_ys.0 + state.y.clone();
        new_ys.1 = new_ys.1 + state.y.clone();
        let new_y_bit = (
            data.y_bits.0 + (cw.y_bits.0 * state.y_bit),
            data.y_bits.1 + (cw.y_bits.1 * state.y_bit),
        );
        (
            RdcfEval {
                level: state.level + 1,
                seed: seeds.0,
                bit: new_bits.0,
                y: new_ys.0,
                y_bit: new_y_bit.0,
            },
            RdcfEval {
                level: state.level + 1,
                seed: seeds.1,
                bit: new_bits.1,
                y: new_ys.1,
                y_bit: new_y_bit.1,
            },
        )
    }

    pub fn eval_init(&self, modulus: u128) -> RdcfEval<N> {
        if !self.key_idx {
            RdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: true,
                y: RingVec::zero_with_len(N, modulus).expect("Failed to create init y"),
                y_bit: Mod2k::one(modulus),
            }
        } else {
            RdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: false,
                y: RingVec::zero_with_len(N, modulus).expect("Failed to create init y"),
                y_bit: Mod2k::zero(modulus),
            }
        }
    }

    pub fn eval_rdcf(&self, prefix: &[bool], modulus: u128) -> RingVec {
        debug_assert!(prefix.len() <= self.domain_size());
        debug_assert!(!prefix.is_empty());
        let mut state = self.eval_init(modulus);

        for i in 0..prefix.len() {
            let bit = prefix[i];
            let state_new = self.eval_bit(&state, modulus, bit);
            state = state_new;
        }

        state.y
    }

    pub fn domain_size(&self) -> usize {
        self.cor_words.len()
    }

    pub fn full_domain_eval(&self, modulus: u128, domain_size: usize) -> Vec<RingVec> {
        let mut states = vec![self.eval_init(modulus); 1 << domain_size];
        for level in 0..domain_size {
            for i in (0..(1 << level)).rev() {
                (states[i << 1], states[i << 1 | 1]) = self.expand_prefix(&states[i], modulus);
            }
        }
        let results = states.iter().map(|s| s.y.clone()).collect();
        states.clear();
        results
    }

    pub fn full_domain_incremental_eval(
        &self,
        modulus: u128,
        domain_size: usize,
    ) -> Vec<Vec<RingVec>> {
        let mut states = vec![self.eval_init(modulus); 1 << domain_size];
        let mut results = Vec::new();
        results.push(states[..1].iter().map(|x| x.y.clone()).collect());
        for level in 0..domain_size {
            for i in (0..(1 << level)).rev() {
                (states[i << 1], states[i << 1 | 1]) = self.expand_prefix(&states[i], modulus);
            }
            results.push(
                states[..(1 << (level + 1))]
                    .iter()
                    .map(|x| x.y.clone())
                    .collect(),
            );
        }
        states.clear();
        results
    }
}
