use core::num;
use std::cmp::{max, min};
use crate::data_structures::modint::ModInt;
use crate::{add_bitstrings, bits_to_u32, data_structures::prg, subtract_bitstrings, u32_to_bits, MSB_u32_to_bits, 
            xor, and_bit, bytes_to_u128};
use crate::Group;
use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use crate::data_structures::payload::RingVec;
use crate::data_structures::pair::Pair;

use serde::Deserialize;
use serde::Serialize;
use crate::sample_driving_data::i16_to_bitvec;

use rand_core::RngCore; 
use rand::Rng;
use std::cell::RefCell;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

#[derive(Clone, Debug, Copy)]
pub struct IntervalFSSCW<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], 
                [u8; AES_BLOCK_SIZE]),
    pub bits: ((Pair<bool>, Pair<bool>),
               (Pair<bool>, Pair<bool>)),
    pub ys: ((RingVec<N>, RingVec<N>), 
             (RingVec<N>, RingVec<N>)),
    pub y_bits: ((Pair<ModInt>, Pair<ModInt>),
                 (Pair<ModInt>, Pair<ModInt>)),
}

#[derive(Clone, Debug, Copy)]
pub struct IntervalFSSData<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: (Pair<bool>, Pair<bool>),
    pub ys: (RingVec<N>, RingVec<N>),
    pub y_bits: (Pair<ModInt>, Pair<ModInt>),
}

#[derive(Clone, Debug)]
pub struct IntervalFSSKey<const N: usize> {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<IntervalFSSCW<N>>,
}


#[derive(Clone, Debug, Copy)]
pub struct IntervalFSSEval<const N: usize> {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: Pair<bool>,
    pub y: RingVec<N>,
    pub y_bit: Pair<ModInt>,
}

fn gen_layer_data<const N: usize>(key: [u8; AES_BLOCK_SIZE], modulus: u128, left: bool, right: bool) -> IntervalFSSData<N> {
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = ((num_payload_bits + 7) / 8) as usize;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = IntervalFSSData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: (Pair::<bool>::new(false, false), Pair::<bool>::new(false, false)),
            ys: (RingVec::<N>::zero(modulus), RingVec::<N>::zero(modulus)),
            y_bits: (
                Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::zero(modulus)), 
                Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::zero(modulus))
            ),
        };

        s.refill();
        s.fill_bytes(&mut out.seeds.0);
        s.refill();
        s.fill_bytes(&mut out.seeds.1);

        out.bits.0 = Pair::new((out.seeds.0[0] & 0x1) == 0, out.seeds.0[0] & 0x2 == 0);
        out.bits.1 = Pair::new((out.seeds.1[0] & 0x1) == 0, out.seeds.1[0] & 0x2 == 0);
        out.seeds.0[0] &= 0xFC; // Zero out first two bits
        out.seeds.1[0] &= 0xFC; // Zero out first two bits

        let mut payload_rnd = vec![0u8; num_payload_bytes * (N + 2) * 2];
        s.refill();
        s.fill_bytes(&mut payload_rnd);

        for i in 0..N {
            out.ys.0[i] = bytes_to_u128(&payload_rnd[i * num_payload_bytes..(i + 1) * num_payload_bytes]) % modulus;
        }
        out.y_bits.0.first = ModInt::new(bytes_to_u128(&payload_rnd[N * num_payload_bytes..(N + 1) * num_payload_bytes]), modulus);
        out.y_bits.0.second = ModInt::new(bytes_to_u128(&payload_rnd[(N + 1) * num_payload_bytes..(N + 2) * num_payload_bytes]), modulus);

        for i in 0..N {
            out.ys.1[i] = bytes_to_u128(&payload_rnd[(i + N + 2) * num_payload_bytes..(i + N + 3) * num_payload_bytes]) % modulus;
        }
        out.y_bits.1.first = ModInt::new(bytes_to_u128(&payload_rnd[(2 * N + 2) * num_payload_bytes..(2 * N + 3) * num_payload_bytes]), modulus);
        out.y_bits.1.second = ModInt::new(bytes_to_u128(&payload_rnd[(2 * N + 3) * num_payload_bytes..(2 * N + 4) * num_payload_bytes]), modulus);

        out
    })
}

fn gen_cor_word<const N: usize>(
    alpha_bit: bool,
    beta_bit: bool,
    left: RingVec<N>,
    mid: RingVec<N>,
    right: RingVec<N>,
    modulus: u128,
    eval: &mut Vec<(IntervalFSSEval<N>, IntervalFSSEval<N>)>,
) -> IntervalFSSCW<N>
{
    let modulus_mask = modulus - 1;
    let mut data = vec![];
    eval.iter().for_each(|(eval0, eval1)| {
        data.push((
            gen_layer_data(eval0.seed, modulus, true, true),
            gen_layer_data(eval1.seed, modulus, true, true)
        ));
    });

    println!("Data 0: {:?}", data[0].0);
    println!("Data 1: {:?}", data[0].1);

    let mut delta_seed = Vec::<([u8; 16], [u8; 16])>::new();
    let mut delta_bits = vec![];
    let mut delta_ys = vec![];
    let mut delta_y_bits = vec![];

    data.iter().for_each(|(d0, d1)| {
        delta_seed.push((
            xor::<16>(&d1.seeds.0, &d0.seeds.0),
            xor::<16>(&d1.seeds.1, &d0.seeds.1)
        ));
        delta_bits.push((
            d1.bits.0 ^ d0.bits.0,
            d1.bits.1 ^ d0.bits.1
        ));
        delta_ys.push((
            d1.ys.0 - d0.ys.0, 
            d1.ys.1 - d0.ys.1
        ));
        delta_y_bits.push((
            d1.y_bits.0 - d0.y_bits.0,
            d1.y_bits.1 - d0.y_bits.1
        ));
    });

    println!("Delta bits: {:?}", delta_bits);
    println!("Delta ys: {:?}", delta_ys);
    println!("Delta y bits: {:?}", delta_y_bits);


    if data.len() > 2 {
        panic!("Something went wrong, data length is greater than 2: {}", data.len());
    }

    let seed1 = rand::rng().random::<[u8; 16]>();
    let bits1 = (
        Pair::<bool>::new(rand::rng().random::<bool>(), rand::rng().random::<bool>()), 
        Pair::<bool>::new(rand::rng().random::<bool>(), rand::rng().random::<bool>()));
    let ys1 = (RingVec::<N>::random(modulus), RingVec::<N>::random(modulus));
    let y_bits1 = (
        Pair::<ModInt>::new(ModInt::random(modulus), ModInt::random(modulus)), 
        Pair::<ModInt>::new(ModInt::random(modulus), ModInt::random(modulus)));
    let mut cw = IntervalFSSCW {
        seeds: ([0u8; 16], seed1),
        bits: ((Pair::<bool>::new(false, false), Pair::<bool>::new(false, false)), bits1),
        ys: ((RingVec::<N>::zero(modulus), RingVec::<N>::zero(modulus)), ys1),
        y_bits: (
            (Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::zero(modulus)), 
             Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::zero(modulus))), 
            y_bits1),
    };

    let bool10 = Pair::<bool>::new(true, false);
    let bool01 = Pair::<bool>::new(false, true);
    let bool00 = Pair::<bool>::new(false, false);

    let mint10 = Pair::<ModInt>::new(ModInt::one(modulus), ModInt::zero(modulus));
    let mint01 = Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::one(modulus));
    let mint00 = Pair::<ModInt>::new(ModInt::zero(modulus), ModInt::zero(modulus));

    if data.len() == 1 {
        if alpha_bit && !beta_bit {
            panic!("Alpha should be smaller than beta, but got alpha_bit = true and beta_bit = false");
        }
        if !alpha_bit && beta_bit {
            cw.seeds.0 = rand::rng().random::<[u8; 16]>();
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool01 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0 + mid, delta_ys[0].1 + mid);
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint01);
        } else if !alpha_bit {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool00 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0 + mid, delta_ys[0].1 + right);
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint00);
        } else if alpha_bit {
            cw.seeds.0 = delta_seed[0].0;
            cw.bits.0 = (bool00 ^ delta_bits[0].0, bool10 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0 + left, delta_ys[0].1 + mid);
            cw.y_bits.0 = (delta_y_bits[0].0 + mint00, 
                           delta_y_bits[0].1 + mint10);
        }
    } else {
        if !alpha_bit {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool00 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0 + mid, delta_ys[0].1 + mid);
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint00);
        } else {
            cw.seeds.0 = delta_seed[0].0;
            cw.bits.0 = (bool00 ^ delta_bits[0].0, bool10 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0 + left, delta_ys[0].1 + mid);
            cw.y_bits.0 = (delta_y_bits[0].0 + mint00, 
                           delta_y_bits[0].1 + mint10);
        }

        if !beta_bit {
            cw.seeds.1 = delta_seed[1].1;
            cw.bits.1 = (bool01 ^ delta_bits[1].0, bool00 ^ delta_bits[1].1);
            cw.ys.1 = (delta_ys[1].0 + mid, delta_ys[1].1 + right);
            cw.y_bits.1 = (delta_y_bits[1].0 + mint01, 
                           delta_y_bits[1].1 + mint00);
        } else {
            cw.seeds.1 = delta_seed[1].0;
            cw.bits.1 = (bool00 ^ delta_bits[1].0, bool01 ^ delta_bits[1].1);
            cw.ys.1 = (delta_ys[1].0 + mid, delta_ys[1].1 + mid);
            cw.y_bits.1 = (delta_y_bits[1].0 + mint00, 
                           delta_y_bits[1].1 + mint01);
        }
    }

    let mut new_seeds = Vec::<([u8; 16], [u8; 16])>::new();
    let mut new_bits = vec![];
    let mut new_y_bits = vec![];

    if data.len() == 1 {
        let d0 = data[0].0;
        let d1 = data[0].1;
        let eval0 = eval[0].0;
        let eval1 = eval[0].1;
        if !alpha_bit {
            println!("Data length is 1, alpha_bit is false");
            new_seeds.push((
                xor::<16>(&d0.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.0 ^ (cw.bits.0.0 & eval0.bit.first) ^ (cw.bits.1.0 & eval0.bit.second),
                           d1.bits.0 ^ (cw.bits.0.0 & eval1.bit.first) ^ (cw.bits.1.0 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.0 + (cw.y_bits.0.0 * eval0.y_bit.first) + (cw.y_bits.1.0 * eval0.y_bit.second),
                             d1.y_bits.0 + (cw.y_bits.0.0 * eval1.y_bit.first) + (cw.y_bits.1.0 * eval1.y_bit.second)));
        }
        if beta_bit {
            println!("Data length is 1, beta_bit is true");
            new_seeds.push((
                xor::<16>(&d0.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.1 ^ (cw.bits.0.1 & eval0.bit.first) ^ (cw.bits.1.1 & eval0.bit.second),
                           d1.bits.1 ^ (cw.bits.0.1 & eval1.bit.first) ^ (cw.bits.1.1 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.1 + (cw.y_bits.0.1 * eval0.y_bit.first) + (cw.y_bits.1.1 * eval0.y_bit.second),
                             d1.y_bits.1 + (cw.y_bits.0.1 * eval1.y_bit.first) + (cw.y_bits.1.1 * eval1.y_bit.second)));
        }    
    } else {
        println!("Data length is 2, alpha_bit = {}, beta_bit = {}", alpha_bit, beta_bit);
        let d0 = data[0].0;
        let d1 = data[0].1;
        let eval0 = eval[0].0;
        let eval1 = eval[0].1;
        if !alpha_bit {
            new_seeds.push((
                xor::<16>(&d0.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.0 ^ (cw.bits.0.0 & eval0.bit.first) ^ (cw.bits.1.0 & eval0.bit.second),
                           d1.bits.0 ^ (cw.bits.0.0 & eval1.bit.first) ^ (cw.bits.1.0 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.0 + (cw.y_bits.0.0 * eval0.y_bit.first) + (cw.y_bits.1.0 * eval0.y_bit.second),
                             d1.y_bits.0 + (cw.y_bits.0.0 * eval1.y_bit.first) + (cw.y_bits.1.0 * eval1.y_bit.second)));
        } else {
            new_seeds.push((
                xor::<16>(&d0.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.1 ^ (cw.bits.0.1 & eval0.bit.first) ^ (cw.bits.1.1 & eval0.bit.second),
                           d1.bits.1 ^ (cw.bits.0.1 & eval1.bit.first) ^ (cw.bits.1.1 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.1 + (cw.y_bits.0.1 * eval0.y_bit.first) + (cw.y_bits.1.1 * eval0.y_bit.second),
                             d1.y_bits.1 + (cw.y_bits.0.1 * eval1.y_bit.first) + (cw.y_bits.1.1 * eval1.y_bit.second)));
        }

        let d0 = data[1].0;
        let d1 = data[1].1;
        let eval0 = eval[1].0;
        let eval1 = eval[1].1;

        if !beta_bit {
            new_seeds.push((
                xor::<16>(&d0.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.0, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.0 ^ (cw.bits.0.0 & eval0.bit.first) ^ (cw.bits.1.0 & eval0.bit.second),
                           d1.bits.0 ^ (cw.bits.0.0 & eval1.bit.first) ^ (cw.bits.1.0 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.0 + (cw.y_bits.0.0 * eval0.y_bit.first) + (cw.y_bits.1.0 * eval0.y_bit.second),
                             d1.y_bits.0 + (cw.y_bits.0.0 * eval1.y_bit.first) + (cw.y_bits.1.0 * eval1.y_bit.second)));
        } else {
            new_seeds.push((
                xor::<16>(&d0.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval0.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval0.bit.second))),
                xor::<16>(&d1.seeds.1, 
                    &xor::<16>(
                        &and_bit::<16>(cw.seeds.0, eval1.bit.first),
                        &and_bit::<16>(cw.seeds.1, eval1.bit.second)))
            ));
            new_bits.push((d0.bits.1 ^ (cw.bits.0.1 & eval0.bit.first) ^ (cw.bits.1.1 & eval0.bit.second),
                           d1.bits.1 ^ (cw.bits.0.1 & eval1.bit.first) ^ (cw.bits.1.1 & eval1.bit.second)));
            new_y_bits.push((d0.y_bits.1 + (cw.y_bits.0.1 * eval0.y_bit.first) + (cw.y_bits.1.1 * eval0.y_bit.second),
                             d1.y_bits.1 + (cw.y_bits.0.1 * eval1.y_bit.first) + (cw.y_bits.1.1 * eval1.y_bit.second)));
        }
    }

    println!("After updating, new_seeds length: {}", new_seeds.len());

    let new_eval: Vec<(IntervalFSSEval<N>, IntervalFSSEval<N>)> = new_seeds.iter().zip(new_bits.iter()).zip(new_y_bits.iter())
        .map(|((seed, bits), y_bits)| {
            (
                IntervalFSSEval {
                    level: 0,
                    seed: seed.0,
                    bit: bits.0,
                    y: RingVec::<N>::zero(modulus),
                    y_bit: y_bits.0.clone(),
                },
                IntervalFSSEval {
                    level: 0,
                    seed: seed.1,
                    bit: bits.1,
                    y: RingVec::<N>::zero(modulus),
                    y_bit: y_bits.1.clone(),
                }
            )
        }).collect();
    
    eval.clear();
    eval.extend(new_eval);

    cw
}

/// All-prefix DPF implementation.
impl<const N: usize> IntervalFSSKey<N>
{

    // Need alpha < beta
    pub fn gen_IntervalFSSKey(alpha_bits: &[bool], beta_bits: &[bool], a: RingVec<N>, b: RingVec<N>, c: RingVec<N>, modulus: u128) -> (IntervalFSSKey<N>, IntervalFSSKey<N>) {
        assert!(alpha_bits.len() == beta_bits.len());
        assert!(modulus > 0 && (modulus & (modulus-1)) == 0, "Modulus must be a power of 2");

        let u = alpha_bits.len();
        let modulus_mask = modulus - 1;
        let mut payload_left = vec![a.clone() - b.clone(); u];
        payload_left[0] = a.clone();
        let mut payload_mid = vec![RingVec::<N>::zero(modulus); u];
        payload_mid[0] = b.clone();
        let mut payload_right = vec![c.clone() - b.clone(); u];
        payload_right[0] = c.clone();   

        let root_seeds = (rand::rng().random::<[u8; 16]>(), rand::rng().random::<[u8; 16]>());

        let eval0 = IntervalFSSEval {
            level: 0,
            seed: root_seeds.0,
            bit: Pair::new(true, false),
            y: RingVec::<N>::zero(modulus),
            y_bit: Pair::new(ModInt::one(modulus), ModInt::zero(modulus)),
        };

        let eval1 = IntervalFSSEval {
            level: 0,
            seed: root_seeds.1,
            bit: Pair::new(false, false),
            y: RingVec::<N>::zero(modulus),
            y_bit: Pair::new(ModInt::zero(modulus), ModInt::zero(modulus)),
        };

        let mut eval = vec![(eval0.clone(), eval1.clone())];

        let mut cor_words: Vec<IntervalFSSCW<N>> = Vec::new();

        for (i, (&alpha_bit, &beta_bit)) in alpha_bits.iter().zip(beta_bits.iter()).enumerate() {
            println!("Layer {}: alpha_bit = {}, beta_bit = {}", i, alpha_bit, beta_bit);
            println!("In layer {}, the length of eval is {}", i, eval.len());
            let cw = gen_cor_word(
                alpha_bit, 
                beta_bit, 
                payload_left[i],
                payload_mid[i], 
                payload_right[i], 
                modulus,
                &mut eval
            );
            cor_words.push(cw);
        }

        (
            IntervalFSSKey {
                key_idx: false,
                root_seed: root_seeds.0,
                cor_words: cor_words.clone(),
            },
            IntervalFSSKey {
                key_idx: true,
                root_seed: root_seeds.1,
                cor_words,
            },
        )
    }

    pub fn eval_bit(&self, state: &IntervalFSSEval<N>, modulus: u128, dir: bool) -> IntervalFSSEval<N> {
        let modulus_mask = modulus - 1;
        let data = gen_layer_data(state.seed, modulus, dir, !dir);
        let mut seed = if !dir {
            data.seeds.0.clone()
        } else {
            data.seeds.1.clone()
        };
        let mut new_bit = if !dir {
            data.bits.0.clone()
        } else {
            data.bits.1.clone()
        };
        let mut new_y = if !dir {
            data.ys.0.clone()
        } else {
            data.ys.1.clone()
        };
        let mut new_y_bit = if !dir {
            data.y_bits.0.clone()
        } else {
            data.y_bits.1.clone()
        };

        println!("Current state: {:?}", state);
        println!("New y: {:?}", new_y);
        println!("New y_bit: {:?}", new_y_bit);

        let cw = self.cor_words[state.level];

        println!("First part ys of the cor word: {:?}", cw.ys.0);
        println!("Second part ys of the cor word: {:?}", cw.ys.1);

        seed = xor::<16>(&seed,
                        &xor::<16>(&and_bit::<16>(cw.seeds.0, state.bit.first),
                                    &and_bit::<16>(cw.seeds.1, state.bit.second)));
        new_bit = if !dir {
            new_bit ^ (cw.bits.0.0 & state.bit.first) ^ (cw.bits.1.0 & state.bit.second)
        } else {
            new_bit ^ (cw.bits.0.1 & state.bit.first) ^ (cw.bits.1.1 & state.bit.second)
        };
        new_y = if !dir {
            new_y + (cw.ys.0.0 * state.y_bit.first.val) + (cw.ys.1.0 * state.y_bit.second.val)
        } else {
            new_y + (cw.ys.0.1 * state.y_bit.first.val) + (cw.ys.1.1 * state.y_bit.second.val)
        };
        new_y_bit = if !dir {
            new_y_bit + (cw.y_bits.0.0 * state.y_bit.first) + (cw.y_bits.1.0 * state.y_bit.second)
        } else {
            new_y_bit + (cw.y_bits.0.1 * state.y_bit.first) + (cw.y_bits.1.1 * state.y_bit.second)
        };

        new_y = new_y + state.y;

        IntervalFSSEval {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        }
    }

    pub fn eval_init(&self, modulus: u128) -> IntervalFSSEval<N> {
        let y_bit_first = if self.key_idx {
            ModInt::zero(modulus)
        } else {
            ModInt::one(modulus)
        };
        IntervalFSSEval {
            level: 0,
            seed: self.root_seed.clone(),
            bit: Pair::new(!self.key_idx, false),
            y: RingVec::<N>::zero(modulus),
            y_bit: Pair::new(y_bit_first, ModInt::zero(modulus)),
        }
    }

    pub fn eval_intervalFSS(&self, idx: &[bool], modulus: u128) -> RingVec<N> {
        debug_assert!(idx.len() <= self.domain_size());
        debug_assert!(!idx.is_empty());
        let mut state = self.eval_init(modulus);

        for i in 0..idx.len() {
            let bit = idx[i];
            let state_new = self.eval_bit(&state, modulus, bit);
            state = state_new;
        }

        state.y
    }

    pub fn domain_size(&self) -> usize {
        self.cor_words.len()
    }
}