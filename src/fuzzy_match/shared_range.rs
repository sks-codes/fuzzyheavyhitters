use crate::{
    fss::{
        distance::{DistanceFSSEval, DistanceFSSKey},
        interval::{IntervalFSSEval, IntervalFSSKey},
    },
    util::{bits_to_u128_msb, bits_to_u8s, u128_to_bits_msb, u8s_to_bits},
};
use std::convert::TryInto;
use anyhow::{anyhow, Result};

#[derive(Clone, Debug)]
pub enum ShareData {
    OKVS {
        eval: Vec<u128>,
    },
    IntervalFSS {
        data: Vec<IntervalFSSEval>,
    },
    DistanceFSS {
        data: Vec<DistanceFSSEval>,
        eval: Vec<u128>,
    },
}

impl ShareData {
    pub fn to_bytes(&self, eval_len: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            ShareData::OKVS { eval } => {
                out.push(0u8); // tag for OKVS
                out.extend_from_slice(&(eval.len() as u32).to_le_bytes());
                for v in eval {
                    let eval_bits = u128_to_bits_msb(*v, eval_len);
                    out.extend_from_slice(&bits_to_u8s(&eval_bits));
                }
            }
            ShareData::IntervalFSS { data } => {
                out.push(1u8); // tag for IntervalFSS
                out.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for interval in data {
                    let bytes = interval
                        .to_bytes()
                        .expect("Failed to serialize IntervalFSSEval");
                    out.extend_from_slice(&bytes);
                }
            }
            ShareData::DistanceFSS { data, eval } => {
                out.push(2u8); // tag for DistanceFSS
                out.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for eval_item in data {
                    let bytes = eval_item
                        .to_bytes()
                        .expect("Failed to serialize DistanceFSSEval");
                    out.extend_from_slice(&bytes);
                }
                out.extend_from_slice(&(eval.len() as u32).to_le_bytes());
                for v in eval {
                    let eval_bits = u128_to_bits_msb(*v, eval_len);
                    out.extend_from_slice(&bits_to_u8s(&eval_bits));
                }
            }
        }
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8], eval_len: usize, modulus: u128) -> Result<(Self, usize)> {
        let eval_len_bytes = (eval_len + 7) / 8; // Calculate the number of bytes needed to represent eval_len bits
        let mut offset = 0;
        if bytes.is_empty() {
            return Err(anyhow!("Empty byte slice"));
        }
        let tag = bytes[offset];
        offset += 1;
        match tag {
            0 => {
                // OKVS
                if bytes.len() < offset + 4 {
                    panic!("Insufficient bytes for OKVS eval length");
                }
                let num_of_eval =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut eval = Vec::with_capacity(num_of_eval);
                for _ in 0..num_of_eval {
                    if bytes.len() < offset + eval_len_bytes {
                        panic!("Insufficient bytes for OKVS eval data");
                    }
                    let val = u8s_to_bits(&bytes[offset..offset + eval_len_bytes], eval_len);
                    offset += eval_len_bytes;
                    eval.push(bits_to_u128_msb(&val));
                }
                Ok((ShareData::OKVS { eval }, offset))
            }
            1 => {
                // IntervalFSS
                if bytes.len() < offset + 4 {
                    panic!("Insufficient bytes for IntervalFSS keys length");
                }
                let num_of_data =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut data = Vec::with_capacity(num_of_data);
                for _ in 0..num_of_data {
                    let (key, used) =
                        IntervalFSSEval::from_bytes(&bytes[offset..], modulus as u128)
                            .expect("Failed to deserialize IntervalFSSEval");
                    offset += used;
                    data.push(key);
                }
                Ok((ShareData::IntervalFSS { data }, offset))
            }
            2 => {
                // DistanceFSS
                if bytes.len() < offset + 4 {
                    panic!("Insufficient bytes for DistanceFSS keys length");
                }
                let num_of_data =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut data = Vec::with_capacity(num_of_data);
                for _ in 0..num_of_data {
                    let (key, used) =
                        DistanceFSSEval::from_bytes(&bytes[offset..], modulus as u128)
                            .expect("Failed to deserialize DistanceFSSEval");
                    offset += used;
                    data.push(key);
                }
                let num_of_eval =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut eval = Vec::with_capacity(num_of_eval);
                for _ in 0..num_of_eval {
                    if bytes.len() < offset + eval_len_bytes {
                        panic!("Insufficient bytes for DistanceFSS eval data");
                    }
                    let val = u8s_to_bits(&bytes[offset..offset + eval_len_bytes], eval_len);
                    offset += eval_len_bytes;
                    eval.push(bits_to_u128_msb(&val));
                }
                Ok((ShareData::DistanceFSS { data, eval }, offset))
            }
            _ => Err(anyhow!("Unknown ShareData tag")),
        }
    }
}

/// Represents the shared data for a range around input x
#[derive(Debug, Clone)]
pub enum SharedRange {
    OKVS {
        okvs_shares: Vec<Vec<u128>>,           // One OKVS encoding per dimension
        okvs_seeds: Vec<([u8; 16], [u8; 16])>, // Seeds for OKVS (r1, r2)
        role: bool,                            // True for server 1, false for server 0
        p: Option<u32>,
    },
    IntervalFSS {
        keys: Vec<IntervalFSSKey>, // One key pair per dimension
        role: bool,
    },
    DistanceFSS {
        keys: Vec<DistanceFSSKey>,
        role: bool,
    },
}

// TODO: Need to pad the number of OKVS key-value pairs to be deterministic
impl SharedRange {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            SharedRange::OKVS {
                okvs_shares,
                okvs_seeds,
                role,
                p,
            } => {
                out.push(0u8); // tag for OKVS
                out.push(*role as u8);
                match p {
                    Some(val) => {
                        out.push(1u8);
                        out.extend_from_slice(&val.to_le_bytes());
                    }
                    None => {
                        out.push(0u8);
                    }
                }
                out.extend_from_slice(&(okvs_shares.len() as u32).to_le_bytes());
                for dim in okvs_shares {
                    out.extend_from_slice(&(dim.len() as u32).to_le_bytes());
                    for v in dim {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                for (r1, r2) in okvs_seeds {
                    out.extend_from_slice(r1);
                    out.extend_from_slice(r2);
                }
            }
            SharedRange::IntervalFSS { keys, role } => {
                out.push(1u8); // tag for IntervalFSS
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    let bytes = k
                        .to_bytes()
                        .expect("Failed to serialize IntervalFSSKey");
                    out.extend_from_slice(&bytes);
                }
            }
            SharedRange::DistanceFSS { keys, role } => {
                out.push(2u8); // tag for DistanceFSS
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    let bytes = k.to_bytes().expect("Failed to serialize DistanceFSSKey");
                    out.extend_from_slice(&bytes);
                }
            }
        }
        out
    }

    /// Returns (SharedRange, rest)
    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize), String> {
        if bytes.is_empty() {
            return Err("Empty bytes for SharedRange".to_string());
        }
        let mut offset = 0;
        let tag = bytes[offset];
        offset += 1;
        match tag {
            0 => {
                // OKVS
                if bytes[offset..].len() < 2 {
                    return Err("Too short for OKVS header".to_string());
                }
                let role = bytes[offset] != 0;
                offset += 1;
                let has_p = bytes[offset];
                offset += 1;
                let p = if has_p == 1 {
                    let mut arr = [0u8; 4];
                    arr.copy_from_slice(&bytes[offset..offset + 4]);
                    offset += 4;
                    Some(u32::from_le_bytes(arr))
                } else {
                    None
                };
                let dim_count =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut okvs_shares = Vec::with_capacity(dim_count);
                for _ in 0..dim_count {
                    let dim_len =
                        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                    offset += 4;
                    let mut dim = Vec::with_capacity(dim_len);
                    for _ in 0..dim_len {
                        dim.push(u128::from_le_bytes(
                            bytes[offset..offset + 16].try_into().unwrap(),
                        ));
                        offset += 16;
                    }
                    okvs_shares.push(dim);
                }
                let mut okvs_seeds = Vec::with_capacity(dim_count);
                for _ in 0..dim_count {
                    let r1: [u8; 16] = bytes[offset..offset + 16].try_into().unwrap();
                    offset += 16;
                    let r2: [u8; 16] = bytes[offset..offset + 16].try_into().unwrap();
                    offset += 16;
                    okvs_seeds.push((r1, r2));
                }
                Ok((
                    SharedRange::OKVS {
                        okvs_shares,
                        okvs_seeds,
                        role,
                        p,
                    },
                    offset,
                ))
            }
            1 => {
                // IntervalFSS
                if bytes[offset..].len() < 5 {
                    return Err("Too short for IntervalFSS header".to_string());
                }
                let role = bytes[offset] != 0;
                offset += 1;
                let key_count =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, used_k) = IntervalFSSKey::from_bytes(&bytes[offset..], modulus)
                        .expect("Failed to deserialize IntervalFSSKey");
                    offset += used_k;
                    keys.push(k);
                }
                Ok((SharedRange::IntervalFSS { keys, role }, offset))
            }
            2 => {
                // DistanceFSS
                if bytes[offset..].len() < 5 {
                    return Err("Too short for DistanceFSS header".to_string());
                }
                let role = bytes[offset] != 0;
                offset += 1;
                let key_count =
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, used_k) =
                        DistanceFSSKey::from_bytes(&bytes[offset..], modulus).unwrap();
                    keys.push(k);
                    offset += used_k;
                }
                Ok((SharedRange::DistanceFSS { keys, role }, offset))
            }
            _ => Err("Unknown SharedRange tag".to_string()),
        }
    }

    pub fn role(&self) -> bool {
        match self {
            SharedRange::OKVS { role, .. } => *role,
            SharedRange::IntervalFSS { role, .. } => *role,
            SharedRange::DistanceFSS { role, .. } => *role,
        }
    }
}
