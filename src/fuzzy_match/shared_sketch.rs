use crate::{
    data_structures::modp::Modp,
};

pub enum SketchMethod {
    IntervalFSS,
    DistanceFSS,
}

pub(crate) type TripleModp<'a> = (Modp<'a>, Modp<'a>, Modp<'a>);

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