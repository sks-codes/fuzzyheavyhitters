use crate::data_structures::mod2k::Mod2k;
use crate::util::{xor, and_bit, xor_u8_16};
use crate::bytes_to_u128;
use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use crate::data_structures::ringvec::RingVec;

use rand_core::RngCore; 
use rand::Rng;
use std::cell::RefCell;
use std::convert::TryInto;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

#[derive(Clone, Debug, Copy, PartialEq)]
pub struct LdcfCW<const N: usize> {
    pub seed: [u8; AES_BLOCK_SIZE], 
    pub seed_bit: (bool, bool),
    pub ys: (RingVec<N>, RingVec<N>), 
    pub y_bits: (Mod2k, Mod2k),
}

impl<const N: usize> LdcfCW<N> {
    pub fn bytes_size(modulus: u128) -> usize {
        let mut size = 0;
        size += AES_BLOCK_SIZE;
        size += 1; // for seed_bit
        size += 2 * RingVec::<N>::byte_size_for_modulus(modulus);
        size += RingVec::<2>::byte_size_for_modulus(modulus);
        size
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::bytes_size(self.ys.0.modulus()));
        out.extend_from_slice(&self.seed);
        let mut bits_byte = 0u8;
        bits_byte |= (self.seed_bit.0 as u8) << 0;
        bits_byte |= (self.seed_bit.1 as u8) << 1;
        out.push(bits_byte);
        out.extend_from_slice(&self.ys.0.to_bytes());
        out.extend_from_slice(&self.ys.1.to_bytes());
        let y_bits_ringvec = RingVec::<2>::from_vec(vec![self.y_bits.0.val, self.y_bits.1.val], self.ys.0.modulus()).expect("Failed to create RingVec from y_bits");
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

        let (ys0, used0) = RingVec::<N>::from_bytes(&bytes[offset..], modulus).expect("Failed to create RingVec from ys0");
        offset += used0;

        let (ys1, used1) = RingVec::<N>::from_bytes(&bytes[offset..], modulus).expect("Failed to create RingVec from ys1");
        offset += used1;

        let (y_bits_ringvec, used_y_bits) = RingVec::<2>::from_bytes(&bytes[offset..], modulus).expect("Failed to create RingVec from y_bits");
        offset += used_y_bits;
        let y_bits = (Mod2k::new(y_bits_ringvec[0], modulus), Mod2k::new(y_bits_ringvec[1], modulus));

        (
            LdcfCW {
            seed,
            seed_bit: seed_bits,
            ys: (ys0, ys1),
            y_bits,
            },
            offset
        )
    }
}

#[derive(Clone, Debug, Copy)]
pub struct LdcfData<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: (bool, bool),
    pub ys: (RingVec<N>, RingVec<N>),
    pub y_bits: (Mod2k, Mod2k),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LdcfKey<const N: usize> {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<LdcfCW<N>>,
}

impl<const N: usize> LdcfKey<N> {
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
        let num_words = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("Failed to read num_words")) as usize;
        offset += 4;
        let mut cor_words = Vec::with_capacity(num_words);
        for _ in 0..num_words {
            let (cw, used) = LdcfCW::from_bytes(&bytes[offset..], modulus);
            cor_words.push(cw);
            offset += used;
        }
        (
            LdcfKey {
                key_idx,
                root_seed,
                cor_words,
            },
            offset
        )
    }
}


#[derive(Clone, Debug, Copy)]
pub struct LdcfEval<const N: usize> {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: bool,
    y: RingVec<N>,
    pub y_bit: Mod2k,
}

impl<const N: usize> LdcfEval<N> {
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
        let level = usize::from_le_bytes(bytes[offset..offset + std::mem::size_of::<usize>()].try_into().expect("Failed to read level"));
        offset += std::mem::size_of::<usize>();
        let mut seed = [0u8; AES_BLOCK_SIZE];
        seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let bit = bytes[offset] != 0;
        offset += 1;
        let (y, used_y) = RingVec::<N>::from_bytes(&bytes[offset..], modulus).expect("Failed to create RingVec from y");
        offset += used_y;
        let (y_bit, used_y_bit) = Mod2k::from_bytes(&bytes[offset..], modulus);
        offset += used_y_bit;

        (
            LdcfEval {
                level,
                seed,
                bit,
                y,
                y_bit,
            },
            offset
        )
    }

    pub(crate) fn y(&self) -> &RingVec<N> {
        &self.y
    }
}

fn gen_layer_data<const N: usize>(key: [u8; AES_BLOCK_SIZE], modulus: u128) -> LdcfData<N> {
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = ((num_payload_bits + 7) / 8) as usize;
    let modulus_mask  = modulus - 1;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = LdcfData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: (false, false),
            ys: (RingVec::<N>::zero(modulus), RingVec::<N>::zero(modulus)),
            y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
        };

        let mut payload_rnd = vec![0u8; num_payload_bytes * (N + 1) * 2 + AES_BLOCK_SIZE * 2];
        s.fill_bytes(&mut payload_rnd);

        out.seeds.0.copy_from_slice(&payload_rnd[payload_rnd.len() - 2 * AES_BLOCK_SIZE..payload_rnd.len() - AES_BLOCK_SIZE]);
        out.seeds.1.copy_from_slice(&payload_rnd[payload_rnd.len() - AES_BLOCK_SIZE..]);

        out.bits.0 = (out.seeds.0[0] & 0x1) == 0;
        out.bits.1 = (out.seeds.1[0] & 0x1) == 0;
        out.seeds.0[0] &= 0xFE; // Zero out first bit
        out.seeds.1[0] &= 0xFE; // Zero out first bit

        for i in 0..N {
            out.ys.0[i] = bytes_to_u128(&payload_rnd[i * num_payload_bytes..(i + 1) * num_payload_bytes]) & modulus_mask;
        }
        out.y_bits.0 = Mod2k::new(bytes_to_u128(&payload_rnd[N * num_payload_bytes..(N + 1) * num_payload_bytes]), modulus);

        for i in 0..N {
            out.ys.1[i] = bytes_to_u128(&payload_rnd[(i + N + 1) * num_payload_bytes..(i + N + 2) * num_payload_bytes]) & modulus_mask;
        }
        out.y_bits.1 = Mod2k::new(bytes_to_u128(&payload_rnd[(2 * N + 1) * num_payload_bytes..(2 * N + 2) * num_payload_bytes]), modulus);
        out
    })
}

fn gen_cor_word<const N: usize>(
    alpha_bit: bool,
    left: RingVec<N>,
    right: RingVec<N>,
    modulus: u128,
    eval: &mut (LdcfEval<N>, LdcfEval<N>),
) -> LdcfCW<N>
{
    let data = (
            gen_layer_data(eval.0.seed, modulus),
            gen_layer_data(eval.1.seed, modulus)
        );
    let delta_seed = (xor::<AES_BLOCK_SIZE>(&data.0.seeds.0, &data.1.seeds.0), xor::<AES_BLOCK_SIZE>(&data.0.seeds.1, &data.1.seeds.1));
    let delta_bits = (data.0.bits.0 ^ data.1.bits.0, data.0.bits.1 ^ data.1.bits.1);
    let delta_ys = (data.1.ys.0 - data.0.ys.0, data.1.ys.1 - data.0.ys.1);
    let delta_y_bits = (data.1.y_bits.0 - data.0.y_bits.0, data.1.y_bits.1 - data.0.y_bits.1);

    let mut cw = LdcfCW {
        seed: [0u8; 16],
        seed_bit: (false, false),
        ys: (RingVec::<N>::zero(modulus), RingVec::<N>::zero(modulus)),
        y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
    };

    if !alpha_bit {
        cw.seed = delta_seed.1;
        cw.seed_bit = (delta_bits.0 ^ true, delta_bits.1 ^ false);
        cw.ys = (delta_ys.0 + right, delta_ys.1 + right);
        cw.y_bits = (delta_y_bits.0 + Mod2k::one(modulus), delta_y_bits.1 + Mod2k::zero(modulus));
    } else {
        cw.seed = delta_seed.0;
        cw.seed_bit = (delta_bits.0 ^ false, delta_bits.1 ^ true);
        cw.ys = (delta_ys.0 + left, delta_ys.1 + right);
        cw.y_bits = (delta_y_bits.0 + Mod2k::zero(modulus), delta_y_bits.1 + Mod2k::one(modulus));
    }

    let new_seed = if !alpha_bit {
        (xor::<AES_BLOCK_SIZE>(&data.0.seeds.0, &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.0.bit)),
        xor::<AES_BLOCK_SIZE>(&data.1.seeds.0, &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.1.bit)))
    } else {
        (xor::<AES_BLOCK_SIZE>(&data.0.seeds.1, &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.0.bit)),
        xor::<AES_BLOCK_SIZE>(&data.1.seeds.1, &and_bit::<AES_BLOCK_SIZE>(cw.seed, eval.1.bit)))
    };

    let new_bits = if !alpha_bit {
        (data.0.bits.0 ^ (cw.seed_bit.0 & eval.0.bit), data.1.bits.0 ^ (cw.seed_bit.0 & eval.1.bit))
    } else {
        (data.0.bits.1 ^ (cw.seed_bit.1 & eval.0.bit), data.1.bits.1 ^ (cw.seed_bit.1 & eval.1.bit))
    };

    *eval = (
                LdcfEval {
                    level: 0,
                    seed: new_seed.0,
                    bit: new_bits.0,
                    y: RingVec::<N>::zero(modulus),
                    y_bit: Mod2k::zero(modulus),
                },
                LdcfEval {
                    level: 0,
                    seed: new_seed.1,
                    bit: new_bits.1,
                    y: RingVec::<N>::zero(modulus),
                    y_bit: Mod2k::zero(modulus),
                }
            );

    cw
}

/// All-prefix DPF implementation.
impl<const N: usize> LdcfKey<N>
{

    // Need alpha < beta
    pub fn gen_ldcf_key(alpha_bits: &[bool], a: &RingVec<N>, b: &RingVec<N>, modulus: u128) -> (LdcfKey<N>, LdcfKey<N>) {
        assert!(modulus > 0 && (modulus & (modulus-1)) == 0, "Modulus must be a power of 2");

        let u = alpha_bits.len();
        let mut payload_left = vec![a.clone() - b.clone(); u];
        payload_left[0] = a.clone();
        let mut payload_right = vec![RingVec::<N>::zero(modulus); u];
        payload_right[0] = b.clone();

        let root_seeds = (rand::rng().random::<[u8; 16]>(), rand::rng().random::<[u8; 16]>());

        let eval0 = LdcfEval {
            level: 0,
            seed: root_seeds.0,
            bit: true,
            y: RingVec::<N>::zero(modulus),
            y_bit: Mod2k::one(modulus),
        };

        let eval1 = LdcfEval{
            level: 0,
            seed: root_seeds.1,
            bit: false,
            y: RingVec::<N>::zero(modulus),
            y_bit: Mod2k::zero(modulus),
        };

        let mut eval = (eval0.clone(), eval1.clone());

        let mut cor_words: Vec<LdcfCW<N>> = Vec::new();

        for (i, &alpha_bit) in alpha_bits.iter().enumerate() {
            let cw = gen_cor_word(
                alpha_bit, 
                payload_left[i],
                payload_right[i], 
                modulus,
                &mut eval
            );
            cor_words.push(cw);
        }

        (
            LdcfKey {
                key_idx: false,
                root_seed: root_seeds.0,
                cor_words: cor_words.clone(),
            },
            LdcfKey {
                key_idx: true,
                root_seed: root_seeds.1,
                cor_words,
            },
        )
    }

    pub fn eval_bit(&self, state: &LdcfEval<N>, modulus: u128, dir: bool) -> LdcfEval<N> {
        let data = gen_layer_data(state.seed, modulus);
        let cw = self.cor_words[state.level];

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

        new_y = new_y + state.y;

        LdcfEval {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        }
    }

    pub fn expand_prefix(&self, state: &LdcfEval<N>, modulus: u128) -> (LdcfEval<N>, LdcfEval<N>) {
        let data = gen_layer_data(state.seed, modulus);
        let cw = self.cor_words[state.level];

        let seeds = (
            xor_u8_16(&data.seeds.0, &and_bit::<16>(cw.seed, state.bit)),
            xor_u8_16(&data.seeds.1, &and_bit::<16>(cw.seed, state.bit))
        );

        let new_bits = (
            data.bits.0 ^ (cw.seed_bit.0 & state.bit),
            data.bits.1 ^ (cw.seed_bit.1 & state.bit)
        );

        let mut new_ys = (
            data.ys.0 + (cw.ys.0 * state.y_bit.val),
            data.ys.1 + (cw.ys.1 * state.y_bit.val)
        );
        new_ys.0 = new_ys.0 + state.y;
        new_ys.1 = new_ys.1 + state.y;

        let new_y_bit = (
            data.y_bits.0 + (cw.y_bits.0 * state.y_bit),
            data.y_bits.1 + (cw.y_bits.1 * state.y_bit)
        );

        (
            LdcfEval {
                level: state.level + 1,
                seed: seeds.0,
                bit: new_bits.0,
                y: new_ys.0,
                y_bit: new_y_bit.0,
            },
            LdcfEval {
                level: state.level + 1,
                seed: seeds.1,
                bit: new_bits.1,
                y: new_ys.1,
                y_bit: new_y_bit.1,
            },
        )
    }

    pub fn eval_init(&self, modulus: u128) -> LdcfEval<N> {
        if !self.key_idx {
            LdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: true,
                y: RingVec::<N>::zero(modulus),
                y_bit: Mod2k::one(modulus),
            }
        } else {
            LdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: false,
                y: RingVec::<N>::zero(modulus),
                y_bit: Mod2k::zero(modulus),
            }
        }
    }

    pub fn eval_ldcf(&self, prefix: &[bool], modulus: u128) -> RingVec<N> {
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

    // Returns evaluations, such that k0.eval() - k1.eval() = f(x) for all x in domain
    pub fn full_domain_eval(
        &self,
        modulus: u128,
        domain_size: usize,
    ) -> Vec<RingVec<N>> {
        let mut states = vec![self.eval_init(modulus); 1 << domain_size];
        for level in 0..domain_size {
            for i in (0..(1 << level)).rev() {
                (states[i << 1], states[i << 1 | 1]) = self.expand_prefix(&states[i], modulus);
            }
        }
        let results = states.iter().map(|s| s.y).collect();
        states.clear();
        results
    }
}