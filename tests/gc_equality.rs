use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
use scuttlebutt::{AesRng, Channel};
use counttree::garbled_circuits::equality::{
    multiple_gb_equality_test, multiple_ev_equality_test,
};

#[test]
fn eq_gc() {
    let gb_value = vec![vec![0,1,1,0], vec![0,0,0,0], vec![1,1,1,0]];
    let ev_value = vec![vec![0,1,1,0], vec![0,0,0,0], vec![1,1,1,0]];
    let expected = gb_value.iter().enumerate().map(|(i, x)| *x == ev_value[i]).collect::<Vec<bool>>();

    let (sender, receiver) = UnixStream::pair().unwrap();

    let (result_sender, result_receiver) = std::sync::mpsc::channel();

    let x = std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        let masks = multiple_gb_equality_test(&mut rng_gb.clone(), &mut channel, gb_value.as_slice());
        result_sender.send(masks).unwrap();
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_equality_test(&mut rng_ev.clone(), &mut channel, ev_value.as_slice());

    let masks = result_receiver.recv().unwrap();
    x.join().unwrap();

    assert_eq!(
        masks.len(),
        results.len(),
        "Masks and results should have the same length"
    );

    for i in 0..results.len() {
        assert_eq!(
            (masks[i] ^ results[i]) as u16,
            expected[i] as u16,
            "The garbled circuit result is incorrect for index {} and should be {}",
            i,
            expected[i]
        );
    }
}
