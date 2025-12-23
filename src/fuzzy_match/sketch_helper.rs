// Precompute some transformations for vectors, such as scaling matrices
// Store in u128 first. Will load to the correct modulo later
pub enum SketchHelper {
    Linf {
        dpf: Vec<Vec<u128>>,
    },
    Lp {

    },
}