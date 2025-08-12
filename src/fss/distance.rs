use crate::fss::ldcf::LdcfKey;
use crate::fss::rdcf::RdcfKey;
use crate::data_structures::payload::RingVec;
use std::convert::TryInto;

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

        let (key00, key10) = LdcfKey::gen_LdcfKey(
            &left_bits,
            &(out_payload - left_payload),
            &zero_payload,
            modulus,
        );

        let (key01, key11) = LdcfKey::gen_LdcfKey(
            &x_bits,
            &left_payload,
            &zero_payload,
            modulus,
        );

        let (key02, key12) = RdcfKey::gen_RdcfKey(
            &x_bits,
            &zero_payload,
            &right_payload,
            modulus,
        );

        let (key03, key13) = RdcfKey::gen_RdcfKey(
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

    pub fn eval_distance_fss(&self, x_bits: &[bool], input_len: usize, modulus: u128) -> u128 {
        let left_eval = self.left_fss.0.eval_ldcf(x_bits, modulus) + self.left_fss.1.eval_ldcf(x_bits, modulus);
        let right_eval = self.right_fss.0.eval_rdcf(x_bits, modulus) + self.right_fss.1.eval_rdcf(x_bits, modulus);
        let mut x = 0;
        for i in 0..x_bits.len() {
            if x_bits[i] {
                x = (x << 1) ^ 1
            } else {
                x = x << 1;
            }
        }
        let mut left_x = x.clone();
        for i in x_bits.len()..input_len {
            left_x = (left_x << 1) ^ 1;
        }
        let mut right_x = x.clone();
        for i in x_bits.len()..input_len {
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
}