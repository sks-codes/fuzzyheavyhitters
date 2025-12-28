use rand::Rng;
use std::ops::{Add, Mul, Sub};

/// Dynamic ring vector whose element bit-width (power-of-two modulus) is only
/// known at runtime. Chooses the smallest native integer type able to contain
/// the values for better space & cache efficiency versus always using u128.
/// Modulus must be 2^bits.
#[derive(Clone, Debug)]
pub struct DynRingVec {
    bits: u8,      // number of significant bits (1..=128)
    modulus: u128, // 2^bits
    mask: u128,    // modulus - 1 (all used bits set)
    storage: RingStorage,
}

#[derive(Clone, Debug)]
enum RingStorage {
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    U128(Vec<u128>),
}

impl DynRingVec {
    /// Create a zero vector of length `len` with given modulus (power of two).
    pub fn zero(len: usize, modulus: u128) -> Self {
        let bits = Self::modulus_to_bits(modulus);
        let storage = match bits {
            0..=8 => RingStorage::U8(vec![0; len]),
            9..=16 => RingStorage::U16(vec![0; len]),
            17..=32 => RingStorage::U32(vec![0; len]),
            33..=64 => RingStorage::U64(vec![0; len]),
            _ => RingStorage::U128(vec![0; len]),
        };
        Self {
            bits: bits as u8,
            modulus,
            mask: modulus - 1,
            storage,
        }
    }

    /// Create from slice of u128 (values will be reduced modulo 2^bits)
    pub fn from_slice(values: &[u128], modulus: u128) -> Self {
        let mut v = Self::zero(values.len(), modulus);
        for (i, &val) in values.iter().enumerate() {
            v.set(i, val);
        }
        v
    }

    /// Random vector of given length.
    pub fn random(len: usize, modulus: u128) -> Self {
        let mut v = Self::zero(len, modulus);
        let mask = v.mask;
        match &mut v.storage {
            RingStorage::U8(data) => {
                for x in data.iter_mut() {
                    *x = rand::rng().random::<u8>() & (mask as u8);
                }
            }
            RingStorage::U16(data) => {
                for x in data.iter_mut() {
                    *x = rand::rng().random::<u16>() & (mask as u16);
                }
            }
            RingStorage::U32(data) => {
                for x in data.iter_mut() {
                    *x = rand::rng().random::<u32>() & (mask as u32);
                }
            }
            RingStorage::U64(data) => {
                for x in data.iter_mut() {
                    *x = rand::rng().random::<u64>() & (mask as u64);
                }
            }
            RingStorage::U128(data) => {
                for x in data.iter_mut() {
                    *x = rand::rng().random::<u128>() & mask;
                }
            }
        }
        v
    }

    #[inline]
    fn modulus_to_bits(modulus: u128) -> usize {
        assert!(
            modulus > 0 && (modulus & (modulus - 1)) == 0,
            "modulus must be power of two"
        );
        if modulus == 1 {
            1
        } else {
            (128 - (modulus - 1).leading_zeros()) as usize
        }
    }

    pub fn bits(&self) -> usize {
        self.bits as usize
    }
    pub fn len(&self) -> usize {
        match &self.storage {
            RingStorage::U8(v) => v.len(),
            RingStorage::U16(v) => v.len(),
            RingStorage::U32(v) => v.len(),
            RingStorage::U64(v) => v.len(),
            RingStorage::U128(v) => v.len(),
        }
    }
    pub fn modulus(&self) -> u128 {
        self.modulus
    }

    #[inline]
    pub fn get(&self, idx: usize) -> u128 {
        match &self.storage {
            RingStorage::U8(v) => v[idx] as u128,
            RingStorage::U16(v) => v[idx] as u128,
            RingStorage::U32(v) => v[idx] as u128,
            RingStorage::U64(v) => v[idx] as u128,
            RingStorage::U128(v) => v[idx],
        }
    }

    #[inline]
    pub fn set(&mut self, idx: usize, value: u128) {
        let masked = value & self.mask;
        match &mut self.storage {
            RingStorage::U8(v) => v[idx] = masked as u8,
            RingStorage::U16(v) => v[idx] = masked as u16,
            RingStorage::U32(v) => v[idx] = masked as u32,
            RingStorage::U64(v) => v[idx] = masked as u64,
            RingStorage::U128(v) => v[idx] = masked,
        }
    }

    // Internal helper: apply binary op producing new RingStorage
    fn bin_op(&self, other: &Self, op: BinOp) -> Self {
        debug_assert_eq!(self.modulus, other.modulus, "modulus mismatch");
        debug_assert_eq!(self.bits, other.bits, "bit width mismatch");
        let mask = self.mask;
        let storage = match (&self.storage, &other.storage) {
            (RingStorage::U8(a), RingStorage::U8(b)) => {
                let mut out = a.clone();
                let m = mask as u8;
                for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                    *o = match op {
                        BinOp::Add => x.wrapping_add(y) & m,
                        BinOp::Sub => x.wrapping_sub(y) & m,
                        BinOp::Mul => (((x as u16) * (y as u16)) & m as u16) as u8,
                    };
                }
                RingStorage::U8(out)
            }
            (RingStorage::U16(a), RingStorage::U16(b)) => {
                let mut out = a.clone();
                let m = mask as u16;
                for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                    *o = match op {
                        BinOp::Add => x.wrapping_add(y) & m,
                        BinOp::Sub => x.wrapping_sub(y) & m,
                        BinOp::Mul => (((x as u32) * (y as u32)) & m as u32) as u16,
                    };
                }
                RingStorage::U16(out)
            }
            (RingStorage::U32(a), RingStorage::U32(b)) => {
                let mut out = a.clone();
                let m = mask as u32;
                for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                    *o = match op {
                        BinOp::Add => x.wrapping_add(y) & m,
                        BinOp::Sub => x.wrapping_sub(y) & m,
                        BinOp::Mul => (((x as u64) * (y as u64)) & m as u64) as u32,
                    };
                }
                RingStorage::U32(out)
            }
            (RingStorage::U64(a), RingStorage::U64(b)) => {
                let mut out = a.clone();
                let m = mask as u64;
                for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                    *o = match op {
                        BinOp::Add => x.wrapping_add(y) & m,
                        BinOp::Sub => x.wrapping_sub(y) & m,
                        BinOp::Mul => (((x as u128) * (y as u128)) & m as u128) as u64,
                    };
                }
                RingStorage::U64(out)
            }
            (RingStorage::U128(a), RingStorage::U128(b)) => {
                let mut out = a.clone();
                let m = mask;
                for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                    *o = match op {
                        BinOp::Add => x.wrapping_add(y) & m,
                        BinOp::Sub => x.wrapping_sub(y) & m,
                        BinOp::Mul => x.wrapping_mul(y) & m,
                    };
                }
                RingStorage::U128(out)
            }
            _ => unreachable!("variant mismatch"),
        };
        Self {
            bits: self.bits,
            modulus: self.modulus,
            mask: self.mask,
            storage,
        }
    }

    /// Element-wise addition / subtraction / multiplication
    pub fn add(&self, other: &Self) -> Self {
        self.bin_op(other, BinOp::Add)
    }
    pub fn sub(&self, other: &Self) -> Self {
        self.bin_op(other, BinOp::Sub)
    }
    pub fn mul(&self, other: &Self) -> Self {
        self.bin_op(other, BinOp::Mul)
    }

    /// Compress to bit-packed bytes (same strategy as original RingVec)
    pub fn to_bytes(&self) -> Vec<u8> {
        let bits_per_element = self.bits as usize;
        if bits_per_element == 0 {
            return Vec::new();
        }
        let len = self.len();
        let total_bits = len * bits_per_element;
        let padded_bits = (total_bits + 7) & !7;
        let total_bytes = padded_bits / 8;
        let mut out = vec![0u8; total_bytes];
        for i in 0..len {
            let val = self.get(i);
            let bit_start = i * bits_per_element;
            for bit_pos in 0..bits_per_element {
                if (val >> bit_pos) & 1 == 1 {
                    let global = bit_start + bit_pos;
                    let byte_idx = global / 8;
                    let bit_in = global % 8;
                    out[byte_idx] |= 1u8 << bit_in;
                }
            }
        }
        out
    }

    /// Decompress from bytes; returns (DynRingVec, bytes_consumed)
    pub fn from_bytes(bytes: &[u8], len: usize, modulus: u128) -> Result<(Self, usize), String> {
        let bits = Self::modulus_to_bits(modulus);
        let total_bits = len * bits;
        let padded_bits = (total_bits + 7) & !7;
        let bytes_needed = padded_bits / 8;
        if bytes.len() < bytes_needed {
            return Err(format!(
                "not enough bytes: need {}, have {}",
                bytes_needed,
                bytes.len()
            ));
        }
        let mut v = Self::zero(len, modulus);
        for i in 0..len {
            let bit_start = i * bits;
            let mut value = 0u128;
            for bit_pos in 0..bits {
                let global = bit_start + bit_pos;
                let byte_idx = global / 8;
                let bit_in = global % 8;
                if (bytes[byte_idx] >> bit_in) & 1 == 1 {
                    value |= 1u128 << bit_pos;
                }
            }
            v.set(i, value);
        }
        Ok((v, bytes_needed))
    }
}

#[derive(Copy, Clone)]
enum BinOp {
    Add,
    Sub,
    Mul,
}

// Implement trait ops producing a new owned vector (like original RingVec)
impl Add for DynRingVec {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        DynRingVec::add(&self, &rhs)
    }
}
impl Sub for DynRingVec {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        DynRingVec::sub(&self, &rhs)
    }
}
impl Mul for DynRingVec {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        DynRingVec::mul(&self, &rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_basic_ops_small_bits() {
        for bits in [1usize, 5, 8, 9, 15, 16, 17, 31, 32] {
            // exercise each width tier
            let modulus = 1u128 << bits;
            let len = 7;
            let a = DynRingVec::random(len, modulus);
            let b = DynRingVec::random(len, modulus);
            let c_add = a.clone() + b.clone();
            let c_sub = a.clone() - b.clone();
            let c_mul = a.clone() * b.clone();
            assert_eq!(c_add.len(), len);
            assert_eq!(c_sub.len(), len);
            assert_eq!(c_mul.len(), len);
            // Spot-check bit width containment
            for i in 0..len {
                assert!(c_add.get(i) < modulus);
            }
        }
    }
    #[test]
    fn test_roundtrip_bytes() {
        let bits = 14usize;
        let modulus = 1u128 << bits;
        let len = 25;
        let v = DynRingVec::random(len, modulus);
        let bytes = v.to_bytes();
        let (dec, used) = DynRingVec::from_bytes(&bytes, len, modulus).unwrap();
        assert_eq!(used, bytes.len());
        for i in 0..len {
            assert_eq!(v.get(i), dec.get(i));
        }
    }
}
