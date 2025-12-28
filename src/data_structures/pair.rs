use std::ops::{Add, BitAnd, BitXor, Mul, Sub}; // Import all necessary traits

/// A generic struct to represent a pair of values of type `T`.
#[derive(Debug, PartialEq, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pair<T> {
    pub first: T,
    pub second: T,
}

impl<T> Pair<T> {
    /// Creates a new Pair.
    pub fn new(first: T, second: T) -> Self {
        Pair { first, second }
    }
}

// --- XOR Implementation (from previous answer, included for completeness) ---
impl<T> BitXor for Pair<T>
where
    T: BitXor<Output = T>,
{
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Pair {
            first: self.first ^ rhs.first,
            second: self.second ^ rhs.second,
        }
    }
}

// --- BITAND Implementation (Pair<T> & T) ---
// This allows `Pair<T> & T_scalar`
impl<T> BitAnd<T> for Pair<T>
where
    // T must implement the BitAnd trait, and its output must also be T
    // This bound applies to the element-wise operation (self.first & rhs_scalar)
    T: BitAnd<Output = T> + Copy, // `Copy` is needed because `rhs` is taken by value
{
    type Output = Self; // The output is still a Pair<T>

    /// Performs element-wise bitwise AND with a scalar value.
    fn bitand(self, rhs_scalar: T) -> Self::Output {
        Pair {
            first: self.first & rhs_scalar,
            second: self.second & rhs_scalar,
        }
    }
}

// --- ADD Implementation ---
impl<T> Add for Pair<T>
where
    // T must implement the Add trait, and its output must also be T
    T: Add<Output = T>,
{
    type Output = Self; // Adding two Pairs results in another Pair

    /// Performs element-wise addition.
    fn add(self, rhs: Self) -> Self::Output {
        Pair {
            first: self.first + rhs.first,
            second: self.second + rhs.second,
        }
    }
}

// --- SUB Implementation ---
impl<T> Sub for Pair<T>
where
    // T must implement the Sub trait, and its output must also be T
    T: Sub<Output = T>,
{
    type Output = Self; // Subtracting two Pairs results in another Pair

    /// Performs element-wise subtraction.
    fn sub(self, rhs: Self) -> Self::Output {
        Pair {
            first: self.first - rhs.first,
            second: self.second - rhs.second,
        }
    }
}

// --- NEW: MUL Implementation (Pair<T> * Pair<T>) ---
// This allows `Pair<T> * Pair<T>` for element-wise multiplication
impl<T> Mul for Pair<T>
where
    // T must implement the Mul trait, and its output must also be T
    T: Mul<Output = T>,
{
    type Output = Self; // Multiplying two Pairs results in another Pair

    /// Performs element-wise multiplication.
    fn mul(self, rhs: Self) -> Self::Output {
        Pair {
            first: self.first * rhs.first,
            second: self.second * rhs.second,
        }
    }
}

// --- NEW: MUL Implementation (Pair<T> * T) ---
// This allows `Pair<T> * T_scalar` for scalar multiplication
impl<T> Mul<T> for Pair<T>
where
    // T must implement the Mul trait, and its output must also be T
    T: Mul<Output = T> + Copy, // `Copy` is needed because `rhs_scalar` is used twice
{
    type Output = Self; // The output is still a Pair<T>

    /// Performs element-wise scalar multiplication.
    fn mul(self, rhs_scalar: T) -> Self::Output {
        Pair {
            first: self.first * rhs_scalar,
            second: self.second * rhs_scalar,
        }
    }
}
