use crate::{
    data_structures::modp::{BarrettCtx, Modp},
};

// Precompute some transformations for vectors, such as scaling matrices.
// Store scaling values as u128; convert to Modp on demand.
#[derive(Clone)]
pub enum SketchHelper {
    Ldcf {
        barrett_ctx: BarrettCtx,
        inv_value: u128,
    },
    Rdcf {
        barrett_ctx: BarrettCtx,
        inv_value: u128,
    },
    IntervalFSS {
        sketch_helper_ldcf: Box<SketchHelper>,
        sketch_helper_rdcf: Box<SketchHelper>,
    },
    DistanceFSSPayload {
        sketch_helper_ldcf_payload: Box<SketchHelper>,
        sketch_helper_rdcf_payload: Box<SketchHelper>,
    },
    DistanceLdcfPayload {
        barrett_ctx: BarrettCtx,
        rescale_full: Vec<u128>,
    },
    DistanceRdcfPayload {
        barrett_ctx: BarrettCtx,
        rescale_full: Vec<u128>,
    },
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> &BarrettCtx {
        match self {
            SketchHelper::Ldcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::Rdcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::DistanceLdcfPayload { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::DistanceRdcfPayload { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::IntervalFSS { sketch_helper_ldcf, .. } => sketch_helper_ldcf.barrett_ctx(),
            SketchHelper::DistanceFSSPayload { sketch_helper_ldcf_payload, .. } => {
                sketch_helper_ldcf_payload.barrett_ctx()
            }
        }
    }

    pub fn get_helper_vector(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        match self {
            SketchHelper::Ldcf { .. } | SketchHelper::Rdcf { .. } => {
                self.get_helper_vector_dcf(level)
            }
            SketchHelper::DistanceLdcfPayload { .. } => self.get_helper_vector_ldcf_payload(level),
            SketchHelper::DistanceRdcfPayload { .. } => self.get_helper_vector_rdcf_payload(level),
            SketchHelper::IntervalFSS { .. } | SketchHelper::DistanceFSSPayload { .. } => {
                panic!("Helper vector requested on wrapper helper variant")
            }
        }
    }

    fn inv_value(&self) -> u128 {
        match self {
            SketchHelper::Ldcf { inv_value, .. } => *inv_value,
            SketchHelper::Rdcf { inv_value, .. } => *inv_value,
            _ => panic!("inv_value called on non-DCF helper"),
        }
    }

    fn get_helper_vector_dcf(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        let inv_value = self.inv_value();
        let domain_size = 1usize << level;
        let barrett_ctx = self.barrett_ctx();
        (
            vec![Modp::zero(barrett_ctx); domain_size],
            vec![Modp::new(barrett_ctx, inv_value); domain_size],
        )
    }
}

fn helper_vector_from_rescale(
    barrett_ctx: &BarrettCtx,
    rescale_full: &[u128],
    level: usize,
) -> (Vec<Modp>, Vec<Modp>) {
    let domain_size = 1usize << level;
    let slice_end = rescale_full.len().min(domain_size);
    let base: Vec<Modp> = rescale_full[..slice_end]
        .iter()
        .map(|x| Modp::new(barrett_ctx, x))
        .collect();
    let mut case0 = base.clone();
    case0.resize(domain_size, Modp::zero(barrett_ctx));
    let mut case1 = base;
    case1.resize(domain_size, Modp::zero(barrett_ctx));
    (case0, case1)
}

impl SketchHelper {
    pub fn get_helper_vector_ldcf_payload(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        let (barrett_ctx, rescale_full) = match self {
            SketchHelper::DistanceLdcfPayload { barrett_ctx, rescale_full } => (barrett_ctx, rescale_full),
            _ => panic!("get_helper_vector_ldcf_payload called on wrong helper type"),
        };
        helper_vector_from_rescale(barrett_ctx, rescale_full, level)
    }

    pub fn get_helper_vector_rdcf_payload(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        let (barrett_ctx, rescale_full) = match self {
            SketchHelper::DistanceRdcfPayload { barrett_ctx, rescale_full } => (barrett_ctx, rescale_full),
            _ => panic!("get_helper_vector_rdcf_payload called on wrong helper type"),
        };
        helper_vector_from_rescale(barrett_ctx, rescale_full, level)
    }
}
