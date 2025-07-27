use std::time::Instant;

const N: usize = 6;

#[derive(Clone, Debug)]
struct ArrayVec {
    data: [u128; N],
}

#[derive(Clone, Debug)]
struct VecWrapper {
    data: Vec<u128>,
}

impl ArrayVec {
    fn new(values: [u128; N]) -> Self {
        Self { data: values }
    }
    
    fn get(&self, index: usize) -> u128 {
        self.data[index]
    }
    
    fn set(&mut self, index: usize, value: u128) {
        self.data[index] = value;
    }
    
    fn element_wise_add(&mut self, other: &Self) {
        for i in 0..N {
            self.data[i] = self.data[i].wrapping_add(other.data[i]);
        }
    }
}

impl VecWrapper {
    fn new(values: Vec<u128>) -> Self {
        Self { data: values }
    }
    
    fn get(&self, index: usize) -> u128 {
        self.data[index]
    }
    
    fn set(&mut self, index: usize, value: u128) {
        self.data[index] = value;
    }
    
    fn element_wise_add(&mut self, other: &Self) {
        for i in 0..self.data.len() {
            self.data[i] = self.data[i].wrapping_add(other.data[i]);
        }
    }
}

fn benchmark_array_operations() {
    const ITERATIONS: usize = 1_000_000;
    
    // Array benchmark
    let start = Instant::now();
    let mut arr1 = ArrayVec::new([1, 2, 3, 4, 5, 6]);
    let arr2 = ArrayVec::new([7, 8, 9, 10, 11, 12]);
    
    for _ in 0..ITERATIONS {
        arr1.element_wise_add(&arr2);
        // Prevent optimization
        std::hint::black_box(&arr1);
    }
    let array_time = start.elapsed();
    
    // Vec benchmark
    let start = Instant::now();
    let mut vec1 = VecWrapper::new(vec![1, 2, 3, 4, 5, 6]);
    let vec2 = VecWrapper::new(vec![7, 8, 9, 10, 11, 12]);
    
    for _ in 0..ITERATIONS {
        vec1.element_wise_add(&vec2);
        // Prevent optimization
        std::hint::black_box(&vec1);
    }
    let vec_time = start.elapsed();
    
    println!("Array operations: {:?}", array_time);
    println!("Vec operations: {:?}", vec_time);
    println!("Speedup: {:.2}x", vec_time.as_nanos() as f64 / array_time.as_nanos() as f64);
}

fn main() {
    benchmark_array_operations();
}
