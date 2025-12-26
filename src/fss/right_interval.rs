// The interval in this file is different.
// If a prefix is LESS THAN the prefix of beta, it will take the mid payload.

use crate::data_structures::mod2k::Mod2k;
use crate::util::{xor, and_bit};
use crate::bytes_to_u128;
use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use crate::data_structures::ringvec::RingVec;
use crate::data_structures::pair::Pair;

use rand_core::RngCore; 
use rand::Rng;
use std::cell::RefCell;
use std::convert::TryInto;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

#[derive(Clone, Debug, PartialEq)]
pub struct RIntervalFSSCW<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], 
                [u8; AES_BLOCK_SIZE]),
    pub bits: ((Pair<bool>, Pair<bool>),
               (Pair<bool>, Pair<bool>)),
    pub ys: ((RingVec, RingVec), 
             (RingVec, RingVec)),
    pub y_bits: ((Pair<Mod2k>, Pair<Mod2k>),
                 (Pair<Mod2k>, Pair<Mod2k>)),
}

impl<const N: usize> RIntervalFSSCW<N> {
    pub fn bytes_size(modulus: u128) -> usize {
        // Calculate the size of the serialized RIntervalFSSCW<N> structure
        let mut size = 0;
        
        // seeds: 2 * AES_BLOCK_SIZE
        size += 2 * AES_BLOCK_SIZE;
        
        // bits: 1 byte (8 bools packed into 1 byte)
        size += 1;
        
        // ys: 4 RingVec<N>
        let ringvec_bytes_size = RingVec::byte_size_for_modulus_len(N, modulus);
        size += 4 * ringvec_bytes_size;
        
        // y_bits: 1 RingVec<8> (8 ModInt values)
        let y_bits_bytes_size = RingVec::byte_size_for_modulus_len(8, modulus);
        size += y_bits_bytes_size;
        
        size
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.seeds.0);
        out.extend_from_slice(&self.seeds.1);
        let mut bits_byte = 0u8;
        bits_byte |= (self.bits.0.0.first as u8) << 0;
        bits_byte |= (self.bits.0.0.second as u8) << 1;
        bits_byte |= (self.bits.0.1.first as u8) << 2;
        bits_byte |= (self.bits.0.1.second as u8) << 3;
        bits_byte |= (self.bits.1.0.first as u8) << 4;
        bits_byte |= (self.bits.1.0.second as u8) << 5;
        bits_byte |= (self.bits.1.1.first as u8) << 6;
        bits_byte |= (self.bits.1.1.second as u8) << 7;
        out.push(bits_byte);
        out.extend(self.ys.0.0.to_bytes());
        out.extend(self.ys.0.1.to_bytes());
        out.extend(self.ys.1.0.to_bytes());
        out.extend(self.ys.1.1.to_bytes());
        // y_bits (4 ModInt)
        let mut y_bits_list = Vec::new();
        y_bits_list.push(self.y_bits.0.0.first.val());
        y_bits_list.push(self.y_bits.0.0.second.val());
        y_bits_list.push(self.y_bits.0.1.first.val());
        y_bits_list.push(self.y_bits.0.1.second.val());
        y_bits_list.push(self.y_bits.1.0.first.val());
        y_bits_list.push(self.y_bits.1.0.second.val());
        y_bits_list.push(self.y_bits.1.1.first.val());
        y_bits_list.push(self.y_bits.1.1.second.val());
        let y_bits_ringvec = RingVec::new(y_bits_list, self.ys.0.0.modulus())
            .expect("Failed to create RingVec from y_bits");
        out.extend(y_bits_ringvec.to_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let mut seeds0 = [0u8; AES_BLOCK_SIZE];
        let mut seeds1 = [0u8; AES_BLOCK_SIZE];
        seeds0.copy_from_slice(&bytes[offset..offset+AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        seeds1.copy_from_slice(&bytes[offset..offset+AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let bits_byte = bytes[offset];
        offset += 1;
        let bits = ((
            Pair::new((bits_byte & 0x1) != 0, (bits_byte & 0x2) != 0),
            Pair::new((bits_byte & 0x4) != 0, (bits_byte & 0x8) != 0),
        ), (
            Pair::new((bits_byte & 0x10) != 0, (bits_byte & 0x20) != 0),
            Pair::new((bits_byte & 0x40) != 0, (bits_byte & 0x80) != 0),
        )
        );

        // ys
        let (ys00, used00) = RingVec::from_bytes(&bytes[offset..], modulus).expect("Failed to parse ys00");
        offset += used00;
        let (ys01, used01) = RingVec::from_bytes(&bytes[offset..], modulus).expect("Failed to parse ys01");
        offset += used01;
        let (ys10, used10) = RingVec::from_bytes(&bytes[offset..], modulus).expect("Failed to parse ys10");
        offset += used10;
        let (ys11, used11) = RingVec::from_bytes(&bytes[offset..], modulus).expect("Failed to parse ys11");
        offset += used11;
        // y_bits
        let (y_bits_ringvec, used_y_bits) = RingVec::from_bytes(&bytes[offset..], modulus).expect("Failed to parse y_bits");
        offset += used_y_bits;
        assert_eq!(ys00.len(), N, "Unexpected ys00 length");
        assert_eq!(ys01.len(), N, "Unexpected ys01 length");
        assert_eq!(ys10.len(), N, "Unexpected ys10 length");
        assert_eq!(ys11.len(), N, "Unexpected ys11 length");
        assert_eq!(y_bits_ringvec.len(), 8, "Unexpected y_bits length");
        let y_bits = ((
            Pair::new(Mod2k::new(y_bits_ringvec[0], modulus), Mod2k::new(y_bits_ringvec[1], modulus)),
            Pair::new(Mod2k::new(y_bits_ringvec[2], modulus), Mod2k::new(y_bits_ringvec[3], modulus))
        ), (
            Pair::new(Mod2k::new(y_bits_ringvec[4], modulus), Mod2k::new(y_bits_ringvec[5], modulus)),
            Pair::new(Mod2k::new(y_bits_ringvec[6], modulus), Mod2k::new(y_bits_ringvec[7], modulus))
        ));
        (
            RIntervalFSSCW {
                seeds: (seeds0, seeds1),
                bits,
                ys: ((ys00, ys01), (ys10, ys11)),
                y_bits,
            },
            offset
        )
    }
}

#[derive(Clone, Debug)]
pub struct RIntervalFSSData<const N: usize> {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: (Pair<bool>, Pair<bool>),
    pub ys: (RingVec, RingVec),
    pub y_bits: (Pair<Mod2k>, Pair<Mod2k>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RIntervalFSSKey<const N: usize> {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<RIntervalFSSCW<N>>,
}

impl<const N: usize> RIntervalFSSKey<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.key_idx as u8);
        out.extend_from_slice(&self.root_seed);
        let num_words = self.cor_words.len() as u32;
        out.extend_from_slice(&num_words.to_le_bytes());
        for cw in &self.cor_words {
            let cw_bytes = cw.to_bytes();
            out.extend(cw_bytes);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let key_idx = bytes[offset] != 0;
        offset += 1;
        let mut root_seed = [0u8; AES_BLOCK_SIZE];
        root_seed.copy_from_slice(&bytes[offset..offset+AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let num_words = u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        let mut cor_words = Vec::with_capacity(num_words);
        for _ in 0..num_words {
            let (cw, used) = RIntervalFSSCW::<N>::from_bytes(&bytes[offset..], modulus);
            cor_words.push(cw);
            offset += used;
        }
        (
            RIntervalFSSKey {
                key_idx,
                root_seed,
                cor_words,
            },
            offset
        )
    }
}


#[derive(Clone, Debug)]
pub struct RIntervalFSSEval<const N: usize> {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: Pair<bool>,
    pub y: RingVec,
    pub y_bit: Pair<Mod2k>,
}

fn gen_layer_data<const N: usize>(key: [u8; AES_BLOCK_SIZE], modulus: u128) -> RIntervalFSSData<N> {
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = ((num_payload_bits + 7) / 8) as usize;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = RIntervalFSSData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: (Pair::<bool>::new(false, false), Pair::<bool>::new(false, false)),
            ys: (
                RingVec::zero_with_len(N, modulus).expect("Failed to create left ringvec"),
                RingVec::zero_with_len(N, modulus).expect("Failed to create right ringvec"),
            ),
            y_bits: (
                Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::zero(modulus)), 
                Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::zero(modulus))
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
        out.y_bits.0.first = Mod2k::new(bytes_to_u128(&payload_rnd[N * num_payload_bytes..(N + 1) * num_payload_bytes]), modulus);
        out.y_bits.0.second = Mod2k::new(bytes_to_u128(&payload_rnd[(N + 1) * num_payload_bytes..(N + 2) * num_payload_bytes]), modulus);

        for i in 0..N {
            out.ys.1[i] = bytes_to_u128(&payload_rnd[(i + N + 2) * num_payload_bytes..(i + N + 3) * num_payload_bytes]) % modulus;
        }
        out.y_bits.1.first = Mod2k::new(bytes_to_u128(&payload_rnd[(2 * N + 2) * num_payload_bytes..(2 * N + 3) * num_payload_bytes]), modulus);
        out.y_bits.1.second = Mod2k::new(bytes_to_u128(&payload_rnd[(2 * N + 3) * num_payload_bytes..(2 * N + 4) * num_payload_bytes]), modulus);

        out
    })
}

fn gen_cor_word<const N: usize>(
    alpha_bit: bool,
    beta_bit: bool,
    left: RingVec,
    mid0: RingVec,
    mid1: RingVec,
    right0: RingVec,
    right1: RingVec,
    modulus: u128,
    eval: &mut Vec<(RIntervalFSSEval<N>, RIntervalFSSEval<N>)>,
) -> RIntervalFSSCW<N>
{
    let mut data = vec![];
    eval.iter().for_each(|(eval0, eval1)| {
        data.push((
            gen_layer_data(eval0.seed, modulus),
            gen_layer_data(eval1.seed, modulus)
        ));
    });

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
            d1.ys.0.clone() - d0.ys.0.clone(), 
            d1.ys.1.clone() - d0.ys.1.clone()
        ));
        delta_y_bits.push((
            d1.y_bits.0 - d0.y_bits.0,
            d1.y_bits.1 - d0.y_bits.1
        ));
    });

    if data.len() > 2 {
        panic!("Something went wrong, data length is greater than 2: {}", data.len());
    }

    let seed1 = rand::rng().random::<[u8; 16]>();
    let bits1 = (
        Pair::<bool>::new(rand::rng().random::<bool>(), rand::rng().random::<bool>()), 
        Pair::<bool>::new(rand::rng().random::<bool>(), rand::rng().random::<bool>()));
    let ys1 = (
        RingVec::random_with_len(N, modulus).expect("Failed to create ys1.0"),
        RingVec::random_with_len(N, modulus).expect("Failed to create ys1.1"),
    );
    let y_bits1 = (
        Pair::<Mod2k>::new(Mod2k::random(modulus), Mod2k::random(modulus)), 
        Pair::<Mod2k>::new(Mod2k::random(modulus), Mod2k::random(modulus)));
    let mut cw = RIntervalFSSCW {
        seeds: ([0u8; 16], seed1),
        bits: ((Pair::<bool>::new(false, false), Pair::<bool>::new(false, false)), bits1),
        ys: (
            (
                RingVec::zero_with_len(N, modulus).expect("Failed to create left ys0.0"),
                RingVec::zero_with_len(N, modulus).expect("Failed to create left ys0.1"),
            ),
            ys1,
        ),
        y_bits: (
            (Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::zero(modulus)), 
             Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::zero(modulus))), 
            y_bits1),
    };

    let bool10 = Pair::<bool>::new(true, false);
    let bool01 = Pair::<bool>::new(false, true);
    let bool00 = Pair::<bool>::new(false, false);

    let mint10 = Pair::<Mod2k>::new(Mod2k::one(modulus), Mod2k::zero(modulus));
    let mint01 = Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::one(modulus));
    let mint00 = Pair::<Mod2k>::new(Mod2k::zero(modulus), Mod2k::zero(modulus));

    if data.len() == 1 {
        if alpha_bit && !beta_bit {
            panic!("Alpha should be smaller than beta, but got alpha_bit = true and beta_bit = false");
        }
        if !alpha_bit && beta_bit {
            cw.seeds.0 = rand::rng().random::<[u8; 16]>();
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool01 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0.clone() + left.clone(), delta_ys[0].1.clone() + mid0.clone());
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint01);
        } else if !alpha_bit {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool00 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0.clone() + left.clone(), delta_ys[0].1.clone() + right0.clone());
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint00);
        } else if alpha_bit {
            cw.seeds.0 = delta_seed[0].0;
            cw.bits.0 = (bool00 ^ delta_bits[0].0, bool10 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0.clone() + left.clone(), delta_ys[0].1.clone() + left.clone());
            cw.y_bits.0 = (delta_y_bits[0].0 + mint00, 
                           delta_y_bits[0].1 + mint10);
        }
    } else {
        if !alpha_bit {
            cw.seeds.0 = delta_seed[0].1;
            cw.bits.0 = (bool10 ^ delta_bits[0].0, bool00 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0.clone() + left.clone(), delta_ys[0].1.clone() + mid0.clone());
            cw.y_bits.0 = (delta_y_bits[0].0 + mint10, 
                           delta_y_bits[0].1 + mint00);
        } else {
            cw.seeds.0 = delta_seed[0].0;
            cw.bits.0 = (bool00 ^ delta_bits[0].0, bool10 ^ delta_bits[0].1);
            cw.ys.0 = (delta_ys[0].0.clone() + left.clone(), delta_ys[0].1.clone() + left.clone());
            cw.y_bits.0 = (delta_y_bits[0].0 + mint00, 
                           delta_y_bits[0].1 + mint10);
        }

        if !beta_bit {
            cw.seeds.1 = delta_seed[1].1;
            cw.bits.1 = (bool01 ^ delta_bits[1].0, bool00 ^ delta_bits[1].1);
            cw.ys.1 = (delta_ys[1].0.clone() + mid1.clone(), delta_ys[1].1.clone() + right1.clone());
            cw.y_bits.1 = (delta_y_bits[1].0 + mint01, 
                           delta_y_bits[1].1 + mint00);
        } else {
            cw.seeds.1 = delta_seed[1].0;
            cw.bits.1 = (bool00 ^ delta_bits[1].0, bool01 ^ delta_bits[1].1);
            cw.ys.1 = (delta_ys[1].0.clone() + mid1.clone(), delta_ys[1].1.clone() + mid1.clone());
            cw.y_bits.1 = (delta_y_bits[1].0 + mint00, 
                           delta_y_bits[1].1 + mint01);
        }
    }

    let mut new_seeds = Vec::<([u8; 16], [u8; 16])>::new();
    let mut new_bits = vec![];
    let mut new_y_bits = vec![];

    if data.len() == 1 {
        let (d0, d1): (RIntervalFSSData<N>, RIntervalFSSData<N>) = data[0].clone();
        let eval0 = eval[0].0.clone();
        let eval1 = eval[0].1.clone();
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
        }
        if beta_bit {
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
        let d0 = data[0].0.clone();
        let d1 = data[0].1.clone();
        let eval0 = eval[0].0.clone();
        let eval1 = eval[0].1.clone();
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

        let (d0, d1): (RIntervalFSSData<N>, RIntervalFSSData<N>) = data[1].clone();
        let eval0 = eval[1].0.clone();
        let eval1 = eval[1].1.clone();

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

    let new_eval: Vec<(RIntervalFSSEval<N>, RIntervalFSSEval<N>)> = new_seeds.iter().zip(new_bits.iter()).zip(new_y_bits.iter())
        .map(|((seed, bits), y_bits)| {
            (
                RIntervalFSSEval {
                    level: 0,
                    seed: seed.0,
                    bit: bits.0,
                    y: RingVec::zero_with_len(N, modulus).expect("Failed to create left eval y"),
                    y_bit: y_bits.0.clone(),
                },
                RIntervalFSSEval {
                    level: 0,
                    seed: seed.1,
                    bit: bits.1,
                    y: RingVec::zero_with_len(N, modulus).expect("Failed to create right eval y"),
                    y_bit: y_bits.1.clone(),
                }
            )
        }).collect();
    
    eval.clear();
    eval.extend(new_eval);

    cw
}

/// All-prefix DPF implementation.
impl<const N: usize> RIntervalFSSKey<N>
{

    // Need alpha < beta
    pub fn gen_rinterval_fss_key(alpha_bits: &[bool], beta_bits: &[bool], a: RingVec, b: RingVec, c: RingVec, modulus: u128) -> (RIntervalFSSKey<N>, RIntervalFSSKey<N>) {
        assert!(alpha_bits.len() == beta_bits.len());
        assert!(modulus > 0 && (modulus & (modulus-1)) == 0, "Modulus must be a power of 2");

        let u = alpha_bits.len();
        let zero = RingVec::zero_with_len(N, modulus).expect("Failed to create zero ringvec");
        let mut payload_left = vec![zero.clone(); u];
        payload_left[0] = a.clone();
        let mut payload_mid0 = vec![b.clone() - a.clone(); u];
        payload_mid0[0] = b.clone();
        let mut payload_mid1 = vec![zero.clone(); u];
        payload_mid1[0] = b.clone();
        let mut payload_right0 = vec![c.clone() - a.clone(); u];
        payload_right0[0] = c.clone();   
        let mut payload_right1 = vec![c.clone() - b.clone(); u];
        payload_right1[0] = c.clone();

        let root_seeds = (rand::rng().random::<[u8; 16]>(), rand::rng().random::<[u8; 16]>());

        let eval0 = RIntervalFSSEval {
            level: 0,
            seed: root_seeds.0,
            bit: Pair::new(true, false),
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create eval0 y"),
            y_bit: Pair::new(Mod2k::one(modulus), Mod2k::zero(modulus)),
        };

        let eval1 = RIntervalFSSEval {
            level: 0,
            seed: root_seeds.1,
            bit: Pair::new(false, false),
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create eval1 y"),
            y_bit: Pair::new(Mod2k::zero(modulus), Mod2k::zero(modulus)),
        };

        let mut eval = vec![(eval0.clone(), eval1.clone())];

        let mut cor_words: Vec<RIntervalFSSCW<N>> = Vec::new();

        for (i, (&alpha_bit, &beta_bit)) in alpha_bits.iter().zip(beta_bits.iter()).enumerate() {
            let cw = gen_cor_word(
                alpha_bit, 
                beta_bit, 
                payload_left[i].clone(),
                payload_mid0[i].clone(), 
                payload_mid1[i].clone(),
                payload_right0[i].clone(), 
                payload_right1[i].clone(),
                modulus,
                &mut eval
            );
            cor_words.push(cw);
        }

        (
            RIntervalFSSKey {
                key_idx: false,
                root_seed: root_seeds.0,
                cor_words: cor_words.clone(),
            },
            RIntervalFSSKey {
                key_idx: true,
                root_seed: root_seeds.1,
                cor_words,
            },
        )
    }

    pub fn eval_bit(&self, state: &RIntervalFSSEval<N>, modulus: u128, dir: bool) -> RIntervalFSSEval<N> {
        let data: RIntervalFSSData<N> = gen_layer_data(state.seed, modulus);
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

        let cw = self.cor_words[state.level].clone();

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

        new_y = new_y + state.y.clone();

        RIntervalFSSEval {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        }
    }

    pub fn eval_init(&self, modulus: u128) -> RIntervalFSSEval<N> {
        let y_bit_first = if self.key_idx {
            Mod2k::zero(modulus)
        } else {
            Mod2k::one(modulus)
        };
        RIntervalFSSEval {
            level: 0,
            seed: self.root_seed.clone(),
            bit: Pair::new(!self.key_idx, false),
            y: RingVec::zero_with_len(N, modulus).expect("Failed to create init y"),
            y_bit: Pair::new(y_bit_first, Mod2k::zero(modulus)),
        }
    }

    pub fn eval_rinterval_fss(&self, idx: &[bool], modulus: u128) -> RingVec {
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
