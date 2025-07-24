use std::ops::{Add, Sub, Mul, BitAnd, BitXor, Index, IndexMut};
use rand::Rng;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::ser::SerializeStruct;
use serde::de::{self, Visitor, SeqAccess, MapAccess};
use std::fmt;

#[derive(Clone, Debug, Copy)]
pub struct RingVec<const N: usize> {
    val: [u128; N],
    modulus: u128,
    modulus_mask: u128, // Stores modulus - 1 for efficient bitwise modulo
}

impl<const N: usize> RingVec<N> {
    pub fn new(val: [u128; N], modulus: u128) -> Self {
        if !modulus.is_power_of_two() || modulus == 0 {
            panic!("Modulus must be a power of two and non-zero.");
        }
        let modulus_mask = modulus - 1;
        let mut new_val = [0u128; N];
        for i in 0..N {
            new_val[i] = val[i] & modulus_mask; // Apply modulus initially
        }
        RingVec {
            val: new_val,
            modulus,
            modulus_mask,
        }
    }

    pub fn zero(modulus: u128) -> Self {
        if !modulus.is_power_of_two() || modulus == 0 {
            panic!("Modulus must be a power of two and non-zero.");
        }
        RingVec {
            val: [0; N],
            modulus,
            modulus_mask: modulus - 1,
        }
    }

    pub fn random(modulus: u128) -> Self {
        if !modulus.is_power_of_two() || modulus == 0 {
            panic!("Modulus must be a power of two and non-zero.");
        }
        let mut val = [0u128; N];
        for i in 0..N {
            val[i] = rand::rng().random::<u128>() & (modulus - 1);
        }
        RingVec {
            val,
            modulus,
            modulus_mask: modulus - 1,
        }
    }

    fn element_wise_ringvec_op<F>(mut self, other: &Self, op: F) -> Self
    where
        F: Fn(u128, u128) -> u128,
    {
        debug_assert_eq!(self.modulus, other.modulus, "RingVec operations require matching moduli.");

        for i in 0..N {
            self.val[i] = op(self.val[i], other.val[i]) & self.modulus_mask;
        }
        self
    }

    fn element_wise_scalar_op<F>(mut self, scalar: u128, op: F) -> Self
    where
        F: Fn(u128, u128) -> u128,
    {
        for i in 0..N {
            self.val[i] = op(self.val[i], scalar) & self.modulus_mask;
        }
        self
    }

    /// Returns the modulus of this RingVec.
    pub fn modulus(&self) -> u128 {
        self.modulus
    }

    /// Returns the value array of this RingVec.
    pub fn val(&self) -> &[u128; N] {
        &self.val
    }
}

// --- RingVec-to-RingVec Operations ---
// Note: These implementations implicitly assume matching moduli.
// In a real-world scenario, you might want to return a Result or panic
// if moduli don't match, or define a clear conversion strategy.

impl<const N: usize> Add for RingVec<N> {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        self.element_wise_ringvec_op(&other, |a, b| a + b)
    }
}

impl<const N: usize> Sub for RingVec<N> {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        let modulus = self.modulus;
        self.element_wise_ringvec_op(&other, |a, b| a + modulus - b)
    }
}

impl<const N: usize> Mul for RingVec<N> {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        self.element_wise_ringvec_op(&other, |a, b| a * b)
    }
}

// --- Indexing Implementations ---

impl<const N: usize> Index<usize> for RingVec<N> {
    type Output = u128;
    fn index(&self, index: usize) -> &Self::Output {
        &self.val[index]
    }
}

impl<const N: usize> IndexMut<usize> for RingVec<N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let val_ref = &mut self.val[index];
        val_ref
    }
}


// --- NEW: Implement Scalar Operations (RingVec<N> op u128) ---

impl<const N: usize> Add<u128> for RingVec<N> {
    type Output = Self;
    fn add(self, rhs: u128) -> Self::Output {
        self.element_wise_scalar_op(rhs, |a, b| a + b)
    }
}

impl<const N: usize> Sub<u128> for RingVec<N> {
    type Output = Self;
    fn sub(self, rhs: u128) -> Self::Output {
        let modulus = self.modulus;
        self.element_wise_scalar_op(rhs, |a, b| a + modulus - b)
    }
}

impl<const N: usize> Mul<u128> for RingVec<N> {
    type Output = Self;
    fn mul(self, rhs: u128) -> Self::Output {
        self.element_wise_scalar_op(rhs, |a, b| a * b)
    }
}

// Custom Serialize implementation for RingVec
impl<const N: usize> Serialize for RingVec<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("RingVec", 3)?;
        state.serialize_field("val", &self.val.to_vec())?; // Convert array to Vec for serialization
        state.serialize_field("modulus", &self.modulus)?;
        state.serialize_field("modulus_mask", &self.modulus_mask)?;
        state.end()
    }
}

// Custom Deserialize implementation for RingVec
impl<'de, const N: usize> Deserialize<'de> for RingVec<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field { Val, Modulus, ModulusMask }

        struct RingVecVisitor<const N: usize>;

        impl<'de, const N: usize> Visitor<'de> for RingVecVisitor<N> {
            type Value = RingVec<N>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct RingVec")
            }

            fn visit_map<V>(self, mut map: V) -> Result<RingVec<N>, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut val = None;
                let mut modulus = None;
                let mut modulus_mask = None;
                
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Val => {
                            if val.is_some() {
                                return Err(de::Error::duplicate_field("val"));
                            }
                            let val_vec: Vec<u128> = map.next_value()?;
                            if val_vec.len() != N {
                                return Err(de::Error::custom(format!("Expected array of length {}, got {}", N, val_vec.len())));
                            }
                            let mut val_array = [0u128; N];
                            val_array.copy_from_slice(&val_vec);
                            val = Some(val_array);
                        }
                        Field::Modulus => {
                            if modulus.is_some() {
                                return Err(de::Error::duplicate_field("modulus"));
                            }
                            modulus = Some(map.next_value()?);
                        }
                        Field::ModulusMask => {
                            if modulus_mask.is_some() {
                                return Err(de::Error::duplicate_field("modulus_mask"));
                            }
                            modulus_mask = Some(map.next_value()?);
                        }
                    }
                }
                
                let val = val.ok_or_else(|| de::Error::missing_field("val"))?;
                let modulus = modulus.ok_or_else(|| de::Error::missing_field("modulus"))?;
                let modulus_mask = modulus_mask.ok_or_else(|| de::Error::missing_field("modulus_mask"))?;
                
                Ok(RingVec { val, modulus, modulus_mask })
            }
        }

        const FIELDS: &'static [&'static str] = &["val", "modulus", "modulus_mask"];
        deserializer.deserialize_struct("RingVec", FIELDS, RingVecVisitor)
    }
}
