use crate::{
    configs::cli_config::ProtocolParameters, fuzzy_match::share_types::{DictionaryType, DistanceMetric, ShareMethod}
};

#[derive(Debug, Clone)]
pub struct SketchConfig {
    pub h1: usize, // FSS phase input bit length
    pub h2: usize, // FSS phase output bit length
    pub q: u128, // sketching values will be in Zq
    pub delta: u128, // Distance threshold
    pub d: usize, // Number of dimensions
    pub method: ShareMethod, // The sharing method used. Only can sketch for FSS now
    pub metric: DistanceMetric, // Distance metric. Can support sketching both Linf and Lp
    pub dictionary_type: DictionaryType, // Known or Unknown
}

impl From<ProtocolParameters> for SketchConfig {
    fn from(_config: ProtocolParameters) -> Self {
        unimplemented!()
    }
}

pub enum SketchValues<'a> {
    Dcf {
        last_layer: (Modp<'a>, Modp<'a>),
        consistency: Vec<(Modp<'a>, Modp<'a>)>,
    },
    Linf {
        ldcf: Box<SketchHelper>, // SketchHelper::Dcf for LDCF
        rdcf: Box<SketchHelper>, // SketchHelper::Dcf for RDCF
        consistency: Modp<'a>,
    },
    DcfPayload {
        length: usize,
        last_layer: Vec<(Modp<'a>, Modp<'a>)>,
        last_layer_consistency: Vec<Modp<'a>>, // TRICKY!!! Currently only work if one of the payload is constant.
        consistency: Vec<Vec<(Modp<'a>, Modp<'a>)>>,
    },
    Lp {
        p: usize,
        ldcf0: Box<SketchHelper>, // SketchHelper::DcfPayload for LDCF
        ldcf1: Box<SketchHelper>, // SketchHelper::DcfPayload for LDCF
        rdcf0: Box<SketchHelper>, // SketchHelper::DcfPayload for RDCF
        rdcf1: Box<SketchHelper>, // SketchHelper::DcfPayload for RDCF
        consistency: Modp<'a>,
    },
}