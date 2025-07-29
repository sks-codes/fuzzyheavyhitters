use crate::fss::left_interval::LIntervalFSSKey;
use crate::fss::right_interval::{self, RIntervalFSSKey};
use crate::data_structures::payload::RingVec;

const BINOMIAL_COEFFICIENTS: [[u128; 6]; 6] = [
    [1, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0],
    [1, 2, 1, 0, 0, 0],
    [1, 3, 3, 1, 0, 0],
    [1, 4, 6, 4, 1, 0],
    [1, 5, 10, 10, 5, 1],
];

// N here is P+1, where P is the distance Lp norm
#[derive(Clone, Debug)]
pub struct DistanceFSSKey<const N: usize> {
    left_fss: LIntervalFSSKey<N>,
    right_fss: RIntervalFSSKey<N>,
}

impl<const N: usize> DistanceFSSKey<N> {
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
        out_payload[0] = max_distance;
        // Creating FSS for the range [left, x] first
        let (left_key0, left_key1) = LIntervalFSSKey::<N>::gen_LIntervalFSSKey(
            left_bits,
            x_bits,
            out_payload.clone(),
            left_payload,
            zero_payload.clone(),
            modulus,
        );

        let (right_key0, right_key1) = RIntervalFSSKey::<N>::gen_RIntervalFSSKey(
            x_bits,
            right_bits,
            zero_payload,
            right_payload,
            out_payload,
            modulus,
        );

        (
            Self {
                left_fss: left_key0,
                right_fss: right_key0,
            },
            Self {
                left_fss: left_key1,
                right_fss: right_key1,
            },
        )
    }

    pub fn eval_distance_fss(&self, x_bits: &[bool], input_len: usize, modulus: u128) -> u128 {
        let left_eval = self.left_fss.eval_lintervalFSS(x_bits, modulus);
        let right_eval = self.right_fss.eval_rintervalFSS(x_bits, modulus);
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