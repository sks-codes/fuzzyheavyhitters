use crate::data_structures::{
    modp::{BarrettCtx, Modp},
    mod2k::Mod2k,
};
use anyhow::{anyhow, Result};

// Precompute helper vectors for sketches. Payload helpers store per-component
// full-domain values for case0 and case1; they are converted to Modp on demand.
#[derive(Clone, Debug)]
pub enum SketchHelper {
    Dcf {
        barrett_ctx: BarrettCtx,
        inv_value: u128,
    },
    IntervalFSS {
        sketch_helper_dcf: Box<SketchHelper>, // DCF
    },
    DistanceFSS {
        sketch_helper_dcf: Box<SketchHelper>, // DCF helper
        payload_helper_ldcf0: Vec<Vec<Mod2k>>, // Rescale from reference vector of ldcf0
        payload_helper_ldcf1: Vec<Vec<Mod2k>>, // Rescale from reference vector of ldcf1
        payload_helper_rdcf0: Vec<Vec<Mod2k>>, // Rescale from reference vector of rdcf0
        payload_helper_rdcf1: Vec<Vec<Mod2k>>, // Rescale from reference vector of rdcf1
    },
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> Result<&BarrettCtx> {
        match self {
            SketchHelper::Dcf { barrett_ctx, .. } => Ok(barrett_ctx),
            SketchHelper::IntervalFSS {
                sketch_helper_dcf
            } => sketch_helper_dcf.barrett_ctx(),
            SketchHelper::DistanceFSS {
                sketch_helper_dcf, ..
            } => sketch_helper_dcf.barrett_ctx(),
        }
    }

    pub fn get_helper_vector<'a>(
        &self,
        ctx: &'a BarrettCtx,
        domain_size: usize,
    ) -> Result<(Vec<Modp<'a>>, Vec<Modp<'a>>)> {
        match self {
            SketchHelper::Dcf { inv_value, .. } => Ok(
                (
                vec![Modp::one(ctx); domain_size],
                vec![Modp::new(ctx, *inv_value); domain_size],
                )
            ),
            SketchHelper::IntervalFSS { .. }
            | SketchHelper::DistanceFSS { .. } =>
                Err(anyhow!("Only support get_helper_vector for SketchHelper::Dcf")),
        }
    }

    pub fn get_payload_helper_ldcf0(
        &self,
    ) -> Result<Vec<Vec<Mod2k>>> {
        match self {
            SketchHelper::DistanceFSS { payload_helper_ldcf0, .. } => Ok(payload_helper_ldcf0.clone()),
            _ => Err(anyhow!("Only support get_payload_helper_ldcf0 for SketchHelper::DistanceFSS")),
        }
    }

    pub fn get_payload_helper_ldcf1(
        &self,
    ) -> Result<Vec<Vec<Mod2k>>> {
        match self {
            SketchHelper::DistanceFSS { payload_helper_ldcf1, .. } => Ok(payload_helper_ldcf1.clone()),
            _ => Err(anyhow!("Only support get_payload_helper_ldcf1 for SketchHelper::DistanceFSS")),
        }
    }

    pub fn get_payload_helper_rdcf0(
        &self,
    ) -> Result<Vec<Vec<Mod2k>>> {
        match self {
            SketchHelper::DistanceFSS { payload_helper_rdcf0, .. } => Ok(payload_helper_rdcf0.clone()),
            _ => Err(anyhow!("Only support get_payload_helper_rdcf0 for SketchHelper::DistanceFSS")),
        }
    }

    pub fn get_payload_helper_rdcf1(
        &self,
    ) -> Result<Vec<Vec<Mod2k>>> {
        match self {
            SketchHelper::DistanceFSS { payload_helper_rdcf1, .. } => Ok(payload_helper_rdcf1.clone()),
            _ => Err(anyhow!("Only support get_payload_helper_rdcf1 for SketchHelper::DistanceFSS")),
        }
    }
}
