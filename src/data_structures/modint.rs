use std::ops::{Add, Sub, Mul};

#[derive(Clone, Copy, Debug, PartialEq)]
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
