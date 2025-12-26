use crate::data_structures::modp::{BarrettCtx, Modp};

// Precompute helper vectors for sketches. Payload helpers store per-component
// full-domain values for case0 and case1; they are converted to Modp on demand.
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
        sketch_helper_ldcf: Box<SketchHelper>, // ldcf
        sketch_helper_rdcf: Box<SketchHelper>, // rdcf
    },
    DistanceFSSPayload {
        sketch_helper_ldcf0: Box<SketchHelper>, // ldcf_payload
        sketch_helper_ldcf1: Box<SketchHelper>, // ldcf_payload
        sketch_helper_rdcf0: Box<SketchHelper>, // rdcf_payload
        sketch_helper_rdcf1: Box<SketchHelper>, // rdcf_payload
    },
    /// Payload helpers store per-component full-domain values for case0 and case1.
    /// Outer vec indexes component, inner vec indexes position within the domain.
    LdcfPayload {
        barrett_ctx: BarrettCtx,
        cases0_full: Vec<Vec<u128>>,
        cases1_full: Vec<Vec<u128>>,
    },
    RdcfPayload {
        barrett_ctx: BarrettCtx,
        cases0_full: Vec<Vec<u128>>,
        cases1_full: Vec<Vec<u128>>,
    },
}

#[derive(Clone)]
pub enum HelperVector<'a> {
    Scalar(Vec<Modp<'a>>, Vec<Modp<'a>>),
    Multi {
        case0: Vec<Vec<Modp<'a>>>,
        case1: Vec<Vec<Modp<'a>>>,
    },
}

impl<'a> HelperVector<'a> {
    pub fn component_count(&self) -> usize {
        match self {
            HelperVector::Scalar(..) => 1,
            HelperVector::Multi { case0, .. } => case0.len(),
        }
    }

    pub fn component(&self, idx: usize) -> (Vec<Modp<'a>>, Vec<Modp<'a>>) {
        match self {
            HelperVector::Scalar(case0, case1) => {
                assert!(idx == 0);
                (case0.clone(), case1.clone())
            }
            HelperVector::Multi { case0, case1 } => {
                assert!(idx < case0.len());
                (case0[idx].clone(), case1[idx].clone())
            }
        }
    }
}

fn helper_vector_from_cases<'a>(
    barrett_ctx: &'a BarrettCtx,
    case0_full: &'a [u128],
    case1_full: &'a [u128],
    level: usize,
) -> (Vec<Modp<'a>>, Vec<Modp<'a>>) {
    let domain_size = 1usize << level;
    let slice_end0 = case0_full.len().min(domain_size);
    let slice_end1 = case1_full.len().min(domain_size);
    let mut case0: Vec<Modp<'a>> = case0_full[..slice_end0]
        .iter()
        .map(|x| Modp::new(barrett_ctx, *x))
        .collect();
    let mut case1: Vec<Modp<'a>> = case1_full[..slice_end1]
        .iter()
        .map(|x| Modp::new(barrett_ctx, *x))
        .collect();
    case0.resize(domain_size, Modp::zero(barrett_ctx));
    case1.resize(domain_size, Modp::zero(barrett_ctx));
    (case0, case1)
}

fn helper_vector_multi_from_components<'a>(
    barrett_ctx: &'a BarrettCtx,
    cases0_full: &'a [Vec<u128>],
    cases1_full: &'a [Vec<u128>],
    level: usize,
) -> HelperVector<'a> {
    assert!(
        cases0_full.len() == cases1_full.len(),
        "payload helper component count mismatch"
    );
    let mut case0 = Vec::with_capacity(cases0_full.len());
    let mut case1 = Vec::with_capacity(cases1_full.len());
    for (component0_full, component1_full) in cases0_full.iter().zip(cases1_full.iter()) {
        let (c0, c1) = helper_vector_from_cases(barrett_ctx, component0_full, component1_full, level);
        case0.push(c0);
        case1.push(c1);
    }
    HelperVector::Multi { case0, case1 }
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> &BarrettCtx {
        match self {
            SketchHelper::Ldcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::Rdcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::LdcfPayload { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::RdcfPayload { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::IntervalFSS { sketch_helper_ldcf, .. } => sketch_helper_ldcf.barrett_ctx(),
            SketchHelper::DistanceFSSPayload { sketch_helper_ldcf0, .. } => {
                sketch_helper_ldcf0.barrett_ctx()
            }
        }
    }

    pub fn get_helper_vector(&self, level: usize) -> HelperVector<'_> {
        match self {
            SketchHelper::Ldcf { .. } | SketchHelper::Rdcf { .. } => {
                self.get_helper_vector_dcf(level)
            }
            SketchHelper::LdcfPayload { .. } => self.get_helper_vector_ldcf_payload(level),
            SketchHelper::RdcfPayload { .. } => self.get_helper_vector_rdcf_payload(level),
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

    fn get_helper_vector_dcf(&self, level: usize) -> HelperVector<'_> {
        let inv_value = self.inv_value();
        let domain_size = 1usize << level;
        let barrett_ctx = self.barrett_ctx();
        HelperVector::Scalar(
            vec![Modp::one(barrett_ctx); domain_size],
            vec![Modp::new(barrett_ctx, inv_value); domain_size],
        )
    }

    fn get_helper_vector_ldcf_payload(&self, level: usize) -> HelperVector<'_> {
        match self {
            SketchHelper::LdcfPayload {
                barrett_ctx,
                cases0_full,
                cases1_full,
            } => helper_vector_multi_from_components(barrett_ctx, cases0_full, cases1_full, level),
            _ => panic!("get_helper_vector_ldcf_payload called on wrong helper type"),
        }
    }

    fn get_helper_vector_rdcf_payload(&self, level: usize) -> HelperVector<'_> {
        match self {
            SketchHelper::RdcfPayload {
                barrett_ctx,
                cases0_full,
                cases1_full,
            } => helper_vector_multi_from_components(barrett_ctx, cases0_full, cases1_full, level),
            _ => panic!("get_helper_vector_rdcf_payload called on wrong helper type"),
        }
    }
}
