use neurofour::{
    arena,
    baseline::{HeuristicAgent, RandomAgent},
    dataset,
    sparse::{Config, SparseNet},
};
#[test]
fn end_to_end_pipeline() {
    let dir = std::env::temp_dir();
    let data = dir.join("neurofour-integration.csv");
    let cp = dir.join("neurofour-integration.ckpt");
    let n = dataset::generate(data.to_str().unwrap(), 12, 7).unwrap();
    assert!(n > 12);
    let samples = dataset::load(data.to_str().unwrap()).unwrap();
    let mut net = SparseNet::new(
        Config {
            max_nodes: 32,
            top_k: 4,
            ..Config::default()
        },
        7,
    );
    let loss = net.train(&samples, 1, 7);
    assert!(loss.is_finite());
    net.save(cp.to_str().unwrap()).unwrap();
    assert!(SparseNet::load(cp.to_str().unwrap()).is_ok());
    let mut a = RandomAgent;
    let mut b = HeuristicAgent;
    let r = arena::play(&mut a, &mut b, 4, 7);
    assert_eq!(r.games, 4);
}
