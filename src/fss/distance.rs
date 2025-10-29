use crate::fss::ldcf::{LdcfKey, LdcfEval};
use crate::fss::rdcf::{RdcfKey, RdcfEval};
use crate::data_structures::ringvec::RingVec;

const BINOMIAL_COEFFICIENTS: [[u128; 6]; 6] = [
    [1, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0],
    [1, 2, 1, 0, 0, 0],
    [1, 3, 3, 1, 0, 0],
    [1, 4, 6, 4, 1, 0],
    [1, 5, 10, 10, 5, 1],
];

// N here is P+1, where P is the distance Lp norm
#[derive(Clone, Debug, PartialEq)]
pub struct DistanceFSSKey<const N: usize> {
    left_fss: (LdcfKey<N>, LdcfKey<N>),
    right_fss: (RdcfKey<N>, RdcfKey<N>),
}

#[derive(Clone, Debug)]
pub struct DistanceFSSEval<const N: usize> {
    left_eval: (LdcfEval<N>, LdcfEval<N>),
    right_eval: (RdcfEval<N>, RdcfEval<N>),
    pub result: u128,
}

impl<const N: usize> DistanceFSSEval<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.left_eval.0.to_bytes());
        out.extend_from_slice(&self.left_eval.1.to_bytes());
        out.extend_from_slice(&self.right_eval.0.to_bytes());
        out.extend_from_slice(&self.right_eval.1.to_bytes());
        let modulus = self.left_eval.0.y().modulus();
        let num_bits = 128 - modulus.leading_zeros();
        out.extend_from_slice(&self.result.to_le_bytes()[..((num_bits + 7) / 8) as usize]);
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let (left_eval0, used_left0) = LdcfEval::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_left0;
        let (left_eval1, used_left1) = LdcfEval::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_left1;
        let (right_eval0, used_right0) = RdcfEval::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_right0;
        let (right_eval1, used_right1) = RdcfEval::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_right1;
        let num_bits = 128 - modulus.leading_zeros();
        let result_bytes = (num_bits + 7) / 8;
        if bytes.len() < offset + result_bytes as usize {
            panic!("Insufficient bytes for DistanceFSSEval result");
        }
        let mut result_array = [0u8; 16];
        result_array[..result_bytes as usize].copy_from_slice(&bytes[offset..offset + result_bytes as usize]);
        let result = u128::from_le_bytes(result_array);
        offset += result_bytes as usize;
        (
            Self {
                left_eval: (left_eval0, left_eval1),
                right_eval: (right_eval0, right_eval1),
                result,
            },
            offset
        )
    }
}

impl<const N: usize> DistanceFSSKey<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.left_fss.0.to_bytes());
        out.extend_from_slice(&self.left_fss.1.to_bytes());
        out.extend_from_slice(&self.right_fss.0.to_bytes());
        out.extend_from_slice(&self.right_fss.1.to_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let (left_fss0, used_left0) = LdcfKey::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_left0;
        let (left_fss1, used_left1) = LdcfKey::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_left1;
        let (right_fss0, used_right0) = RdcfKey::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_right0;
        let (right_fss1, used_right1) = RdcfKey::<N>::from_bytes(&bytes[offset..], modulus);
        offset += used_right1;
        (
            Self {
                left_fss: (left_fss0, left_fss1),
                right_fss: (right_fss0, right_fss1),
            },
            offset
        )
    }
    pub fn gen_distance_fss_key(
        x: u128,
        x_bits: &[bool],
        left_bits: &[bool],
        right_bits: &[bool],
        max_distance: u128,
        modulus: u128,
    ) -> (Self, Self) {
        assert!(N <= 6, "N must be less than or equal to 6 for distance FSS key generation");
        let mut x_powers = [0u128; N];
        x_powers[0] = 1;
        for i in 1..N {
            x_powers[i] = (x_powers[i-1] * x) % modulus;
        }
        for i in 0..N {
            x_powers[i] = (x_powers[i] * BINOMIAL_COEFFICIENTS[N-1][i]) % modulus;
        }
        let zero_payload = RingVec::<N>::new([0; N], modulus);
        for i in 0..N {
            if (i & 1) == 1 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
        }
        let right_payload = RingVec::<N>::new(x_powers, modulus);
        for i in 0..N {
            if (i & 1) == 1 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
            if ((N - i) & 1) == 0 {
                x_powers[i] = (modulus - x_powers[i]) % modulus;
            }
        }
        let left_payload = RingVec::<N>::new(x_powers, modulus);
        let mut out_payload = RingVec::<N>::zero(modulus);
        out_payload[N-1] = max_distance;

        let (key00, key10) = LdcfKey::gen_ldcf_key(
            &left_bits,
            &(out_payload - left_payload),
            &zero_payload,
            modulus,
        );

        let (key01, key11) = LdcfKey::gen_ldcf_key(
            &x_bits,
            &left_payload,
            &zero_payload,
            modulus,
        );

        let (key02, key12) = RdcfKey::gen_rdcf_key(
            &x_bits,
            &zero_payload,
            &right_payload,
            modulus,
        );

        let (key03, key13) = RdcfKey::gen_rdcf_key(
            &right_bits,
            &zero_payload,
            &(out_payload - right_payload),
            modulus,
        );

        (
            Self {
                left_fss: (key00, key01),
                right_fss: (key02, key03),
            },
            Self {
                left_fss: (key10, key11),
                right_fss: (key12, key13),
            },
        )
    }

    pub fn expand_prefix(&self, prefix: &[bool], state: &DistanceFSSEval<N>, input_len: usize, modulus: u128) -> (DistanceFSSEval<N>, DistanceFSSEval<N>) {
        let left_eval0 = self.left_fss.0.expand_prefix(&state.left_eval.0, modulus);
        let left_eval1 = self.left_fss.1.expand_prefix(&state.left_eval.1, modulus);
        let right_eval0 = self.right_fss.0.expand_prefix(&state.right_eval.0, modulus);
        let right_eval1 = self.right_fss.1.expand_prefix(&state.right_eval.1, modulus);

        let left_eval = (
            left_eval0.0.y() + left_eval1.0.y(),
            left_eval0.1.y() + left_eval1.1.y(),
        );
        let right_eval = (
            right_eval0.0.y() + right_eval1.0.y(),
            right_eval0.1.y() + right_eval1.1.y(),
        );
        let mut x = 0;
        for i in 0..prefix.len() {
            if prefix[i] {
                x = (x << 1) ^ 1;
            } else {
                x = x << 1;
            }
        }
        let x0 = x << 1;
        let mut left_x0 = x0.clone();
        for _ in prefix.len()+1..input_len {
            left_x0 = (left_x0 << 1) ^ 1;
        }
        let mut right_x0 = x0.clone();
        for _ in prefix.len()+1..input_len {
            right_x0 = right_x0 << 1;
        }

        let mut result0 = 0u128;
        let modulus_mask = modulus - 1;
        let mut pow_left_x0 = 1u128;
        for i in 0..N {
            result0 = (result0 + pow_left_x0 * left_eval.0[N-1-i]) & modulus_mask;
            pow_left_x0 = (pow_left_x0 * left_x0) & modulus_mask;
        }
        let mut pow_right_x0 = 1u128;
        for i in 0..N {
            result0 = (result0 + pow_right_x0 * right_eval.0[N-1-i]) & modulus_mask;
            pow_right_x0 = (pow_right_x0 * right_x0) & modulus_mask;
        }

        let x1 = (x << 1) | 1;
        let mut left_x1 = x1.clone();
        for _ in prefix.len()+1..input_len {
            left_x1 = (left_x1 << 1) ^ 1;
        }
        let mut right_x1 = x1.clone();
        for _ in prefix.len()+1..input_len {
            right_x1 = right_x1 << 1;
        }
        let mut result1 = 0u128;
        let modulus_mask = modulus - 1;
        let mut pow_left_x1 = 1u128;
        for i in 0..N {
            result1 = (result1 + pow_left_x1 * left_eval.1[N-1-i]) & modulus_mask;
            pow_left_x1 = (pow_left_x1 * left_x1) & modulus_mask;
        }
        let mut pow_right_x1 = 1u128;
        for i in 0..N {
            result1 = (result1 + pow_right_x1 * right_eval.1[N-1-i]) & modulus_mask;
            pow_right_x1 = (pow_right_x1 * right_x1) & modulus_mask;
        }

        (
            DistanceFSSEval {
                left_eval: (left_eval0.0, left_eval1.0),
                right_eval: (right_eval0.0, right_eval1.0),
                result: result0,
            },
            DistanceFSSEval {
                left_eval: (left_eval0.1, left_eval1.1),
                right_eval: (right_eval0.1, right_eval1.1),
                result: result1,
            },
        )
    }

    pub fn eval_distance_fss(&self, x_bits: &[bool], input_len: usize, modulus: u128) -> u128 {
        let left_eval = self.left_fss.0.eval_ldcf(x_bits, modulus) + self.left_fss.1.eval_ldcf(x_bits, modulus);
        let right_eval = self.right_fss.0.eval_rdcf(x_bits, modulus) + self.right_fss.1.eval_rdcf(x_bits, modulus);
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
        for i in 0..N {
            result = (result + pow_left_x * left_eval[N-1-i]) & modulus_mask;
            pow_left_x = (pow_left_x * left_x) & modulus_mask;
        }
        let mut pow_right_x = 1u128;
        for i in 0..N {
            result = (result + pow_right_x * right_eval[N-1-i]) & modulus_mask;
            pow_right_x = (pow_right_x * right_x) & modulus_mask;
        }
        result
    }

    pub fn init_eval(&self, modulus: u128) -> DistanceFSSEval<N> {
        DistanceFSSEval {
            left_eval: (self.left_fss.0.eval_init(modulus), self.left_fss.1.eval_init(modulus)),
            right_eval: (self.right_fss.0.eval_init(modulus), self.right_fss.1.eval_init(modulus)),
            result: 0u128,
        }
    }
}