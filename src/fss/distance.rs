use std::convert::TryInto;

use crate::{
    data_structures::{
        ringvec::RingVec,
        mod2k::Mod2k,
    },
    fss::{
        ldcf::{LdcfEval, LdcfKey},
        rdcf::{RdcfEval, RdcfKey}
    },
};
use anyhow::{ensure, Result};

pub(crate) const BINOMIAL_COEFFICIENTS: [[u128; 6]; 6] = [
    [1, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0],
    [1, 2, 1, 0, 0, 0],
    [1, 3, 3, 1, 0, 0],
    [1, 4, 6, 4, 1, 0],
    [1, 5, 10, 10, 5, 1],
];

// N here is P+1, where P is the distance Lp norm
#[derive(Clone, Debug, PartialEq)]
pub struct DistanceFSSKey {
    p: usize,
    left_fss: (LdcfKey, LdcfKey),
    right_fss: (RdcfKey, RdcfKey),
}

#[derive(Clone, Debug)]
pub struct DistanceFSSEval {
    left_eval: (LdcfEval, LdcfEval),
    right_eval: (RdcfEval, RdcfEval),
}

impl DistanceFSSEval {
    pub fn payload_len(&self) -> usize {
        self.left_eval.0.y().len()
    }

    pub fn degree(&self) -> usize {
        self.payload_len().saturating_sub(1)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.left_eval.0.to_bytes()?);
        out.extend_from_slice(&self.left_eval.1.to_bytes()?);
        out.extend_from_slice(&self.right_eval.0.to_bytes()?);
        out.extend_from_slice(&self.right_eval.1.to_bytes()?);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        let mut offset = 0;
        let (left_eval0, used_left0) = LdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += used_left0;
        let (left_eval1, used_left1) = LdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += used_left1;
        let (right_eval0, used_right0) = RdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += used_right0;
        let (right_eval1, used_right1) = RdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += used_right1;
        Ok((
            Self {
                left_eval: (left_eval0, left_eval1),
                right_eval: (right_eval0, right_eval1),
            },
            offset,
        ))
    }

    pub fn eval(&self, prefix: &[bool], input_len: usize, modulus: u128, p: usize) -> Result<u128> {
        let left_eval = self.left_eval.0.y() + self.left_eval.1.y();
        let right_eval = self.right_eval.0.y() + self.right_eval.1.y();

        ensure!(left_eval.len() == p+1, 
            "Size mismatch for left_eval of distance fss, left_eval.len() = {}, p+1 = {}", left_eval.len(), p+1);
        ensure!(right_eval.len() == p+1, 
            "Size mismatch for right_eval of distance fss, right_eval.len() = {}, p+1 = {}", right_eval.len(), p+1);

        let mut x = 0;
        for i in 0..prefix.len() {
            if prefix[i] {
                x = (x << 1) ^ 1;
            } else {
                x = x << 1;
            }
        }
        let mut left_x = x.clone();
        for _ in prefix.len()..input_len {
            left_x = (left_x << 1) ^ 1;
        }
        let mut right_x = x.clone();
        for _ in prefix.len()..input_len {
            right_x = right_x << 1;
        }

        let mut result = 0u128;
        let modulus_mask = modulus - 1;
        let mut pow_left_x = 1u128;
        for i in 0..p+1 {
            result = (result + pow_left_x * left_eval[p - i]) & modulus_mask;
            pow_left_x = (pow_left_x * left_x) & modulus_mask;
        }
        let mut pow_right_x = 1u128;
        for i in 0..p+1 {
            result = (result + pow_right_x * right_eval[p - i]) & modulus_mask;
            pow_right_x = (pow_right_x * right_x) & modulus_mask;
        }

        Ok(result)
    }
}

impl DistanceFSSKey {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.p as u32).to_ne_bytes());
        out.extend_from_slice(&self.left_fss.0.to_bytes()?);
        out.extend_from_slice(&self.left_fss.1.to_bytes()?);
        out.extend_from_slice(&self.right_fss.0.to_bytes()?);
        out.extend_from_slice(&self.right_fss.1.to_bytes()?);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        let mut offset = 0;
        let p = u32::from_ne_bytes(bytes[offset..offset+4].try_into()?) as usize;
        offset += 4;
        let (left_fss0, used_left0) = LdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += used_left0;
        let (left_fss1, used_left1) = LdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += used_left1;
        let (right_fss0, used_right0) = RdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += used_right0;
        let (right_fss1, used_right1) = RdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += used_right1;
        Ok((
            Self {
                p: p,
                left_fss: (left_fss0, left_fss1),
                right_fss: (right_fss0, right_fss1),
            },
            offset,
        ))
    }
    pub fn gen_distance_fss_key(
        x: u128,
        x_bits: &[bool],
        left_bits: &[bool],
        right_bits: &[bool],
        delta: u128,
        p: usize,
        modulus: u128,
    ) -> Result<(Self, Self)> {
        let delta_mod2k = Mod2k::new(delta, modulus);
        let max_distance_mod2k = delta_mod2k.pow(p as u128) + 1;
        let max_distance = max_distance_mod2k.val();

        let mut x_powers = vec![0u128; p+1];
        x_powers[0] = 1;
        for i in 1..p+1 {
            x_powers[i] = (x_powers[i - 1] * x) % modulus;
        }
        for i in 0..p+1 {
            x_powers[i] = (x_powers[i] * BINOMIAL_COEFFICIENTS[p][i]) % modulus;
        }
        let zero_payload =
            RingVec::zero_with_len(p+1, modulus).expect("Failed to create zero payload");
        for i in 0..p+1 {
            if (i & 1) == 1 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
        }
        let right_payload =
            RingVec::new(x_powers.to_vec(), modulus).expect("Failed to create right payload");
        for i in 0..p+1 {
            if (i & 1) == 1 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
            if ((p+1 - i) & 1) == 0 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
        }
        let left_payload =
            RingVec::new(x_powers.to_vec(), modulus).expect("Failed to create left payload");
        let mut out_payload =
            RingVec::zero_with_len(p+1, modulus).expect("Failed to create output payload");
        out_payload[p] = max_distance;

        let out_minus_left = out_payload.clone() - left_payload.clone();
        let (key00, key10) =
            LdcfKey::gen_ldcf_key(&left_bits, &out_minus_left, &zero_payload, modulus)?;

        let (key01, key11) = LdcfKey::gen_ldcf_key(&x_bits, &left_payload, &zero_payload, modulus)?;

        let (key02, key12) =
            RdcfKey::gen_rdcf_key(&x_bits, &zero_payload, &right_payload, modulus)?;

        let out_minus_right = out_payload.clone() - right_payload.clone();
        let (key03, key13) =
            RdcfKey::gen_rdcf_key(&right_bits, &zero_payload, &out_minus_right, modulus)?;

        Ok((
            Self {
                p: p,
                left_fss: (key00, key01),
                right_fss: (key02, key03),
            },
            Self {
                p: p,
                left_fss: (key10, key11),
                right_fss: (key12, key13),
            },
        ))
    }

    pub fn expand_prefix(
        &self,
        state: &DistanceFSSEval,
        modulus: u128,
    ) -> Result<(DistanceFSSEval, DistanceFSSEval)> {
        let left_eval0 = self.left_fss.0.expand_prefix(&state.left_eval.0, modulus)?;
        let left_eval1 = self.left_fss.1.expand_prefix(&state.left_eval.1, modulus)?;
        let right_eval0 = self.right_fss.0.expand_prefix(&state.right_eval.0, modulus)?;
        let right_eval1 = self.right_fss.1.expand_prefix(&state.right_eval.1, modulus)?;


        Ok((
            DistanceFSSEval {
                left_eval: (left_eval0.0, left_eval1.0),
                right_eval: (right_eval0.0, right_eval1.0),
            },
            DistanceFSSEval {
                left_eval: (left_eval0.1, left_eval1.1),
                right_eval: (right_eval0.1, right_eval1.1),
            },
        ))
    }

    pub fn eval_distance_fss(&self, x_bits: &[bool], input_len: usize, modulus: u128) -> Result<u128> {
        let left_eval = self.left_fss.0.eval_ldcf(x_bits, modulus)?
            + self.left_fss.1.eval_ldcf(x_bits, modulus)?;
        let right_eval = self.right_fss.0.eval_rdcf(x_bits, modulus)?
            + self.right_fss.1.eval_rdcf(x_bits, modulus)?;
        println!("Left Eval: {:?}, Right Eval: {:?}", left_eval, right_eval);
        let mut x = 0;
        for i in 0..x_bits.len() {
            if x_bits[i] {
                x = (x << 1) ^ 1
            } else {
                x = x << 1;
            }
        }
        let mut left_x = x.clone();
        for _ in x_bits.len()..input_len {
            left_x = (left_x << 1) ^ 1;
        }
        let mut right_x = x.clone();
        for _ in x_bits.len()..input_len {
            right_x = right_x << 1;
        }

        let mut result = 0u128;
        let modulus_mask = modulus - 1;
        let mut pow_left_x = 1u128;
        for i in 0..self.p+1 {
            result = (result + pow_left_x * left_eval[self.p - i]) & modulus_mask;
            pow_left_x = (pow_left_x * left_x) & modulus_mask;
        }
        let mut pow_right_x = 1u128;
        for i in 0..self.p+1 {
            result = (result + pow_right_x * right_eval[self.p - i]) & modulus_mask;
            pow_right_x = (pow_right_x * right_x) & modulus_mask;
        }
        Ok(result)
    }

    pub fn init_eval(&self, modulus: u128) -> Result<DistanceFSSEval> {
        Ok(DistanceFSSEval {
            left_eval: (
                self.left_fss.0.init_eval(modulus)?,
                self.left_fss.1.init_eval(modulus)?,
            ),
            right_eval: (
                self.right_fss.0.init_eval(modulus)?,
                self.right_fss.1.init_eval(modulus)?,
            ),
        })
    }

    pub fn left_fss0(&self) -> &LdcfKey {
        &self.left_fss.0
    }

    pub fn left_fss1(&self) -> &LdcfKey {
        &self.left_fss.1
    }

    pub fn right_fss0(&self) -> &RdcfKey {
        &self.right_fss.0
    }

    pub fn right_fss1(&self) -> &RdcfKey {
        &self.right_fss.1
    }

    pub fn p(&self) -> usize {
        self.p
    }
}
