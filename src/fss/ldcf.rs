use crate::aes::{FixedKeyPrgStream, AES_BLOCK_SIZE};
use crate::bytes_to_u128;
use crate::data_structures::mod2k::Mod2k;
use crate::data_structures::ringvec::RingVec;
use crate::util::{and_bit, xor, xor_u8_16};

use anyhow::{anyhow, ensure, Context, Result};
use rand::Rng;
use rand_core::RngCore;
use std::cell::RefCell;
use std::convert::TryInto;

thread_local!(static FIXED_KEY_STREAM: RefCell<FixedKeyPrgStream> = RefCell::new(FixedKeyPrgStream::new()));

fn serialize_ringvec(vec: &RingVec) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&vec.len().to_le_bytes());
    out.extend_from_slice(&vec.to_bytes()?);
    Ok(out)
}

fn deserialize_ringvec(bytes: &[u8], modulus: u128) -> Result<(RingVec, usize)> {
    let len_size = std::mem::size_of::<usize>();
    ensure!(
        bytes.len() >= len_size,
        "Not enough bytes to read RingVec length"
    );
    let len = usize::from_le_bytes(
        bytes[..len_size]
            .try_into()
            .context("Failed to read RingVec length")?,
    );
    let (vec, used) = RingVec::from_bytes(&bytes[len_size..], modulus, len)?;
    Ok((vec, len_size + used))
}

#[derive(Clone, Debug, PartialEq)]
pub struct LdcfCW {
    pub seed: [u8; AES_BLOCK_SIZE],
    pub seed_bit: (bool, bool),
    pub ys: (RingVec, RingVec),
    pub y_bits: (Mod2k, Mod2k),
}

impl LdcfCW {
    fn check_wellformedness(&self) -> Result<()> {
        ensure!(
            self.ys.0.len() == self.ys.1.len(),
            "Size mismatch between ys of LdcfCW, got {} and {}",
            self.ys.0.len(),
            self.ys.1.len()
        );
        ensure!(
            self.ys.0.modulus() == self.ys.1.modulus(),
            "Modulus mismatch between ys of LdcfCW, got {} and {}",
            self.ys.0.modulus(),
            self.ys.1.modulus()
        );
        Ok(())
    }

    pub fn bytes_size(&self) -> Result<usize> {
        self.check_wellformedness()
            .map_err(|err| anyhow!("Wrong form for LdcfCW: {}", err))?;
        let mut size = 0;
        size += AES_BLOCK_SIZE;
        size += 1; // for seed_bit
        let length = self.ys.0.len();
        let modulus = self.ys.0.modulus();
        size += 2 * RingVec::byte_size_for_modulus_len(length, modulus);
        size += RingVec::byte_size_for_modulus_len(2, modulus);
        Ok(size)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let bytes_size = self
            .bytes_size()
            .map_err(|err| anyhow!("Cannot calculate LdcfCW bytes size: {}", err))?;
        let mut out = Vec::with_capacity(bytes_size);
        out.extend_from_slice(&self.seed);
        let mut bits_byte = 0u8;
        bits_byte |= (self.seed_bit.0 as u8) << 0;
        bits_byte |= (self.seed_bit.1 as u8) << 1;
        out.push(bits_byte);
        out.extend_from_slice(&serialize_ringvec(&self.ys.0)?);
        out.extend_from_slice(&serialize_ringvec(&self.ys.1)?);
        let y_bits_ringvec = RingVec::new(
            vec![self.y_bits.0.val, self.y_bits.1.val],
            self.ys.0.modulus(),
        )
        .context("Failed to create RingVec from y_bits")?;
        out.extend_from_slice(&serialize_ringvec(&y_bits_ringvec)?);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and greater than 0"
        );
        ensure!(
            bytes.len() >= AES_BLOCK_SIZE + 1,
            "Not enough bytes to read LdcfCW header"
        );
        let mut offset = 0;
        let mut seed = [0u8; AES_BLOCK_SIZE];
        seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;

        let bits_byte = bytes[offset];
        offset += 1;
        let seed_bits = (bits_byte & 0x1 != 0, bits_byte & 0x2 != 0);

        let (ys0, used0) = deserialize_ringvec(&bytes[offset..], modulus)
            .context("Failed to create RingVec from ys0")?;
        offset += used0;

        let (ys1, used1) = deserialize_ringvec(&bytes[offset..], modulus)
            .context("Failed to create RingVec from ys1")?;
        offset += used1;

        let (y_bits_ringvec, used_y_bits) = deserialize_ringvec(&bytes[offset..], modulus)
            .context("Failed to create RingVec from y_bits")?;
        offset += used_y_bits;

        ensure!(ys0.len() == ys1.len(), "Size mismatch in ys: {} vs {}", ys0.len(), ys1.len());
        ensure!(y_bits_ringvec.len() == 2, "Unexpected y_bits length {}", y_bits_ringvec.len());

        let y_bits = (
            Mod2k::new(y_bits_ringvec[0], modulus),
            Mod2k::new(y_bits_ringvec[1], modulus),
        );

        Ok((
            LdcfCW {
                seed,
                seed_bit: seed_bits,
                ys: (ys0, ys1),
                y_bits,
            },
            offset,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct LdcfData {
    pub seeds: ([u8; AES_BLOCK_SIZE], [u8; AES_BLOCK_SIZE]),
    pub bits: (bool, bool),
    pub ys: (RingVec, RingVec),
    pub y_bits: (Mod2k, Mod2k),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LdcfKey {
    pub key_idx: bool,
    pub root_seed: [u8; AES_BLOCK_SIZE],
    pub cor_words: Vec<LdcfCW>,
}

impl LdcfKey {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.push(self.key_idx as u8);
        out.extend_from_slice(&self.root_seed);
        let num_words = self.cor_words.len() as u32;
        out.extend_from_slice(&num_words.to_le_bytes());
        for cw in &self.cor_words {
            out.extend_from_slice(&cw.to_bytes()?);
        }
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        ensure!(
            bytes.len() >= 1 + AES_BLOCK_SIZE + 4,
            "Not enough bytes to read LdcfKey header"
        );
        let mut offset = 0;
        let key_idx = bytes[offset] != 0;
        offset += 1;
        let mut root_seed = [0u8; AES_BLOCK_SIZE];
        root_seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let num_words = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .context("Failed to read num_words")?,
        ) as usize;
        offset += 4;
        let mut cor_words = Vec::with_capacity(num_words);
        for _ in 0..num_words {
            let (cw, used) = LdcfCW::from_bytes(&bytes[offset..], modulus)
                .context("Invalid LdcfCW from bytes")?;
            cor_words.push(cw);
            offset += used;
        }
        Ok((
            LdcfKey {
                key_idx,
                root_seed,
                cor_words,
            },
            offset,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct LdcfEval {
    level: usize,
    seed: [u8; AES_BLOCK_SIZE],
    pub bit: bool,
    y: RingVec,
    pub y_bit: Mod2k,
}

impl LdcfEval {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.level.to_le_bytes());
        out.extend_from_slice(&self.seed);
        out.push(self.bit as u8);
        out.extend_from_slice(&serialize_ringvec(&self.y)?);
        out.extend_from_slice(&self.y_bit.to_bytes());
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        ensure!(
            bytes.len() >= std::mem::size_of::<usize>() + AES_BLOCK_SIZE + 1,
            "Not enough bytes to read LdcfEval header"
        );
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of two and non-zero"
        );
        let mut offset = 0;
        let level = usize::from_le_bytes(
            bytes[offset..offset + std::mem::size_of::<usize>()]
                .try_into()
                .context("Failed to read level")?,
        );
        offset += std::mem::size_of::<usize>();
        let mut seed = [0u8; AES_BLOCK_SIZE];
        seed.copy_from_slice(&bytes[offset..offset + AES_BLOCK_SIZE]);
        offset += AES_BLOCK_SIZE;
        let bit = bytes[offset] != 0;
        offset += 1;
        let (y, used_y) = deserialize_ringvec(&bytes[offset..], modulus)
            .context("Failed to create RingVec from y")?;
        offset += used_y;
        let (y_bit, used_y_bit) = Mod2k::from_bytes(&bytes[offset..], modulus);
        offset += used_y_bit;

        Ok((
            LdcfEval {
                level,
                seed,
                bit,
                y,
                y_bit,
            },
            offset,
        ))
    }

    pub fn y(&self) -> &RingVec {
        &self.y
    }
}

fn gen_layer_data(key: [u8; AES_BLOCK_SIZE], modulus: u128, payload_len: usize) -> Result<LdcfData> {
    ensure!(
        modulus.is_power_of_two(),
        "Modulus must be a power of 2 and greater than 0"
    );
    let num_payload_bits = modulus.ilog2();
    let num_payload_bytes = ((num_payload_bits + 7) / 8) as usize;
    let modulus_mask = modulus - 1;
    FIXED_KEY_STREAM.with(|stream| {
        let mut s = stream.borrow_mut();
        s.set_key(&key);
        let mut out = LdcfData {
            seeds: ([0; AES_BLOCK_SIZE], [0; AES_BLOCK_SIZE]),
            bits: (false, false),
            ys: (
                RingVec::zero_with_len(payload_len, modulus)
                    .context("Failed to create left ringvec")?,
                RingVec::zero_with_len(payload_len, modulus)
                    .context("Failed to create right ringvec")?,
            ),
            y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
        };

        let mut payload_rnd =
            vec![0u8; num_payload_bytes * (payload_len + 1) * 2 + AES_BLOCK_SIZE * 2];
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

        for i in 0..payload_len {
            out.ys.0[i] =
                bytes_to_u128(&payload_rnd[i * num_payload_bytes..(i + 1) * num_payload_bytes])
                    & modulus_mask;
        }
        out.y_bits.0 = Mod2k::new(
            bytes_to_u128(
                &payload_rnd
                    [payload_len * num_payload_bytes..(payload_len + 1) * num_payload_bytes],
            ),
            modulus,
        );

        for i in 0..payload_len {
            out.ys.1[i] = bytes_to_u128(
                &payload_rnd
                    [(i + payload_len + 1) * num_payload_bytes
                        ..(i + payload_len + 2) * num_payload_bytes],
            ) & modulus_mask;
        }
        out.y_bits.1 = Mod2k::new(
            bytes_to_u128(
                &payload_rnd
                    [(2 * payload_len + 1) * num_payload_bytes
                        ..(2 * payload_len + 2) * num_payload_bytes],
            ),
            modulus,
        );
        Ok(out)
    })
}

fn gen_cor_word(
    alpha_bit: bool,
    left: &RingVec,
    right: &RingVec,
    modulus: u128,
    eval: &mut (LdcfEval, LdcfEval),
) -> Result<LdcfCW> {
    ensure!(
        modulus.is_power_of_two(),
        "Modulus must be a power of two and greater than 0"
    );
    ensure!(
        left.len() == right.len(),
        "Left/right RingVec length mismatch: {} vs {}",
        left.len(),
        right.len()
    );
    ensure!(
        left.modulus() == right.modulus(),
        "Left/right RingVec modulus mismatch: {} vs {}",
        left.modulus(),
        right.modulus()
    );
    ensure!(
        left.modulus() == modulus,
        "RingVec modulus ({}) does not match provided modulus ({})",
        left.modulus(),
        modulus
    );
    ensure!(
        eval.0.y.len() == left.len() && eval.1.y.len() == left.len(),
        "Eval payload length does not match RingVec length"
    );
    ensure!(
        eval.0.y.modulus() == modulus && eval.1.y.modulus() == modulus,
        "Eval RingVec modulus does not match provided modulus"
    );
    ensure!(
        eval.0.y_bit.modulus() == modulus && eval.1.y_bit.modulus() == modulus,
        "Eval y_bit modulus does not match provided modulus"
    );

    let payload_len = left.len();
    let data = (
        gen_layer_data(eval.0.seed, modulus, payload_len)?,
        gen_layer_data(eval.1.seed, modulus, payload_len)?,
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

    let mut cw = LdcfCW {
        seed: [0u8; 16],
        seed_bit: (false, false),
        ys: (
            RingVec::zero_with_len(payload_len, modulus)
                .context("Failed to create left ringvec")?,
            RingVec::zero_with_len(payload_len, modulus)
                .context("Failed to create right ringvec")?,
        ),
        y_bits: (Mod2k::zero(modulus), Mod2k::zero(modulus)),
    };

    if !alpha_bit {
        cw.seed = delta_seed.1;
        cw.seed_bit = (delta_bits.0 ^ true, delta_bits.1 ^ false);
        cw.ys = (delta_ys.0 + right.clone(), delta_ys.1 + right.clone());
        cw.y_bits = (
            delta_y_bits.0 + Mod2k::one(modulus),
            delta_y_bits.1 + Mod2k::zero(modulus),
        );
    } else {
        cw.seed = delta_seed.0;
        cw.seed_bit = (delta_bits.0 ^ false, delta_bits.1 ^ true);
        cw.ys = (delta_ys.0 + left.clone(), delta_ys.1 + right.clone());
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
        LdcfEval {
            level: 0,
            seed: new_seed.0,
            bit: new_bits.0,
            y: RingVec::zero_with_len(payload_len, modulus)
                .context("Failed to create left eval y")?,
            y_bit: Mod2k::zero(modulus),
        },
        LdcfEval {
            level: 0,
            seed: new_seed.1,
            bit: new_bits.1,
            y: RingVec::zero_with_len(payload_len, modulus)
                .context("Failed to create right eval y")?,
            y_bit: Mod2k::zero(modulus),
        },
    );

    Ok(cw)
}

/// All-prefix DPF implementation.
impl LdcfKey {
    // Need alpha < beta
    pub fn gen_ldcf_key(
        alpha_bits: &[bool],
        a: &RingVec,
        b: &RingVec,
        modulus: u128,
    ) -> Result<(LdcfKey, LdcfKey)> {
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and non-zero"
        );
        ensure!(!alpha_bits.is_empty(), "alpha_bits cannot be empty");
        ensure!(
            a.len() == b.len(),
            "Size mismatch between payloads a ({}) and b ({})",
            a.len(),
            b.len()
        );
        ensure!(
            a.modulus() == b.modulus(),
            "Modulus mismatch between payloads a ({}) and b ({})",
            a.modulus(),
            b.modulus()
        );
        ensure!(
            a.modulus() == modulus,
            "Payload modulus ({}) must match provided modulus ({})",
            a.modulus(),
            modulus
        );

        let payload_len = a.len();
        let u = alpha_bits.len();
        let mut payload_left = vec![a.clone() - b.clone(); u];
        payload_left[0] = a.clone();
        let zero = RingVec::zero_with_len(payload_len, modulus)
            .context("Failed to create zero ringvec")?;
        let mut payload_right = vec![zero.clone(); u];
        payload_right[0] = b.clone();

        let root_seeds = (
            rand::rng().random::<[u8; 16]>(),
            rand::rng().random::<[u8; 16]>(),
        );

        let eval0 = LdcfEval {
            level: 0,
            seed: root_seeds.0,
            bit: true,
            y: RingVec::zero_with_len(payload_len, modulus).context("Failed to create eval0 y")?,
            y_bit: Mod2k::one(modulus),
        };

        let eval1 = LdcfEval {
            level: 0,
            seed: root_seeds.1,
            bit: false,
            y: RingVec::zero_with_len(payload_len, modulus).context("Failed to create eval1 y")?,
            y_bit: Mod2k::zero(modulus),
        };

        let mut eval = (eval0.clone(), eval1.clone());

        let mut cor_words: Vec<LdcfCW> = Vec::new();

        for (i, &alpha_bit) in alpha_bits.iter().enumerate() {
            let cw = gen_cor_word(
                alpha_bit,
                &payload_left[i],
                &payload_right[i],
                modulus,
                &mut eval,
            )?;
            cor_words.push(cw);
        }

        Ok((
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
        ))
    }

    pub fn eval_bit(&self, state: &LdcfEval, modulus: u128, dir: bool) -> Result<LdcfEval> {
        ensure!(
            state.level < self.cor_words.len(),
            "Eval state level {} out of bounds for {} correlation words",
            state.level,
            self.cor_words.len()
        );
        ensure!(
            state.y.modulus() == modulus,
            "State y modulus ({}) does not match provided modulus ({})",
            state.y.modulus(),
            modulus
        );
        ensure!(
            state.y_bit.modulus() == modulus,
            "State y_bit modulus ({}) does not match provided modulus ({})",
            state.y_bit.modulus(),
            modulus
        );
        let payload_len = state.y.len();
        let data: LdcfData = gen_layer_data(state.seed, modulus, payload_len)?;
        let cw = self.cor_words[state.level].clone();
        self.validate_cw(&cw, payload_len, modulus)?;

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

        Ok(LdcfEval {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y: new_y,
            y_bit: new_y_bit,
        })
    }

    pub fn expand_prefix(&self, state: &LdcfEval, modulus: u128) -> Result<(LdcfEval, LdcfEval)> {
        ensure!(
            state.level < self.cor_words.len(),
            "Eval state level {} out of bounds for {} correlation words",
            state.level,
            self.cor_words.len()
        );
        ensure!(
            state.y.modulus() == modulus,
            "State y modulus ({}) does not match provided modulus ({})",
            state.y.modulus(),
            modulus
        );
        ensure!(
            state.y_bit.modulus() == modulus,
            "State y_bit modulus ({}) does not match provided modulus ({})",
            state.y_bit.modulus(),
            modulus
        );
        let payload_len = state.y.len();
        let data: LdcfData = gen_layer_data(state.seed, modulus, payload_len)?;
        let cw = self.cor_words[state.level].clone();
        self.validate_cw(&cw, payload_len, modulus)?;

        let seeds = (
            xor_u8_16(&data.seeds.0, &and_bit::<16>(cw.seed, state.bit)),
            xor_u8_16(&data.seeds.1, &and_bit::<16>(cw.seed, state.bit)),
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

        Ok((
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
        ))
    }

    pub fn init_eval(&self, modulus: u128) -> Result<LdcfEval> {
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and non-zero"
        );
        let payload_len = self.payload_len()?;
        if let Some(first_cw) = self.cor_words.first() {
            self.validate_cw(first_cw, payload_len, modulus)?;
        }
        if !self.key_idx {
            Ok(LdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: true,
                y: RingVec::zero_with_len(payload_len, modulus)
                    .context("Failed to create init y")?,
                y_bit: Mod2k::one(modulus),
            })
        } else {
            Ok(LdcfEval {
                level: 0,
                seed: self.root_seed.clone(),
                bit: false,
                y: RingVec::zero_with_len(payload_len, modulus)
                    .context("Failed to create init y")?,
                y_bit: Mod2k::zero(modulus),
            })
        }
    }

    pub fn eval_ldcf(&self, prefix: &[bool], modulus: u128) -> Result<RingVec> {
        ensure!(
            prefix.len() <= self.domain_size(),
            "Prefix length {} exceeds domain size {}",
            prefix.len(),
            self.domain_size()
        );
        ensure!(!prefix.is_empty(), "Prefix bits cannot be empty");
        let mut state = self.init_eval(modulus)?;

        for &bit in prefix {
            state = self.eval_bit(&state, modulus, bit)?;
        }

        Ok(state.y)
    }

    pub fn domain_size(&self) -> usize {
        self.cor_words.len()
    }

    pub fn full_domain_eval(&self, modulus: u128, height: usize) -> Result<Vec<RingVec>> {
        let mut states = vec![self.init_eval(modulus)?; 1 << height];
        for level in 0..height {
            for i in (0..(1 << level)).rev() {
                (states[i << 1], states[i << 1 | 1]) =
                    self.expand_prefix(&states[i], modulus)?;
            }
        }
        let results = states.iter().map(|s| s.y.clone()).collect();
        states.clear();
        Ok(results)
    }

    pub fn full_domain_incremental_eval(
        &self,
        modulus: u128,
        height: usize,
    ) -> Result<Vec<Vec<RingVec>>> {
        let mut states = vec![self.init_eval(modulus)?; 1 << height];
        let mut results: Vec<Vec<RingVec>> = Vec::new();
        results.push(states[..1].iter().map(|x| x.y.clone()).collect());
        for level in 0..height {
            for i in (0..(1 << level)).rev() {
                (states[i << 1], states[i << 1 | 1]) =
                    self.expand_prefix(&states[i], modulus)?;
            }
            results.push(
                states[..(1 << (level + 1))]
                    .iter()
                    .map(|x| x.y.clone())
                    .collect(),
            );
        }
        states.clear();
        Ok(results)
    }

    fn validate_cw(&self, cw: &LdcfCW, payload_len: usize, modulus: u128) -> Result<()> {
        ensure!(
            cw.ys.0.len() == payload_len && cw.ys.1.len() == payload_len,
            "Correlation word payload length mismatch: expected {}, got {} and {}",
            payload_len,
            cw.ys.0.len(),
            cw.ys.1.len()
        );
        ensure!(
            cw.ys.0.modulus() == modulus && cw.ys.1.modulus() == modulus,
            "Correlation word RingVec modulus mismatch"
        );
        ensure!(
            cw.y_bits.0.modulus() == modulus && cw.y_bits.1.modulus() == modulus,
            "Correlation word y_bits modulus mismatch"
        );
        Ok(())
    }

    fn payload_len(&self) -> Result<usize> {
        let len = self
            .cor_words
            .first()
            .map(|cw| cw.ys.0.len())
            .ok_or_else(|| anyhow!("LdcfKey has no correlation words to determine payload length"))?;
        ensure!(
            self.cor_words
                .iter()
                .all(|cw| cw.ys.0.len() == len && cw.ys.1.len() == len),
            "Inconsistent payload lengths in correlation words"
        );
        Ok(len)
    }
}
