use crate::data_structures::modp::{BarrettCtx, Modp};


// Precompute some transformations for vectors, such as scaling matrices.
// Store scaling values as tuples of u128; convert to Modp on demand.
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
    // L1 payloads carry two components per point
    LdcfPayloadL1 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128)>,
        case1_full: Vec<(u128, u128)>,
    },
    RdcfPayloadL1 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128)>,
        case1_full: Vec<(u128, u128)>,
    },
    // L2 payloads carry three components per point
    LdcfPayloadL2 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128, u128)>,
        case1_full: Vec<(u128, u128, u128)>,
    },
    RdcfPayloadL2 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128, u128)>,
        case1_full: Vec<(u128, u128, u128)>,
    },
    // L3 payloads carry four components per point
    LdcfPayloadL3 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128, u128, u128)>,
        case1_full: Vec<(u128, u128, u128, u128)>,
    },
    RdcfPayloadL3 {
        barrett_ctx: BarrettCtx,
        case0_full: Vec<(u128, u128, u128, u128)>,
        case1_full: Vec<(u128, u128, u128, u128)>,
    },
}

#[derive(Clone)]
pub enum HelperVector<'a> {
    Scalar(Vec<Modp<'a>>, Vec<Modp<'a>>),
    Pair(Vec<(Modp<'a>, Modp<'a>)>, Vec<(Modp<'a>, Modp<'a>)>),
    Triple(Vec<(Modp<'a>, Modp<'a>, Modp<'a>)>, Vec<(Modp<'a>, Modp<'a>, Modp<'a>)>),
    Quad(
        Vec<(Modp<'a>, Modp<'a>, Modp<'a>, Modp<'a>)>,
        Vec<(Modp<'a>, Modp<'a>, Modp<'a>, Modp<'a>)>,
    ),
}

impl<'a> HelperVector<'a> {
    pub fn component_count(&self) -> usize {
        match self {
            HelperVector::Scalar(..) => 1,
            HelperVector::Pair(..) => 2,
            HelperVector::Triple(..) => 3,
            HelperVector::Quad(..) => 4,
        }
    }

    pub fn component(&self, idx: usize) -> (Vec<Modp<'a>>, Vec<Modp<'a>>) {
        match self {
            HelperVector::Scalar(case0, case1) => {
                assert!(idx == 0);
                (case0.clone(), case1.clone())
            }
            HelperVector::Pair(case0, case1) => {
                assert!(idx < 2);
                (
                    case0.iter().map(|v| if idx == 0 { v.0 } else { v.1 }).collect(),
                    case1.iter().map(|v| if idx == 0 { v.0 } else { v.1 }).collect(),
                )
            }
            HelperVector::Triple(case0, case1) => {
                assert!(idx < 3);
                (
                    case0.iter().map(|v| [v.0, v.1, v.2][idx]).collect(),
                    case1.iter().map(|v| [v.0, v.1, v.2][idx]).collect(),
                )
            }
            HelperVector::Quad(case0, case1) => {
                assert!(idx < 4);
                (
                    case0.iter().map(|v| [v.0, v.1, v.2, v.3][idx]).collect(),
                    case1.iter().map(|v| [v.0, v.1, v.2, v.3][idx]).collect(),
                )
            }
        }
    }
}

impl SketchHelper {
    pub fn barrett_ctx(&self) -> &BarrettCtx {
        match self {
            SketchHelper::Ldcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::Rdcf { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::LdcfPayloadL1 { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::RdcfPayloadL1 { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::LdcfPayloadL2 { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::RdcfPayloadL2 { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::LdcfPayloadL3 { barrett_ctx, .. } => barrett_ctx,
            SketchHelper::RdcfPayloadL3 { barrett_ctx, .. } => barrett_ctx,
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
            SketchHelper::LdcfPayloadL1 { .. } => self.get_helper_vector_ldcf_payload_l1(level),
            SketchHelper::RdcfPayloadL1 { .. } => self.get_helper_vector_rdcf_payload_l1(level),
            SketchHelper::LdcfPayloadL2 { .. } => self.get_helper_vector_ldcf_payload_l2(level),
            SketchHelper::RdcfPayloadL2 { .. } => self.get_helper_vector_rdcf_payload_l2(level),
            SketchHelper::LdcfPayloadL3 { .. } => self.get_helper_vector_ldcf_payload_l3(level),
            SketchHelper::RdcfPayloadL3 { .. } => self.get_helper_vector_rdcf_payload_l3(level),
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

impl SketchHelper {
    pub fn get_helper_vector_ldcf_payload_l1(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::LdcfPayloadL1 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_ldcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b)))
            .collect();
        let case1: Vec<(Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b)))
            .collect();
        HelperVector::Pair(case0, case1)
    }

    pub fn get_helper_vector_rdcf_payload_l1(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::RdcfPayloadL1 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_rdcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b)))
            .collect();
        let case1: Vec<(Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b)))
            .collect();
        HelperVector::Pair(case0, case1)
    }

    pub fn get_helper_vector_ldcf_payload_l2(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::LdcfPayloadL2 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_ldcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b, c)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c)))
            .collect();
        let case1: Vec<(Modp, Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b, c)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c)))
            .collect();
        HelperVector::Triple(case0, case1)
    }

    pub fn get_helper_vector_rdcf_payload_l2(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::RdcfPayloadL2 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_rdcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b, c)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c)))
            .collect();
        let case1: Vec<(Modp, Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b, c)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c)))
            .collect();
        HelperVector::Triple(case0, case1)
    }

    pub fn get_helper_vector_ldcf_payload_l3(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::LdcfPayloadL3 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_ldcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp, Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b, c, d)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c), Modp::new(barrett_ctx, *d)))
            .collect();
        let case1: Vec<(Modp, Modp, Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b, c, d)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c), Modp::new(barrett_ctx, *d)))
            .collect();
        HelperVector::Quad(case0, case1)
    }

    pub fn get_helper_vector_rdcf_payload_l3(&self, level: usize) -> HelperVector<'_> {
        let (barrett_ctx, case0_full, case1_full) = match self {
            SketchHelper::RdcfPayloadL3 { barrett_ctx, case0_full, case1_full } => {
                (barrett_ctx, case0_full, case1_full)
            }
            _ => panic!("get_helper_vector_rdcf_payload called on wrong helper type"),
        };
        let domain_size = 1usize << level;
        let slice0 = case0_full.len().min(domain_size);
        let slice1 = case1_full.len().min(domain_size);
        let case0: Vec<(Modp, Modp, Modp, Modp)> = case0_full[..slice0]
            .iter()
            .map(|(a, b, c, d)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c), Modp::new(barrett_ctx, *d)))
            .collect();
        let case1: Vec<(Modp, Modp, Modp, Modp)> = case1_full[..slice1]
            .iter()
            .map(|(a, b, c, d)| (Modp::new(barrett_ctx, *a), Modp::new(barrett_ctx, *b), Modp::new(barrett_ctx, *c), Modp::new(barrett_ctx, *d)))
            .collect();
        HelperVector::Quad(case0, case1)
    }
}
