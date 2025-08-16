use core::arch::x86_64::{
    __m128i, _mm_add_epi64, _mm_loadu_si128, _mm_set_epi64x, _mm_storeu_si128,
};

use aes::block_cipher::{generic_array::GenericArray, Block, BlockCipher, NewBlockCipher};
use aes::block_cipher::generic_array::typenum;
use aes::Aes128;

// AES key size in bytes. We always use AES-128,
// which has 16-byte keys.
const AES_KEY_SIZE: usize = 16;

// AES block size in bytes. Always 16 bytes.
pub const AES_BLOCK_SIZE: usize = 16;

// XXX Todo try using 8-way parallelism
pub struct FixedKeyPrgStream {
    aes: Aes128,
    ctr_generic_array: GenericArray<GenericArray<u8, typenum::U16>, typenum::U101>,
    buf_blocks: GenericArray<GenericArray<u8, typenum::U16>, typenum::U101>,
    count: usize,
    buf: [u8; 101 * AES_BLOCK_SIZE],
    buf_ptr: usize,
    have: usize,
}


impl FixedKeyPrgStream {
    pub fn new() -> Self {
        let key = GenericArray::from_slice(&[0; AES_KEY_SIZE]);
        // Initialize ctr_array and ctr_generic_array with counters from 0 to 100
        let mut ctr_array = [unsafe { std::mem::zeroed() }; 101];
        let mut ctr_generic_array = GenericArray::<GenericArray<u8, typenum::U16>, typenum::U101>::default();
        for i in 0..=100 {
            let mut ctr_bytes = [0u8; AES_BLOCK_SIZE];
            ctr_bytes[8..].copy_from_slice(&(i as u64).to_be_bytes());
            ctr_array[i] = FixedKeyPrgStream::load(&ctr_bytes);
            ctr_generic_array[i].copy_from_slice(&ctr_bytes);
        }
        FixedKeyPrgStream {
            aes: Aes128::new(&key),
            ctr_generic_array: ctr_generic_array.clone(),
            buf_blocks: ctr_generic_array,
            count: 0,
            buf: [0; 101 * AES_BLOCK_SIZE],
            buf_ptr: 0,
            have: 0,
        }
    }

    pub fn set_key(&mut self, key: &[u8; 16]) {
        for i in 0..self.count {
            self.buf_blocks[i].copy_from_slice(&self.ctr_generic_array[i]);
        }
        self.buf_ptr = 0;
        self.have = 0;
        self.count = 0;
    }

    pub fn skip_block(&mut self) {
        self.buf_ptr += AES_BLOCK_SIZE;
    }

    pub fn refill(&mut self) {
        self.have += AES_BLOCK_SIZE;

        let mut to_encrypt = self.ctr_generic_array[self.count];
        self.aes.encrypt_block(&mut to_encrypt);
        // Compute:   AES_0000(ctr) XOR ctr
        to_encrypt.iter_mut()
            .zip(self.ctr_generic_array[self.count].iter())
            .for_each(|(x1, x2)| *x1 ^= *x2);
        self.buf[self.count * AES_BLOCK_SIZE..(self.count + 1) * AES_BLOCK_SIZE]
            .copy_from_slice(&to_encrypt);
        self.count += 1;
    }

    pub fn refill8(&mut self) {
        self.have += 8 * AES_BLOCK_SIZE;

        // Create a reference to exactly 8 blocks for encryption
        let mut blocks_to_encrypt = GenericArray::<GenericArray<u8, typenum::U16>, typenum::U8>::from_mut_slice(
            &mut self.ctr_generic_array[self.count..self.count + 8]
        ).clone();
        
        self.aes.encrypt_blocks(&mut blocks_to_encrypt);
        
        for i in 0..8 {
            blocks_to_encrypt[i].iter_mut()
                .zip(self.ctr_generic_array[self.count + i].iter())
                .for_each(|(x1, x2)| *x1 ^= *x2);
            self.buf[(self.count + i) * AES_BLOCK_SIZE..(self.count + i + 1) * AES_BLOCK_SIZE]
                .copy_from_slice(&blocks_to_encrypt[i]);
        }
        self.count += 8;
    }

    // From RustCrypto aesni crate
    #[inline(always)]
    fn inc_be(v: __m128i) -> __m128i {
        unsafe { _mm_add_epi64(v, _mm_set_epi64x(1, 0)) }
    }

    #[inline(always)]
    fn store(val: __m128i, at: &mut [u8]) {
        debug_assert_eq!(at.len(), AES_BLOCK_SIZE);

        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            _mm_storeu_si128(at.as_mut_ptr() as *mut __m128i, val)
        }
    }

    // Modified from RustCrypto aesni crate
    #[inline(always)]
    fn load(key: &[u8; 16]) -> __m128i {
        let val = Block::<Aes128>::from_slice(key);

        // Safety: `loadu` supports unaligned loads
        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            _mm_loadu_si128(val.as_ptr() as *const __m128i)
        }
    }
}

impl rand::RngCore for FixedKeyPrgStream {
    fn next_u32(&mut self) -> u32 {
        rand_core::impls::next_u32_via_fill(self)
    }

    fn next_u64(&mut self) -> u64 {
        rand_core::impls::next_u64_via_fill(self)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut dest_ptr = 0;
        while dest_ptr < dest.len() {
            if self.have < dest.len() - dest_ptr {
                if dest.len() - dest_ptr - self.have > 4 * AES_BLOCK_SIZE {
                    self.refill8();
                } else {
                    self.refill();
                }
            }

            let to_copy = std::cmp::min(self.have, dest.len() - dest_ptr);
            // let start = Instant::now();
            dest[dest_ptr..dest_ptr + to_copy]
                .copy_from_slice(&self.buf[self.buf_ptr..self.buf_ptr + to_copy]);

            self.buf_ptr += to_copy;
            self.have -= to_copy;
            dest_ptr += to_copy;
        }
    }
}