use blake3;
use std::convert::TryInto;
use std::fmt::Debug;
use std::ops::BitXor;

// Custom error type for OKVS operations
#[derive(Debug, Clone, PartialEq)]
pub enum OkvsError {
    ZeroRow(usize),
    InvalidMatrix,
    DecodingFailed,
    IncompatibleDimensions,
}

impl std::fmt::Display for OkvsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OkvsError::ZeroRow(row) => write!(f, "Zero row found at index {}", row),
            OkvsError::InvalidMatrix => write!(f, "Invalid matrix structure"),
            OkvsError::DecodingFailed => write!(f, "Failed to decode the encoding"),
            OkvsError::IncompatibleDimensions => write!(f, "Incompatible matrix dimensions"),
        }
    }
}

impl std::error::Error for OkvsError {}

// Type alias for convenience
pub type Result<T> = std::result::Result<T, OkvsError>;

/// Trait defining the requirements for OKVS value types
pub trait OkvsValue: BitXor<Output = Self> + Copy + Clone + Debug + Default + PartialEq {}

// Implement the trait for common types
impl OkvsValue for u128 {}
impl OkvsValue for u64 {}
impl OkvsValue for u32 {}
impl OkvsValue for u16 {}
impl OkvsValue for u8 {}
impl OkvsValue for bool {}

pub struct RbOkvsF2k<V: OkvsValue> {
    pub kv_count: usize,
    pub columns: usize,
    band_width: usize,
    r1: [u8; 16],
    r2: [u8; 16],
    _phantom: std::marker::PhantomData<V>,
}

impl<V: OkvsValue> RbOkvsF2k<V> {
    pub fn new(
        kv_count: usize,
        columns: usize,
        band_width: usize,
        r1: &[u8; 16],
        r2: &[u8; 16],
    ) -> Self {
        assert!(
            band_width <= columns,
            "Band width must be less than or equal to the number of columns"
        );
        assert!(
            band_width <= 256,
            "Band width must be less than or equal to 256"
        );
        Self {
            kv_count,
            columns,
            band_width,
            r1: *r1,
            r2: *r2,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn encode(&self, keys: &Vec<Vec<bool>>, values: &Vec<V>) -> Result<Vec<V>> {
        assert_eq!(
            keys.len(),
            values.len(),
            "Keys and values must have the same length"
        );
        assert_eq!(keys.len(), self.kv_count, "Keys length must match kv_count");
        let (mut matrix, start_pos, mut y) = self.create_sorted_matrix(keys, values)?;
        self.simple_gauss(&mut y, &mut matrix, start_pos)
    }

    pub fn decode(&self, encoding: &Vec<V>, key: &[Vec<bool>]) -> Vec<V> {
        let n = key.len();
        let mut start = vec![0usize; n];
        let mut band = vec![vec![false; self.band_width]; n];
        start.iter_mut().enumerate().for_each(|(i, start_i)| {
            *start_i = self.hash_to_index(&key[i], &self.r1, self.columns - self.band_width + 1);
        });

        band.iter_mut().enumerate().for_each(|(i, band_i)| {
            *band_i = self.hash_to_band(&key[i], &self.r2);
        });

        let mut res = vec![V::default(); n];

        for i in 0..n {
            for j in 0..self.band_width {
                if band[i][j] {
                    res[i] = res[i] ^ encoding[start[i] + j];
                }
            }
        }

        res
    }

    fn create_sorted_matrix(
        &self,
        keys: &Vec<Vec<bool>>,
        values: &Vec<V>,
    ) -> Result<(Vec<Vec<bool>>, Vec<usize>, Vec<V>)> {
        let mut start_pos: Vec<(usize, usize)> = vec![(0, 0); self.kv_count];
        let mut matrix: Vec<Vec<bool>> = vec![vec![false; self.band_width]; self.kv_count];
        let mut start_ids: Vec<usize> = vec![0; self.kv_count];
        let mut y: Vec<V> = vec![V::default(); self.kv_count];

        start_pos
            .iter_mut()
            .enumerate()
            .for_each(|(i, start_pos_i)| {
                *start_pos_i = (
                    i,
                    self.hash_to_index(&keys[i], &self.r1, self.columns - self.band_width + 1),
                );
            });

        radix_sort(&mut start_pos, self.columns - self.band_width);

        matrix.iter_mut().enumerate().for_each(|(i, matrix_i)| {
            *matrix_i = self.hash_to_band(&keys[start_pos[i].0], &self.r2);
        });

        y.iter_mut().enumerate().for_each(|(i, y_i)| {
            *y_i = values[start_pos[i].0];
        });

        start_ids
            .iter_mut()
            .enumerate()
            .for_each(|(i, start_ids_i)| {
                *start_ids_i = start_pos[i].1;
            });

        Ok((matrix, start_ids, y))
    }

    fn simple_gauss(
        &self,
        y: &mut Vec<V>,
        bands: &mut Vec<Vec<bool>>,
        start_pos: Vec<usize>,
    ) -> Result<Vec<V>> {
        assert_eq!(
            bands.len(),
            self.kv_count,
            "Number of bands must match kv_count"
        );
        assert_eq!(y.len(), self.kv_count, "Length of y must match kv_count");

        let mut pivot = vec![0 as usize; self.kv_count];
        let mut first_nonzero = vec![self.band_width; self.kv_count];

        for i in 0..self.kv_count {
            for j in 0..self.band_width {
                if bands[i][j] {
                    first_nonzero[i] = j;
                    break;
                }
            }

            if first_nonzero[i] == self.band_width {
                return Err(OkvsError::ZeroRow(i));
            }

            pivot[i] = first_nonzero[i] + start_pos[i];

            let bands_i = bands[i].clone();
            let y_i = y[i];

            for j in (i + 1)..self.kv_count {
                if start_pos[j] > pivot[i] {
                    break;
                }
                let offset = pivot[i] - start_pos[j];
                let lead = bands[j][offset];
                if lead {
                    for k in 0..(self.band_width - first_nonzero[i]) {
                        bands[j][k + offset] ^= bands_i[k + first_nonzero[i]];
                    }
                    y[j] = y[j] ^ y_i;
                }
            }
        }

        let mut x = vec![V::default(); self.columns];
        for i in (0..self.kv_count).rev() {
            let mut res = y[i];
            for j in 0..self.band_width {
                if bands[i][j] {
                    res = res ^ x[start_pos[i] + j];
                }
            }
            x[pivot[i]] = res;
        }

        for i in 0..self.kv_count {
            let mut res = V::default();
            for j in 0..self.band_width {
                if bands[i][j] {
                    res = res ^ x[start_pos[i] + j];
                }
            }
            if res != y[i] {
                return Err(OkvsError::DecodingFailed);
            }
        }

        Ok(x)
    }

    fn hash_to_index(&self, x: &[bool], r1: &[u8; 16], column: usize) -> usize {
        let mut hasher = blake3::Hasher::new();

        // Include the original length
        hasher.update(&(x.len() as u64).to_le_bytes());

        // Pack bools into bytes (8 bools per byte)
        let packed_bytes = self.pack_bools_to_bytes(x);
        hasher.update(&packed_bytes);

        hasher.update(r1);
        let hash = hasher.finalize();
        println!("Column: {}", column);
        let index =
            u128::from_le_bytes(hash.as_bytes()[0..16].try_into().unwrap()) % (column as u128);
        index as usize
    }

    fn hash_to_band(&self, x: &[bool], r2: &[u8; 16]) -> Vec<bool> {
        let mut hasher = blake3::Hasher::new();

        // Include the original length
        hasher.update(&(x.len() as u64).to_le_bytes());

        // Pack bools into bytes (8 bools per byte)
        let packed_bytes = self.pack_bools_to_bytes(x);
        hasher.update(&packed_bytes);

        hasher.update(r2);
        let hash = hasher.finalize();
        let mut band = vec![false; self.band_width];
        for i in 0..self.band_width {
            band[i] = (hash.as_bytes()[i / 8] >> (i % 8) & 1) != 0;
        }
        band
    }

    /// Helper method to pack a vector of bools into bytes
    /// Each byte contains up to 8 bools, with remaining bits set to 0
    fn pack_bools_to_bytes(&self, bools: &[bool]) -> Vec<u8> {
        let num_bytes = (bools.len() + 7) / 8; // Round up to nearest byte
        let mut bytes = vec![0u8; num_bytes];

        for (i, &bit) in bools.iter().enumerate() {
            if bit {
                bytes[i / 8] |= 1 << (i % 8);
            }
        }

        bytes
    }
}

/// Sort by arr[i].1
pub fn radix_sort(arr: &mut Vec<(usize, usize)>, max: usize) {
    let mut exp = 1;
    loop {
        if max / exp == 0 {
            break;
        }
        *arr = count_sort(arr, exp);
        exp *= 10;
    }
}

fn count_sort(arr: &Vec<(usize, usize)>, exp: usize) -> Vec<(usize, usize)> {
    let mut count = [0usize; 10];

    arr.iter().for_each(|(_, b)| count[(b / exp) % 10] += 1);

    for i in 1..10 {
        count[i] += count[i - 1];
    }

    let mut output = vec![(0usize, 0usize); arr.len()];

    arr.iter().rev().for_each(|(a, b)| {
        output[count[(b / exp) % 10] - 1] = (*a, *b);
        count[(b / exp) % 10] -= 1;
    });

    output
}
