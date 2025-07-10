use std::cmp::{max, min};
use crate::{add_bitstrings, bits_to_u32, prg, subtract_bitstrings, u32_to_bits, MSB_u32_to_bits, 
            xor, and_bit};
use crate::Group;

use serde::Deserialize;
use serde::Serialize;
use crate::sample_driving_data::i16_to_bitvec;
use rand::{rng};

use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use std::cell::RefCell;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntervalFSSCW<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], 
                [u8; AES_BLOCK_SIZE]),
    pub bits: (((bool, bool), (bool, bool)),
               ((bool, bool), (bool, bool))),
    pub ys: (([u128; N], [u128; N]), 
             ([u128; N], [u128; N])),
    pub y_bits: (((u128, u128), (u128, u128)),
                 ((u128, u128), (u128, u128))),
}

pub struct IntervalFSSData<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: ((bool, bool), (bool, bool)),
    pub ys: ([u128; N], [u128; N]),
    pub y_bits: ((u128, u128), (u128, u128)),
}

#[derive(Clone, Debug)]
pub struct IntervalFSSKey {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<IntervalFSSCW>,
}


#[derive(Clone)]
pub struct EvalState {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: bool,
    pub y: u128,
    pub y_bit: u128,
}

trait TupleMapToExt<T, U> {
    type Output;
    fn map<F: FnMut(&T) -> U>(&self, f: F) -> Self::Output;
}

type TupleMutIter<'a, T> =
std::iter::Chain<std::iter::Once<(bool, &'a mut T)>, std::iter::Once<(bool, &'a mut T)>>;

trait TupleExt<T> {
   fn map_mut<F: Fn(&mut T)>(&mut self, f: F);
    fn get(&self, val: bool) -> &T;
    fn get_mut(&mut self, val: bool) -> &mut T;
    fn iter_mut(&mut self) -> TupleMutIter<T>;
}

impl<T, U> TupleMapToExt<T, U> for (T, T) {
    type Output = (U, U);

    #[inline(always)]
    fn map<F: FnMut(&T) -> U>(&self, mut f: F) -> Self::Output {
        (f(&self.0), f(&self.1))
    }
}

impl<T> TupleExt<T> for (T, T) {
    #[inline(always)]
    fn map_mut<F: Fn(&mut T)>(&mut self, f: F) {
        f(&mut self.0);
        f(&mut self.1);
    }

    #[inline(always)]
    fn get(&self, val: bool) -> &T {
        match val {
            false => &self.0,
            true => &self.1,
        }
    }

    #[inline(always)]
    fn get_mut(&mut self, val: bool) -> &mut T {
        match val {
            false => &mut self.0,
            true => &mut self.1,
        }
    }

    fn iter_mut(&mut self) -> TupleMutIter<T> {
        std::iter::once((false, &mut self.0)).chain(std::iter::once((true, &mut self.1)))
    }
}

fn gen_layer_data<const N: usize>(key: [u8; AES_BLOCK_SIZE], modulus: u128, left: bool, right: bool) -> IntervalFSSData<N> {
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = (num_payload_bits + 7) / 8;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = IntervalFSSData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: ((false, false), (false, false)),
            ys: ([0; N], [0; N]),
            y_bits: ((0u128, 0u128), (0u128, 0u128)),
        };

        s.refill();
        s.fill_bytes(&mut out.seeds.0);
        s.refill();
        s.fill_bytes(&mut out.seeds.1);

        out.bits.0 = ((out.seeds.0[0] & 0x1) == 0, out.seeds.0[0] & 0x2 == 0);
        out.bits.1 = ((out.seeds.1[0] & 0x1) == 0, out.seeds.1[0] & 0x2 == 0);
        out.seeds.0[0] &= 0xFC; // Zero out first two bits
        out.seeds.1[0] &= 0xFC; // Zero out first two bits

        let mut payload_0 = vec![0u8; num_payload_bytes * (N + 1) * 2];
        let mut payload_1 = vec![0u8; num_payload_bytes * (N + 1) * 2];
        s.refill();
        s.fill_bytes(&mut payload_0);
        s.refill();
        s.fill_bytes(&mut payload_1);

        for i in 0..num_payload_bits {
            let ul: u8 = payload_0[i];
            out.y_bits.0.0 ^= (ul as u128) << (i * 8); 
            let ur: u8 = payload_0[i + num_payload_bytes * (N + 1)];
            out.y_bits.0.1 ^= (ur as u128) << (i * 8);
            let vl: u8 = payload_1[i];
            out.y_bits.1.0 ^= (vl as u128) << (i * 8);
            let vr: u8 = payload_1[i + num_payload_bytes * (N + 1)];
            out.y_bits.1.1 ^= (vr as u128) << (i * 8);
        }

        for i in 0..N {
            for j in 0..num_payload_bytes {
                let ul: u8 = payload_0[(i + 1) * num_payload_bytes + j];
                out.ys.0.0[i] ^= (ul as u128) << (j * 8);
                let ur: u8 = payload_0[(i + 1) * num_payload_bytes + j + num_payload_bytes * (N + 1)];
                out.ys.0.1[i] ^= (ur as u128) << (j * 8);
                let vl: u8 = payload_1[(i + 1) * num_payload_bytes + j];
                out.ys.1.0[i] ^= (vl as u128) << (j * 8);
                let vr: u8 = payload_1[(i + 1) * num_payload_bytes + j + num_payload_bytes * (N + 1)];
                out.ys.1.1[i] ^= (vr as u128) << (j * 8);
            }
        }

        out
    })
}

fn gen_cor_word(
    alpha_bit: bool,
    beta_bit: bool,
    left: u128,
    mid: u128,
    right: u128,
    side_bit: bool,
    modulus: u128,
    seeds: &mut [[u8; AES_BLOCK_SIZE]],
    bits: &mut [(bool, bool)],
    y_bits: &mut [(u128, u128)],
) -> IntervalFSSCW
{
    let modulus_mask = modulus - 1;
    let mut data = vec![];
    seeds.iter().for_each(|seed| data.push((gen_layer_data(seed.0, modulus, true, true), 
                                                       gen_layer_data(seed.1, modulus, true, true))));
    let mut delta_seed = vec![];
    let mut delta_bits = vec![];
    let mut delta_ys = vec![];
    let mut delta_y_bits = vec![];
    data.iter().for_each(|(d0, d1)| {
        delta_seed.push((xor::<16>(d0.seeds.0, d1.seeds.0), xor::<16>(d0.seeds.1, d1.seeds.1)));
        delta_bits.push(((d0.bits.0.0 ^ d1.bits.0.0, d0.bits.0.1 ^ d1.bits.0.1),
                         (d0.bits.1.0 ^ d1.bits.1.0, d0.bits.1.1 ^ d1.bits.1.1)));
        delta_ys.push(((d0.ys.0 + modulus - d1.ys.0) & modulus_mask, (d0.ys.1 + modulus - d1.ys.1) & modulus_mask));
        delta_y_bits.push((((d0.y_bits.0.0 + modulus - d1.y_bits.0.0) & modulus_mask, (d0.y_bits.0.1 + modulus - d1.y_bits.0.1) & modulus_mask),
                           ((d0.y_bits.1.0 + modulus - d1.y_bits.1.0) & modulus_mask, (d0.y_bits.1.1 + modulus - d1.y_bits.1.1) & modulus_mask)));
    });

    if data.len() > 2 {
        panic!("Something went wrong, data length is greater than 2: {}", data.len());
    }

    if data.len() == 1 {
        if alpha_bit == 1 && beta_bit == 0 {
            panic!("Alpha should be smaller than beta, but got alpha_bit = 1 and beta_bit = 0");
        }
        let seed1 = rand::rng().random::<u128>().to_le_bytes();
        let bits1 = ((rand::rng().random::<bool>(), rand::rng().random::<bool>()), 
                     (rand::rng().random::<bool>(), rand::rng().random::<bool>()));
        let ys1 = (rand::rng().random::<u128>() & modulus_mask, rand::rng().random::<u128>() & modulus_mask);
        let y_bits1 = (((rand::rng().random::<u128>() & modulus_mask, rand::rng().random::<u128>() & modulus_mask), 
                        (rand::rng().random::<u128>() & modulus_mask, rand::rng().random::<u128>() & modulus_mask)));
        let mut cw = IntervalFSSCW {
            seeds: ([0u8; 16], seed1),
            bits: (((false, false), (false, false)), bits1),
            ys: ((0, 0), ys1),
            y_bits: (((0, 0), (0, 0)), y_bits1),
        };
        if alpha_bit == 0 && beta_bit == 1 {
            let mut s = rand::rng().random::<u128>();
            cw.seeds.0 = s.to_le_bytes();
            cw.bits.0 = ((true ^ delta_bits[0].0.0, false ^ delta_bits[0].0.1), (false ^ delta_bits[0].1.0, true ^ delta_bits[0].1.1));
            cw.ys.0 = ((mid + modulus - delta_ys[0].0) & modulus_mask, (mid + modulus - delta_ys[0].1) & modulus_mask);
            cw.y_bits.0 = ((((1 + modulus - delta_y_bits[0].0.0) & modulus_mask, (0 + modulus - delta_y_bits[0].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[0].1.0) & modulus_mask, (1 + modulus - delta_y_bits[0].1.1) & modulus_mask)));
        } else if alpha_bit == 0 {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = ((true ^ delta_bits[0].0.0, false ^ delta_bits[0].0.1), (false ^ delta_bits[0].1.0, false ^ delta_bits[0].1.1));
            cw.ys.0 = ((mid + modulus - delta_ys[0].0) & modulus_mask, (right + modulus - delta_ys[0].1) & modulus_mask);
            cw.y_bits.0 = ((((1 + modulus - delta_y_bits[0].0.0) & modulus_mask, (0 + modulus - delta_y_bits[0].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[0].1.0) & modulus_mask, (0 + modulus - delta_y_bits[0].1.1) & modulus_mask)));
        } else if alpha_bit == 1 {
            let cw = IntervalFSSCW {
                seeds: (delta_seed[0].0,
                        seed1),
                bits: (((false ^ delta_bits[0].0.0, false ^ delta_bits[0].0.1), (true ^ delta_bits[0].1.0, false ^ delta_bits[0].1.1)),
                            bits1),
                ys: (((left + modulus - delta_ys[0].0) & modulus_mask, (mid + modulus - delta_ys[0].1) & modulus_mask),
                        ys1),
                y_bits: (((((0 + modulus - delta_y_bits[0].0.0) & modulus_mask, (0 + modulus - delta_y_bits[0].0.1) & modulus_mask), 
                           ((1 + modulus - delta_y_bits[0].1.0) & modulus_mask, (0 + modulus - delta_y_bits[0].1.1) & modulus_mask))),
                            y_bits1),
            };
        }
    } else {
        if alpha_bit == 0 {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = ((true ^ delta_bits[0].0.0, false ^ delta_bits[0].0.1), (false ^ delta_bits[0].1.0, false ^ delta_bits[0].1.1));
            cw.ys.0 = ((mid + modulus - delta_ys[0].0) & modulus_mask, (right + modulus - delta_ys[0].1) & modulus_mask);
            cw.y_bits.0 = ((((1 + modulus - delta_y_bits[0].0.0) & modulus_mask, (0 + modulus - delta_y_bits[0].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[0].1.0) & modulus_mask, (0 + modulus - delta_y_bits[0].1.1) & modulus_mask)));
        } else {
            cw.seeds.0 = delta_seed[0].0;
            cw.bits.0 = ((false ^ delta_bits[0].0.0, false ^ delta_bits[0].0.1), (true ^ delta_bits[0].1.0, false ^ delta_bits[0].1.1));
            cw.ys.0 = ((left + modulus - delta_ys[0].0) & modulus_mask, (mid + modulus - delta_ys[0].1) & modulus_mask);
            cw.y_bits.0 = ((((1 + modulus - delta_y_bits[0].0.0) & modulus_mask, (0 + modulus - delta_y_bits[0].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[0].1.0) & modulus_mask, (0 + modulus - delta_y_bits[0].1.1) & modulus_mask)));
        }

        if beta_bit == 0 {
            cw.seeds.1 = delta_seed[1].1;
            cw.bits.1 = ((false ^ delta_bits[1].0.0, true ^ delta_bits[1].0.1), (false ^ delta_bits[1].1.0, false ^ delta_bits[1].1.1));
            cw.ys.1 = ((mid + modulus - delta_ys[1].0) & modulus_mask, (right + modulus - delta_ys[1].1) & modulus_mask);
            cw.y_bits.1 = ((((0 + modulus - delta_y_bits[1].0.0) & modulus_mask, (1 + modulus - delta_y_bits[1].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[1].1.0) & modulus_mask, (0 + modulus - delta_y_bits[1].1.1) & modulus_mask)));
        } else {
            cw.seeds.1 = delta_seed[1].0;
            cw.bits.1 = ((false ^ delta_bits[1].0.0, false ^ delta_bits[1].0.1), (false ^ delta_bits[1].1.0, true ^ delta_bits[1].1.1));
            cw.ys.1 = ((left + modulus - delta_ys[1].0) & modulus_mask, (mid + modulus - delta_ys[1].1) & modulus_mask);
            cw.y_bits.1 = ((((0 + modulus - delta_y_bits[1].0.0) & modulus_mask, (0 + modulus - delta_y_bits[1].0.1) & modulus_mask), 
                           ((0 + modulus - delta_y_bits[1].1.0) & modulus_mask, (1 + modulus - delta_y_bits[1].1.1) & modulus_mask)));
        }
    }

    let mut new_seeds = vec![];
    let mut new_bits = vec![];
    let mut new_y_bits = vec![];

    if data.len() == 1 {
        if alpha_bit == 0 && beta_bit == 1 {
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.0, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.1, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_bits.push((data[0].bits.0.0 ^ (cw.bits.0.0.0 & bits[0].0) ^ (cw.bits.1.0.0 & bits[0].1),
                            data[0].bits.0.1 ^ (cw.bits.0.0.1 & bits[0].0) ^ (cw.bits.1.0.1 & bits[0].1)));
            new_bits.push((data[0].bits.1.0 ^ (cw.bits.0.1.0 & bits[0].0) ^ (cw.bits.1.1.0 & bits[0].1),
                            data[0].bits.1.1 ^ (cw.bits.0.1.1 & bits[0].0) ^ (cw.bits.1.1.1 & bits[0].1)));
            new_y_bits.push(((data[0].y_bits.0.0 + ((cw.y_bits.0.0.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.0.1 + ((cw.y_bits.0.0.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.1 * bits[0].1) & modulus_mask)) & modulus_mask));    
            new_y_bits.push(((data[0].y_bits.1.0 + ((cw.y_bits.0.1.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.1.1 + ((cw.y_bits.0.1.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.1 * bits[0].1) & modulus_mask)) & modulus_mask));
        } else if alpha_bit == 0 {
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.0, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_bits.push((data[0].bits.0.0 ^ (cw.bits.0.0.0 & bits[0].0) ^ (cw.bits.1.0.0 & bits[0].1),
                            data[0].bits.0.1 ^ (cw.bits.0.0.1 & bits[0].0) ^ (cw.bits.1.0.1 & bits[0].1)));
            new_y_bits.push(((data[0].y_bits.0.0 + ((cw.y_bits.0.0.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.0.1 + ((cw.y_bits.0.0.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.1 * bits[0].1) & modulus_mask)) & modulus_mask));
        } else if alpha_bit == 1 {
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.1, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_bits.push((data[0].bits.1.0 ^ (cw.bits.0.1.0 & bits[0].0) ^ (cw.bits.1.1.0 & bits[0].1),
                            data[0].bits.1.1 ^ (cw.bits.0.1.1 & bits[0].0) ^ (cw.bits.1.1.1 & bits[0].1)));
            new_y_bits.push(((data[0].y_bits.1.0 + ((cw.y_bits.0.1.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.1.1 + ((cw.y_bits.0.1.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.1 * bits[0].1) & modulus_mask)) & modulus_mask));
        }
    } else {
        if alpha_bit == 0 {
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.0, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_bits.push((data[0].bits.0.0 ^ (cw.bits.0.0.0 & bits[0].0) ^ (cw.bits.1.0.0 & bits[0].1),
                            data[0].bits.0.1 ^ (cw.bits.0.0.1 & bits[0].0) ^ (cw.bits.1.0.1 & bits[0].1)));
            new_y_bits.push(((data[0].y_bits.0.0 + ((cw.y_bits.0.0.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.0.1 + ((cw.y_bits.0.0.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.0.1 * bits[0].1) & modulus_mask)) & modulus_mask));
        } else {
            new_seeds.push(xor::<16>(xor::<16>(data[0].seeds.1, and_bit::<16>(cw.seeds.0, bits[0].0)), and_bit::<16>(cw.seeds.1, bits[0].1)));
            new_bits.push((data[0].bits.1.0 ^ (cw.bits.0.1.0 & bits[0].0) ^ (cw.bits.1.1.0 & bits[0].1),
                            data[0].bits.1.1 ^ (cw.bits.0.1.1 & bits[0].0) ^ (cw.bits.1.1.1 & bits[0].1)));
            new_y_bits.push(((data[0].y_bits.1.0 + ((cw.y_bits.0.1.0 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.0 * bits[0].1) & modulus_mask)) & modulus_mask,
                                (data[0].y_bits.1.1 + ((cw.y_bits.0.1.1 * bits[0].0) & modulus_mask) + ((cw.y_bits.1.1.1 * bits[0].1) & modulus_mask)) & modulus_mask));
        }

        if beta_bit == 0 {
            new_seeds.push(xor::<16>(xor::<16>(data[1].seeds.0, and_bit::<16>(cw.seeds.0, bits[1].0)), and_bit::<16>(cw.seeds.1, bits[1].1)));
            new_bits.push((data[1].bits.0.0 ^ (cw.bits.0.0.0 & bits[1].0) ^ (cw.bits.1.0.0 & bits[1].1),
                            data[1].bits.0.1 ^ (cw.bits.0.0.1 & bits[1].0) ^ (cw.bits.1.0.1 & bits[1].1)));
            new_y_bits.push(((data[1].y_bits.0.0 + ((cw.y_bits.0.0.0 * bits[1].0) & modulus_mask) + ((cw.y_bits.1.0.0 * bits[1].1) & modulus_mask)) & modulus_mask,
                                (data[1].y_bits.0.1 + ((cw.y_bits.0.0.1 * bits[1].0) & modulus_mask) + ((cw.y_bits.1.0.1 * bits[1].1) & modulus_mask)) & modulus_mask));
        } else {
            new_seeds.push(xor::<16>(xor::<16>(data[1].seeds.1, and_bit::<16>(cw.seeds.0, bits[1].0)), and_bit::<16>(cw.seeds.1, bits[1].1)));
            new_bits.push((data[1].bits.1.0 ^ (cw.bits.0.1.0 & bits[1].0) ^ (cw.bits.1.1.0 & bits[1].1),
                            data[1].bits.1.1 ^ (cw.bits.0.1.1 & bits[1].0) ^ (cw.bits.1.1.1 & bits[1].1)));
            new_y_bits.push(((data[1].y_bits.1.0 + ((cw.y_bits.0.1.0 * bits[1].0) & modulus_mask) + ((cw.y_bits.1.1.0 * bits[1].1) & modulus_mask)) & modulus_mask,
                                (data[1].y_bits.1.1 + ((cw.y_bits.0.1.1 * bits[1].0) & modulus_mask) + ((cw.y_bits.1.1.1 * bits[1].1) & modulus_mask)) & modulus_mask));
        }
    }

    seeds = new_seeds;
    bits = new_bits;
    y_bits = new_y_bits;

    cw
}

/// All-prefix DPF implementation.
impl IntervalFSSKey
{

    // Need alpha < beta
    pub fn gen_IntervalFSSKey(alpha_bits: &[bool], beta_bits: &[bool], a: u128, b: u128, c: u128, modulus: u128, side : bool) -> (IntervalFSSKey, IntervalFSSKey) {
        assert!(alpha_bits.len() == beta_bits.len());
        assert!(modulus > 0 && (modulus & (modulus-1)) == 0, "Modulus must be a power of 2");
        assert!(a < modulus && b < modulus && c < modulus, "a, b, c must be less than modulus");

        let u = alpha_bits.len();
        let modulus_mask = modulus - 1;
        let mut payload_left = vec![(a + modulus - b) & modulus_mask; u];
        payload_left[0] = a;
        let mut payload_mid = vec![0; u];
        payload_mid[0] = b;
        let mut payload_right = vec![(c + modulus - b) & modulus_mask; u];
        payload_right[0] = c;   

        let root_seeds = [(prg::PrgSeed::random(), prg::PrgSeed::random())];
        let root_bits = [((false, false), (true, false))];
        let root_y_bits = [((0u128, 0u128), (0u128, 0u128))];

        let mut seeds = root_seeds.clone();
        let mut bits = root_bits;
        let mut y_bits = root_y_bits;

        let mut cor_words: Vec<IntervalFSSCW> = Vec::new();

        for (i, (&alpha_bit, &beta_bit)) in alpha_bits.iter().zip(beta_bits.iter()).enumerate() {
            let cw = gen_cor_word(
                alpha_bit, 
                beta_bit, 
                payload_left[i],
                payload_mid[i], 
                payload_right[i], 
                side, 
                modulus,
                &mut seeds,
                &mut bits,
                &mut y_bits
            );
            cor_words.push(cw);
        }

        (
            IntervalFSSKey {
                key_idx: false,
                root_seed: root_seeds[0].0,
                cor_words: cor_words.clone(),
            },
            IntervalFSSKey {
                key_idx: true,
                root_seed: root_seeds[0].1,
                cor_words,
            },
        )
    }


    pub fn eval_bit(&self, state: &EvalState, modulus: u128, dir: bool) -> EvalState {
        let modulus_mask = modulus - 1;
        let data = gen_layer_data(state.key, modulus, dir, !dir);
        let mut seed = data.seeds.get(dir).clone();
        let mut new_bit = data.bits.get(dir);
        let mut new_y = data.ys.get(dir);
        let mut new_y_bit = data.y_bits.get(dir);

        let cw = self.cor_words.get(state.level).unwrap();

        seed = xor::<16>(xor::<16>(&seed, and_bit::<16>(&cw.seed.0, &state.bit.0)), 
                       and_bit::<16>(&cw.seed.1, &state.bit.1));
        new_bit = new_bit ^ (cw.bits.get(dir).0 & state.bit.0) ^ (cw.bits.get(!dir).1 & state.bit.1);
        new_y = (new_y + (cw.ys.get(dir).0 * state.y_bit.0) & modulus_mask
                    + (cw.ys.get(!dir).1 * state.y_bit.1) & modulus_mask) & modulus_mask;
        new_y_bit = (new_y_bit + (cw.y_bits.get(dir).0 * state.y_bit.0) & modulus_mask
                    + (cw.y_bits.get(!dir).1 * state.y_bit.1) & modulus_mask) & modulus_mask;

        new_y = (new_y + state.y) & modulus_mask;

        EvalState {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        }
    }

    pub fn eval_init(&self) -> EvalState {
        EvalState {
            level: 0,
            seed: self.root_seed.clone(),
            bit: self.key_idx,
            y: 0,
            y_bit: self.key_idx
        }
    }

    pub fn eval_ibDCF(&self, idx: &[bool], modulus: u128) -> bool {
        debug_assert!(idx.len() <= self.domain_size());
        debug_assert!(!idx.is_empty());
        let mut state = self.eval_init();

        for i in 0..idx.len() {
            let bit = idx[i];
            let state_new = self.eval_bit(&state, modulus, bit);
            state = state_new;
        }

        state.y_bit ^ state.bit
    }

    pub fn domain_size(&self) -> usize {
        self.cor_words.len()
    }
}