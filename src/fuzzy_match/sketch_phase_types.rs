use crate::{
    data_structures::modp::{Modp, BarrettCtx},
    configs::cli_config::ProtocolParameters,
    fuzzy_match::share_phase_types::{DictionaryType, DistanceMetric, ShareMethod},
};
use anyhow::{anyhow, Result, ensure};
use std::convert::TryInto;

pub type TripleModp<'a> = (Modp<'a>, Modp<'a>, Modp<'a>);
pub type TripleOwned = (u128, u128, u128);

#[derive(Debug, Clone)]
pub struct SketchConfig {
    pub h1: usize,                       // FSS phase input bit length
    pub h2: usize,                       // FSS phase output bit length
    pub q: u128,                         // sketching values will be in Zq
    pub delta: u128,                     // Distance threshold
    pub d: usize,                        // Number of dimensions
    pub method: ShareMethod,             // The sharing method used. Only can sketch for FSS now
    pub metric: DistanceMetric,          // Distance metric. Can support sketching both Linf and Lp
    pub dictionary_type: DictionaryType, // Known or Unknown
}

impl From<ProtocolParameters> for SketchConfig {
    fn from(config: ProtocolParameters) -> Self {
        let method = match config.share_method.as_str() {
            "OKVS" => ShareMethod::OKVS,
            "FSS" => ShareMethod::FSS,
            other => panic!("Unsupported share method: {}", other),
        };

        let dictionary_type = match config.dictionary_type.as_str() {
            "Known" => DictionaryType::Known,
            "Unknown" => DictionaryType::Unknown,
            other => panic!("Unsupported dictionary type: {}", other),
        };

        let metric = match config.distance_metric.as_str() {
            "Linf" => DistanceMetric::LInfinity,
            "L1" => DistanceMetric::Lp { p: 1 },
            "L2" => DistanceMetric::Lp { p: 2 },
            "L3" => DistanceMetric::Lp { p: 3 },
            other => panic!("Unsupported distance metric: {}", other),
        };

        SketchConfig {
            h1: config.h1,
            h2: config.h2,
            q: config.sketch_modulus,
            delta: config.delta,
            d: config.d,
            method,
            metric,
            dictionary_type,
        }
    }
}

#[derive(Debug)]
pub enum SketchValues<'a> {
    Dcf {
        last_layer_case0: (Modp<'a>, Modp<'a>),
        last_layer_case1: (Modp<'a>, Modp<'a>),
        consistency: Vec<(Modp<'a>, Modp<'a>)>,
    },
    Linf {
        ldcf: Box<SketchValues<'a>>, // SketchValues::Dcf for LDCF
        rdcf: Box<SketchValues<'a>>, // SketchValues::Dcf for RDCF
        consistency: Modp<'a>,
    },
    DcfPayload {
        length: usize,
        last_layer_consistency: Vec<Modp<'a>>, // TRICKY!!! Currently only work if one of the payload is constant.
        consistency: Vec<Vec<(Modp<'a>, Modp<'a>)>>,
    },
    Lp {
        p: usize,
        ldcf0: Box<SketchValues<'a>>, // SketchValues::DcfPayload for LDCF
        ldcf1: Box<SketchValues<'a>>, // SketchValues::DcfPayload for LDCF
        rdcf0: Box<SketchValues<'a>>, // SketchValues::DcfPayload for RDCF
        rdcf1: Box<SketchValues<'a>>, // SketchValues::DcfPayload for RDCF
        reference_dpf_case0: (Modp<'a>, Modp<'a>),
        reference_dpf_case1: (Modp<'a>, Modp<'a>),
    },
}

pub enum SketchData<'a> {
    Dcf {
        /*
        Complete calculation for sketching dcf 
        case0: z_ast, z2_ast
        case1: z_bullet, z2_bullet
        need to compute: (z_ast * z_ast - z2_ast) * (z_bullet * z_bullet - z2_bullet)
        need 3 triples
        Consistency: Need number of triples equal to the number of layers
        */
        z_ast: TripleModp<'a>,
        z_bullet: TripleModp<'a>,
        z: TripleModp<'a>,
        consistency: Vec<TripleModp<'a>>,
    },
    Linf {
        ldcf: Box<SketchData<'a>>, // SketchData::Dcf
        rdcf: Box<SketchData<'a>>, // SketchData::Dcf
        // Shift consistency is linear, no need triple
    },
    DcfPayload {
        length: usize,
        // Only need consistency between layers
        consistency: Vec<Vec<TripleModp<'a>>>,
    },
    Lp {
        p: usize,
        ldcf0: Box<SketchData<'a>>,
        ldcf1: Box<SketchData<'a>>,
        rdcf0: Box<SketchData<'a>>,
        rdcf1: Box<SketchData<'a>>,
        // For reference_dpf
        z_ast: TripleModp<'a>,
        z_bullet: TripleModp<'a>,
        z: TripleModp<'a>,
    }
}

impl<'a> SketchData<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            SketchData::Dcf { z_ast, z_bullet, z, consistency } => {
                out.push(0u8);
                for triple in [z_ast, z_bullet, z].iter() {
                    out.extend_from_slice(&triple.0.to_bytes());
                    out.extend_from_slice(&triple.1.to_bytes());
                    out.extend_from_slice(&triple.2.to_bytes());
                }
                let len = consistency.len() as u32;
                out.extend_from_slice(&len.to_le_bytes());
                for triple in consistency {
                    out.extend_from_slice(&triple.0.to_bytes());
                    out.extend_from_slice(&triple.1.to_bytes());
                    out.extend_from_slice(&triple.2.to_bytes());
                }
            },
            SketchData::Linf { ldcf, rdcf } => {
                out.push(1u8);
                out.extend_from_slice(&ldcf.to_bytes());
                out.extend_from_slice(&rdcf.to_bytes());
            },
            SketchData::DcfPayload { length, consistency } => {
                out.push(2u8);
                let len = *length as u32;
                out.extend_from_slice(&len.to_le_bytes());
                let cons_len = consistency.len() as u32;
                out.extend_from_slice(&cons_len.to_le_bytes());
                for layer_consistency in consistency {
                    for triple in layer_consistency {
                        out.extend_from_slice(&triple.0.to_bytes());
                        out.extend_from_slice(&triple.1.to_bytes());
                        out.extend_from_slice(&triple.2.to_bytes());
                    }
                }
            },
            SketchData::Lp { p, ldcf0, ldcf1, rdcf0, rdcf1, z_ast, z_bullet, z } => {
                out.push(3u8);
                let p_u32 = *p as u32;
                out.extend_from_slice(&p_u32.to_le_bytes());
                out.extend_from_slice(&ldcf0.to_bytes());
                out.extend_from_slice(&ldcf1.to_bytes());
                out.extend_from_slice(&rdcf0.to_bytes());
                out.extend_from_slice(&rdcf1.to_bytes());
                for triple in [z_ast, z_bullet, z].iter() {
                    out.extend_from_slice(&triple.0.to_bytes());
                    out.extend_from_slice(&triple.1.to_bytes());
                    out.extend_from_slice(&triple.2.to_bytes());
                }
            }
        }
        out
    }

    pub fn from_bytes(ctx: &'a BarrettCtx, bytes: &[u8]) -> Result<(Self, usize)> {
        ensure!(!bytes.is_empty(), "Empty byte slice");
        let mut offset = 0;
        let tag = bytes[offset];
        offset += 1;
        match tag {
            0u8 => {
                ensure!(bytes[offset..].len() >= 16 * 9, "Not enough bytes to read SketchData::Dcf triples");
                let (z_ast_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_ast_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_ast_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z_ast = (z_ast_0, z_ast_1, z_ast_2);
                let (z_bullet_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_bullet_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_bullet_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z_bullet = (z_bullet_0, z_bullet_1, z_bullet_2);
                let (z_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z = (z_0, z_1, z_2);
                ensure!(bytes[offset..].len() >= 4, "Not enough bytes to read consistency length");
                let cons_len = u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                ensure!(bytes[offset..].len() >= cons_len * 16 * 3, "Not enough bytes to read SketchData::Dcf consistency triples");
                let mut consistency = Vec::with_capacity(cons_len);
                for _ in 0..cons_len {
                    let (cons_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                    offset += read_bytes;
                    let (cons_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                    offset += read_bytes;
                    let (cons_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                    offset += read_bytes;
                    consistency.push((cons_0, cons_1, cons_2)); 
                }
                Ok((
                    SketchData::Dcf { z_ast, z_bullet, z, consistency },
                    offset,
                ))
            },
            1u8 => {
                let (ldcf, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read ldcf in SketchData::Linf: {}", e))?;
                offset += read_bytes;
                let (rdcf, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read rdcf in SketchData::Linf: {}", e))?;
                offset += read_bytes;
                Ok((
                    SketchData::Linf { 
                        ldcf: Box::new(ldcf), 
                        rdcf: Box::new(rdcf) 
                    }, 
                    offset
                ))
            },
            2u8 => {
                ensure!(bytes[offset..].len() >= 4, "Not enough bytes to read length in SketchData::DcfPayload");
                let length = u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                ensure!(bytes[offset..].len() >= 4, "Not enough bytes to read consistency length in SketchData::DcfPayload");
                let cons_len = u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                let mut consistency = Vec::with_capacity(cons_len);
                for _ in 0..cons_len {
                    let mut layer_consistency = Vec::with_capacity(length);
                    for _ in 0..length {
                        ensure!(bytes[offset..].len() >= 16 * 3, "Not enough bytes to read SketchData::DcfPayload consistency triple");
                        let (cons_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                        offset += read_bytes;
                        let (cons_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                        offset += read_bytes;
                        let (cons_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                        offset += read_bytes;
                        layer_consistency.push((cons_0, cons_1, cons_2));
                    }
                    consistency.push(layer_consistency);
                }
                Ok((
                    SketchData::DcfPayload { length, consistency },
                    offset,
                ))
            },
            3u8 => {
                ensure!(bytes[offset..].len() >= 4, "Not enough bytes to read p in SketchData::Lp");
                let p = u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                let (ldcf0, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read ldcf0 in SketchData::Lp: {}", e))?;
                offset += read_bytes;
                let (ldcf1, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read ldcf1 in SketchData::Lp: {}", e))?;
                offset += read_bytes;
                let (rdcf0, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read rdcf0 in SketchData::Lp: {}", e))?;
                offset += read_bytes;
                let (rdcf1, read_bytes) = SketchData::from_bytes(ctx, &bytes[offset..])
                    .map_err(|e| anyhow!("Failed to read rdcf1 in SketchData::Lp: {}", e))?;
                offset += read_bytes;
                ensure!(bytes[offset..].len() >= 16 * 9, "Not enough bytes to read SketchData::Lp triples");
                let (z_ast_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_ast_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_ast_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z_ast = (z_ast_0, z_ast_1, z_ast_2);
                let (z_bullet_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_bullet_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_bullet_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z_bullet = (z_bullet_0, z_bullet_1, z_bullet_2);
                let (z_0, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_1, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let (z_2, read_bytes) = Modp::from_bytes(ctx, &bytes[offset..])?;
                offset += read_bytes;
                let z = (z_0, z_1, z_2);
                Ok((
                    SketchData::Lp { 
                        p, 
                        ldcf0: Box::new(ldcf0), 
                        ldcf1: Box::new(ldcf1), 
                        rdcf0: Box::new(rdcf0), 
                        rdcf1: Box::new(rdcf1), 
                        z_ast, 
                        z_bullet, 
                        z 
                    },
                    offset,
                ))
            },
            other => Err(anyhow!("Unknown tag {} in SketchData", other)),
        }
    }
} 

#[derive(Debug)]
pub enum VerifyValues<'a> {
    Dcf {
        last_layer: Modp<'a>,
        consistency: Vec<Modp<'a>>,
    },
    Linf {
        ldcf: Box<VerifyValues<'a>>, 
        rdcf: Box<VerifyValues<'a>>,
        consistency: Modp<'a>,
    },
    DcfPayload {
        length: usize,
        last_layer_consistency: Vec<Modp<'a>>,
        consistency: Vec<Vec<Modp<'a>>>,
    },
    Lp {
        p: usize,
        ldcf0: Box<VerifyValues<'a>>,
        ldcf1: Box<VerifyValues<'a>>,
        rdcf0: Box<VerifyValues<'a>>,
        rdcf1: Box<VerifyValues<'a>>,
        reference_dpf: Modp<'a>,
    }
}
