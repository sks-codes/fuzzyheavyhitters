use core::arch::x86_64::{
    __m128i, _mm_add_epi64, _mm_loadu_si128, _mm_set_epi64x, _mm_storeu_si128,
};

use aes::block_cipher::{generic_array::GenericArray, Block, BlockCipher, NewBlockCipher};
use aes::Aes128;
use aes_ctr::stream_cipher::{NewStreamCipher, SyncStreamCipher};
use aes_ctr::Aes128Ctr;

use rand::Rng;
use rand_core::RngCore;

use serde::Deserialize;
use serde::Serialize;
use std::ops;

// AES key size in bytes. We always use AES-128,
// which has 16-byte keys.
const AES_KEY_SIZE: usize = 16;

// AES block size in bytes. Always 16 bytes.
pub const AES_BLOCK_SIZE: usize = 16;

// XXX Todo try using 8-way parallelism
pub struct FixedKeyPrgStream {
    aes: Aes128,
    ctr: __m128i,
    buf: [u8; AES_BLOCK_SIZE * 8],
    have: usize,
    buf_ptr: usize,
    count: usize,
}


impl FixedKeyPrgStream {
    pub fn new() -> Self {
        let key = GenericArray::from_slice(&[0; AES_KEY_SIZE]);

        let ctr_init = FixedKeyPrgStream::load(&[0; AES_BLOCK_SIZE]);
        FixedKeyPrgStream {
            aes: Aes128::new(&key),
            ctr: ctr_init,
            buf: [0; AES_BLOCK_SIZE * 8],
            buf_ptr: AES_BLOCK_SIZE,
            have: AES_BLOCK_SIZE,
            count: 0,
        }
    }

    pub fn set_key(&mut self, key: &[u8; 16]) {
        self.ctr = FixedKeyPrgStream::load(key);
        self.buf_ptr = AES_BLOCK_SIZE;
        self.have = AES_BLOCK_SIZE;
    }

    pub fn skip_block(&mut self) {
        // Only allow skipping a block on a block boundary.
        debug_assert_eq!(self.have % AES_BLOCK_SIZE, 0);
        debug_assert_eq!(self.buf_ptr, AES_BLOCK_SIZE);
        self.ctr = FixedKeyPrgStream::inc_be(self.ctr);
    }

    pub fn refill(&mut self) {
        //println!("Refill");
        debug_assert_eq!(self.buf_ptr, AES_BLOCK_SIZE);

        self.have = AES_BLOCK_SIZE;
        self.buf_ptr = 0;

        // Write counter into buffer.
        FixedKeyPrgStream::store(self.ctr, &mut self.buf[0..AES_BLOCK_SIZE]);

        let count_bytes = self.buf;
        let mut gen = GenericArray::from_mut_slice(&mut self.buf[0..AES_BLOCK_SIZE]);
        self.aes.encrypt_block(&mut gen);

        // Compute:   AES_0000(ctr) XOR ctr
        self.buf
            .iter_mut()
            .zip(count_bytes.iter())
            .for_each(|(x1, x2)| *x1 ^= *x2);

        self.ctr = FixedKeyPrgStream::inc_be(self.ctr);
        self.count += AES_BLOCK_SIZE;
    }

    pub fn refill8(&mut self) {
        self.have = 8 * AES_BLOCK_SIZE;
        self.buf_ptr = 0;

        let block = GenericArray::clone_from_slice(&[0u8; 16]);
        let mut block8 = GenericArray::clone_from_slice(&[block; 8]);

        let mut cnts = [[0u8; AES_BLOCK_SIZE]; 8];
        for i in 0..8 {
            // Write counter into buffer
            FixedKeyPrgStream::store(self.ctr, &mut block8[i]);
            FixedKeyPrgStream::store(self.ctr, &mut cnts[i]);
            self.ctr = FixedKeyPrgStream::inc_be(self.ctr);
        }

        self.aes.encrypt_blocks(&mut block8);

        for i in 0..8 {
            // Compute:   AES_0000(ctr) XOR ctr
            block8[i]
                .iter_mut()
                .zip(cnts[i].iter())
                .for_each(|(x1, x2)| *x1 ^= *x2);
        }

        for i in 0..8 {
            self.buf[i * AES_BLOCK_SIZE..(i + 1) * AES_BLOCK_SIZE].copy_from_slice(&block8[i]);
        }

        self.count += 8 * AES_BLOCK_SIZE;

        //println!("Blocks: {:?}", self.buf[0]);
        //println!("Blocks: {:?}", self.buf[1]);
        //println!("Blocks: {:?}", self.buf[2]);
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
            if self.buf_ptr == self.have {
                if dest.len() > 4 * AES_BLOCK_SIZE {
                    self.refill8();
                //self.refill();
                } else {
                    self.refill();
                }
            }

            let to_copy = std::cmp::min(self.have - self.buf_ptr, dest.len() - dest_ptr);
            dest[dest_ptr..dest_ptr + to_copy]
                .copy_from_slice(&self.buf[self.buf_ptr..self.buf_ptr + to_copy]);

            self.buf_ptr += to_copy;
            dest_ptr += to_copy;
        }
    }
}