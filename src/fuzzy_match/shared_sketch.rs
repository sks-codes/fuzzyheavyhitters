pub enum SketchMethod {
    IntervalFSS,
    DistanceFSS,
}

pub enum SketchData {
    IntervalFSS {
        // Enable computing z1^4 - z2^2
        a: Vec<u128>,  // Secret share of a
        a2: Vec<u128>, // Secret share of 6a^2
        a3: Vec<u128>, // Secret share of -4a^3
        a4: Vec<u128>, // Secret share of a^4-b^2
        b: Vec<u128>,  // Secret share of b
    },
}

pub fn generate_sketch_data(
    num_clients: usize,
    d: usize,
    sketch_modulus: u128,
    method: &SketchMethod,
) -> (Vec<SketchData>, Vec<SketchData>) {
    match method {
        SketchMethod::IntervalFSS => {
            generate_interval_fss_sketch_data(num_clients, d, sketch_modulus)
        }
        SketchMethod::DistanceFSS => unimplemented!("DistanceFSS sketch not implemented yet"),
    }
}

pub fn generate_interval_fss_sketch_data(
    num_clients: usize,
    d: usize,
    sketch_modulus: u128,
) -> (Vec<SketchData>, Vec<SketchData>) {
    let mut sketch_data_0 = Vec::with_capacity(num_clients);
    let mut sketch_data_1 = Vec::with_capacity(num_clients);
    for _ in 0..num_clients {
        let a = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();

        let mut a2 = a
            .iter()
            .map(|&x| (x * x) % sketch_modulus)
            .collect::<Vec<u128>>();

        let mut a3 = a
            .iter()
            .zip(a2.iter())
            .map(|(&x, &y)| (x * y) % sketch_modulus)
            .collect::<Vec<u128>>();

        let mut a4 = a2
            .iter()
            .map(|&x| (x * x) % sketch_modulus)
            .collect::<Vec<u128>>();

        let b = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();

        let b2 = b
            .iter()
            .map(|&x| (x * x) % sketch_modulus)
            .collect::<Vec<u128>>();

        a2.iter_mut().for_each(|x| *x = (6 * (*x)) % sketch_modulus);
        a3.iter_mut()
            .for_each(|x| *x = (sketch_modulus - ((4 * (*x)) % sketch_modulus)) % sketch_modulus);
        a4.iter_mut()
            .zip(b2.iter())
            .for_each(|(x, &y)| *x = (*x + sketch_modulus - y) % sketch_modulus);

        let a_0 = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();
        let a2_0 = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();
        let a3_0 = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();
        let a4_0 = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();
        let b_0 = (0..2 * d)
            .map(|_| rand::random::<u128>() % sketch_modulus)
            .collect::<Vec<u128>>();

        let a_1 = a
            .iter()
            .zip(a_0.iter())
            .map(|(&x, &y)| (sketch_modulus + x - y) % sketch_modulus)
            .collect::<Vec<u128>>();
        let a2_1 = a2
            .iter()
            .zip(a2_0.iter())
            .map(|(&x, &y)| (sketch_modulus + x - y) % sketch_modulus)
            .collect::<Vec<u128>>();
        let a3_1 = a3
            .iter()
            .zip(a3_0.iter())
            .map(|(&x, &y)| (sketch_modulus + x - y) % sketch_modulus)
            .collect::<Vec<u128>>();
        let a4_1 = a4
            .iter()
            .zip(a4_0.iter())
            .map(|(&x, &y)| (sketch_modulus + x - y) % sketch_modulus)
            .collect::<Vec<u128>>();
        let b_1 = b
            .iter()
            .zip(b_0.iter())
            .map(|(&x, &y)| (sketch_modulus + x - y) % sketch_modulus)
            .collect::<Vec<u128>>();

        sketch_data_0.push(SketchData::IntervalFSS {
            a: a_0,
            a2: a2_0,
            a3: a3_0,
            a4: a4_0,
            b: b_0,
        });
        sketch_data_1.push(SketchData::IntervalFSS {
            a: a_1,
            a2: a2_1,
            a3: a3_1,
            a4: a4_1,
            b: b_1,
        });
    }
    (sketch_data_0, sketch_data_1)
}
