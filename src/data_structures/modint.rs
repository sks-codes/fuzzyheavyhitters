use std::ops::{Add, Sub, Mul};
use std::convert::{TryFrom, TryInto};
use scuttlebutt::Block;
use crate::{Group, Share};
use rand::Rng;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct ModInt {
    pub val: u128,
    modulus: u128,
    modulus_mask: u128, // Stores modulus - 1 for efficient bitwise modulo
}

impl ModInt {
    pub fn new(val: u128, modulus: u128) -> Self {
        if !modulus.is_power_of_two() || modulus == 0 {
            panic!("Modulus must be a power of two and non-zero.");
        }
        let modulus_mask = modulus - 1;
        ModInt {
            val: val & modulus_mask, // Apply modulus initially
            modulus,
            modulus_mask,
        }
    }

    pub fn zero(modulus: u128) -> Self {
        Self::new(0, modulus)
    }

    pub fn one(modulus: u128) -> Self {
        Self::new(1, modulus)
    }

    pub fn val(&self) -> u128 {
        self.val
    }

    pub fn modulus(&self) -> u128 {
        self.modulus
    }

    pub fn random(modulus: u128) -> Self {
        if !modulus.is_power_of_two() || modulus == 0 {
            panic!("Modulus must be a power of two and non-zero.");
        }
        let val = rand::random::<u128>() & (modulus - 1);
        ModInt {
            val,
            modulus,
            modulus_mask: modulus - 1,
        }
    }
}

impl Add for ModInt {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        assert_eq!(self.modulus, rhs.modulus, "Moduli must be equal for addition");
        ModInt {
            val: (self.val + rhs.val) & self.modulus_mask,
            modulus: self.modulus,
            modulus_mask: self.modulus_mask,
        }
    }
}

impl Sub for ModInt {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        assert_eq!(self.modulus, rhs.modulus, "Moduli must be equal for subtraction");
        ModInt {
            val: (self.val + self.modulus - rhs.val) & self.modulus_mask,
            modulus: self.modulus,
            modulus_mask: self.modulus_mask,
        }
    }
}

impl Mul for ModInt {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        assert_eq!(self.modulus, rhs.modulus, "Moduli must be equal for multiplication");
        ModInt {
            val: (self.val * rhs.val) & self.modulus_mask,
            modulus: self.modulus,
            modulus_mask: self.modulus_mask,
        }
    }
}

// Implement Group trait
impl Group for ModInt {
    fn zero() -> Self {
        // Default to common modulus for now - this should be configurable
        Self::new(0, 256)
    }
    
    fn one() -> Self {
        Self::new(1, 256)
    }
    
    fn negate(&mut self) {
        if self.val == 0 {
            self.val = 0;
        } else {
            self.val = self.modulus - self.val;
        }
    }
    
    fn reduce(&mut self) {
        self.val = self.val & self.modulus_mask;
    }
    
    fn add(&mut self, other: &Self) {
        assert_eq!(self.modulus, other.modulus, "Moduli must be equal");
        self.val = (self.val + other.val) & self.modulus_mask;
    }
    
    fn add_lazy(&mut self, other: &Self) {
        assert_eq!(self.modulus, other.modulus, "Moduli must be equal");
        self.val = self.val + other.val; // No reduction for lazy
    }
    
    fn mul(&mut self, other: &Self) {
        assert_eq!(self.modulus, other.modulus, "Moduli must be equal");
        self.val = (self.val * other.val) & self.modulus_mask;
    }
    
    fn mul_lazy(&mut self, other: &Self) {
        assert_eq!(self.modulus, other.modulus, "Moduli must be equal");
        self.val = self.val * other.val; // No reduction for lazy
    }
    
    fn sub(&mut self, other: &Self) {
        assert_eq!(self.modulus, other.modulus, "Moduli must be equal");
        if self.val >= other.val {
            self.val = self.val - other.val;
        } else {
            self.val = self.modulus - (other.val - self.val);
        }
    }
}

// Implement FromRng trait requirement for Share
impl crate::data_structures::prg::FromRng for ModInt {
    fn from_rng(&mut self, stream: &mut (impl rand::Rng + rand_core::RngCore)) {
        self.val = stream.random::<u128>() & self.modulus_mask;
    }

    fn randomize(&mut self) {
        self.val = rand::random::<u128>() & self.modulus_mask;
    }
}

// Implement Share trait
impl Share for ModInt {}

// Implement From<u32> for ModInt
impl From<u32> for ModInt {
    fn from(val: u32) -> Self {
        Self::new(val as u128, 256) // Default modulus
    }
}

// Implement TryFrom<Block> for ModInt
impl TryFrom<Block> for ModInt {
    type Error = &'static str;
    
    fn try_from(block: Block) -> Result<Self, Self::Error> {
        // Convert block to u128
        let val: u128 = unsafe { std::mem::transmute(block) };
        Ok(Self::new(val, 256)) // Default modulus
    }
}

// Implement Into<Block> for ModInt
impl Into<Block> for ModInt {
    fn into(self) -> Block {
        unsafe { std::mem::transmute(self.val) }
    }
}

/// Convert ModInt to bit width, ensuring it fits in less than 128 bits
pub fn get_bit_width_from_modint(modint: &ModInt) -> usize {
    let modulus = modint.modulus();
    let bit_width = (127 - modulus.leading_zeros()) as usize;
    assert!(bit_width < 128, "ModInt modulus requires {} bits, must be < 128", bit_width);
    bit_width
}
