use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use rand::Rng;

pub struct PRG {
    counter: u64,
    aes: Aes128,
    key: [u8; 16],
}

impl PRG {
    /// Create a new PRG instance with an optional seed and ID.
    pub fn new(seed: Option<&[u8; 16]>, id: u64) -> Self {
        let mut key = [0u8; 16];
        if let Some(s) = seed {
            key.copy_from_slice(s);
        } else {
            key = PRG::generate_random_key();
        }
        PRG::apply_id_to_key(&mut key, id);

        let aes = Aes128::new(GenericArray::from_slice(&key));
        PRG {
            counter: 0,
            aes,
            key,
        }
    }

    /// Generate a random 16-byte key using a secure random generator.
    fn generate_random_key() -> [u8; 16] {
        let mut rng = rand::thread_rng();
        let mut key = [0u8; 16];
        rng.fill(&mut key);
        key
    }

    /// Apply the ID to the key for reseeding purposes.
    fn apply_id_to_key(key: &mut [u8; 16], id: u64) {
        let id_bytes = id.to_le_bytes();
        for (i, byte) in id_bytes.iter().enumerate() {
            key[i] ^= byte;
        }
    }

    /// Reseed the PRG with a new seed and ID.
    pub fn reseed(&mut self, seed: &[u8; 16], id: u64) {
        self.key.copy_from_slice(seed);
        PRG::apply_id_to_key(&mut self.key, id);
        self.aes = Aes128::new(GenericArray::from_slice(&self.key));
        self.counter = 0;
    }

    pub fn random_16byte_block(&mut self, blocks: &mut [[u8; 16]]) {
        // Create an array of AES blocks for encryption
        let mut aes_blocks: Vec<_> = (0..blocks.len())
                .map(|_| {
                let mut block = [0u8; 16];
                block[8..].copy_from_slice(&self.counter.to_le_bytes());
                self.counter += 1; // Increment counter for each block
                GenericArray::clone_from_slice(&block)
            })
            .collect();
        // Encrypt all blocks in one call
        self.aes.encrypt_blocks(&mut aes_blocks);

        // Copy the encrypted blocks back into the original `blocks` array
        for (i, encrypted) in aes_blocks.iter().enumerate() {
            blocks[i].copy_from_slice(encrypted);
        }
    }

    pub fn random_32byte_block(&mut self, blocks: &mut [[u8; 32]]) {
        // Preallocate space for AES blocks (2 AES blocks per 32-byte block)
        let mut aes_blocks: Vec<_> =
            vec![GenericArray::default(); blocks.len() * 2];

        for (i, block) in blocks.iter_mut().enumerate() {
            // Embed the counter in the last 8 bytes of the 32-byte block
            block[8..16].copy_from_slice(&self.counter.to_le_bytes());
            self.counter += 1;
            block[24..32].copy_from_slice(&self.counter.to_le_bytes());
            self.counter += 1;

            // Write the two halves of the 32-byte block into the AES blocks vector
            aes_blocks[i * 2].copy_from_slice(&block[0..16]);
            aes_blocks[i * 2 + 1].copy_from_slice(&block[16..32]);
        }

        // Encrypt all blocks in one batch
        self.aes.encrypt_blocks(&mut aes_blocks);

        // Copy the encrypted halves back into their respective 32-byte blocks
        for (i, block) in blocks.iter_mut().enumerate() {
            block[0..16].copy_from_slice(&aes_blocks[i * 2]);
            block[16..32].copy_from_slice(&aes_blocks[i * 2 + 1]);
        }
    }

    pub fn random_bool_array(&mut self, bits: &mut [bool]) {
        let mut blocks = vec![[0u8; 16]; (bits.len() + 127) / 128];
        self.random_16byte_block(&mut blocks); // Use the AES-based random_block generator

        bits.iter_mut().enumerate().for_each(|(i, bit)| {
            let block_index = i / 128;
            let bit_offset = i % 128;
            let byte_index = bit_offset / 8;
            let bit_index = bit_offset % 8;

            *bit = (blocks[block_index][byte_index] & (1 << bit_index)) != 0;
        });
    }

    pub fn fill_bytes(&mut self, buffer: &mut [u8]) {
        let block_count = (buffer.len() + 15) / 16;
        let mut blocks = vec![[0u8; 16]; block_count];

        // Generate random blocks using AES
        self.random_16byte_block(&mut blocks);

        // Flatten blocks into the buffer
        for (i, byte) in buffer.iter_mut().enumerate() {
            let block_index = i / 16;
            let byte_index = i % 16;
            *byte = blocks[block_index][byte_index];
        }
    }
}