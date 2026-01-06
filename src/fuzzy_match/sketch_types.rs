use crate::data_structures::modp::Modp;
use crate::{
    configs::cli_config::ProtocolParameters,
    fuzzy_match::share_types::{DictionaryType, DistanceMetric, ShareMethod},
};

pub(crate) type TripleModp<'a> = (Modp<'a>, Modp<'a>, Modp<'a>);


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