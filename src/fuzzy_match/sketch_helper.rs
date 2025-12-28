use crate::data_structures::modp::{BarrettCtx, Modp};

// Precompute helper vectors for sketches. Payload helpers store per-component
// full-domain values for case0 and case1; they are converted to Modp on demand.
#[derive(Clone, Debug)]
pub enum SketchHelper {
    Dcf {
        barrett_ctx: BarrettCtx,
        inv_value: u128,
    },
    DcfPayload {
        barrett_ctx: BarrettCtx,
        cases0_full: Vec<Vec<u128>>,
        cases1_full: Vec<Vec<u128>>,
    },
    IntervalFSS {
        sketch_helper_ldcf: Box<SketchHelper>, // DCF
        sketch_helper_rdcf: Box<SketchHelper>, // DCF
    },
    DistanceFSSPayload {
        sketch_helper_ldcf0: Box<SketchHelper>, // DCF payload helpers
        sketch_helper_ldcf1: Box<SketchHelper>, // DCF payload helpers
        sketch_helper_rdcf0: Box<SketchHelper>, // DCF payload helpers
        sketch_helper_rdcf1: Box<SketchHelper>, // DCF payload helpers
    },
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> &BarrettCtx {
        match self {
            SketchHelper::Dcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::DcfPayload { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::IntervalFSS {
                sketch_helper_ldcf, ..
            } => sketch_helper_ldcf.barrett_ctx(),
            SketchHelper::DistanceFSSPayload {
                sketch_helper_ldcf0,
                ..
            } => sketch_helper_ldcf0.barrett_ctx(),
        }
    }

    pub fn get_helper_vector<'a>(
        &self,
        ctx: &'a BarrettCtx,
        domain_size: usize,
    ) -> (Vec<Modp<'a>>, Vec<Modp<'a>>) {
        match self {
            SketchHelper::Dcf { inv_value, .. } => (
                vec![Modp::one(ctx); domain_size],
                vec![Modp::new(ctx, *inv_value); domain_size],
            ),
            SketchHelper::IntervalFSS { .. }
            | SketchHelper::DistanceFSSPayload { .. }
            | SketchHelper::DcfPayload { .. } => {
                panic!("Helper vector requested on wrapper helper variant")
            }
        }
    }
}
