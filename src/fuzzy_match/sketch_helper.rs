use crate::data_structures::modp::{BarrettCtx, Modp};

// Precompute helper vectors for sketches. Payload helpers store per-component
// full-domain values for case0 and case1; they are converted to Modp on demand.
#[derive(Clone)]
pub enum SketchHelper {
    DCF {
        barrett_ctx: BarrettCtx,
        inv_value: u128,
    },
    IntervalFSS {
        sketch_helper_ldcf: Box<SketchHelper>, // DCF
        sketch_helper_rdcf: Box<SketchHelper>, // DCF
    },
    DistanceFSS {
        sketch_helper_ldcf0: Box<SketchHelper>, // DCF
        sketch_helper_ldcf1: Box<SketchHelper>, // DCF
        sketch_helper_rdcf0: Box<SketchHelper>, // DCF
        sketch_helper_rdcf1: Box<SketchHelper>, // DCF
    },
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> &BarrettCtx {
        match self {
            SketchHelper::DCF { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::IntervalFSS { sketch_helper_ldcf, .. } => sketch_helper_ldcf.barrett_ctx(),
            SketchHelper::DistanceFSSPayload { sketch_helper_ldcf0, .. } => {
                sketch_helper_ldcf0.barrett_ctx()
            }
        }
    }

    pub fn get_helper_vector(&self, domain_size: usize) -> (Vec<Modp<'a>>, Vec<Modp<'a>>) {
        match self {
            SketchHelper::DCF { barrett_ctx, inv_value } => {
                (
                    vec![Modp::one(barrett_ctx); domain_size],
                    vec![Modp::new(barrett_ctx, inv_value); domain_size],
                )
            },
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
}
