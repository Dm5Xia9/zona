//! Demo: simulate a 20-node P2P network bootstrapping and stabilising.

use tracing::info;
use zona_p2p_crypto::NodeKeypair;
use zona_p2p_sim::Sim;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let n = 20;
    let mut sim = Sim::new(0xDEAD_BEEF, 0.05); // 5% message drop

    info!("Creating {n} nodes…");
    let ids: Vec<_> = (0..n).map(|_| sim.add_node(NodeKeypair::generate())).collect();

    info!("Bootstrapping: each node introduced to 3 random neighbours…");
    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(1);
    use rand::seq::SliceRandom;
    for i in 0..n {
        let mut candidates: Vec<usize> = (0..n).filter(|&j| j != i).collect();
        candidates.shuffle(&mut rng);
        for &j in candidates.iter().take(3) {
            sim.introduce(ids[i], ids[j]);
        }
    }

    info!("Running 30 delivery steps…");
    sim.run_steps(30);

    info!("Running repair ticks…");
    sim.repair_all();
    sim.run_steps(20);

    let healthy = sim.count_healthy();
    info!("Healthy nodes (≥ 2 greedy slots, 2 referrer_ids): {healthy}/{n}");

    // Routing test.
    let src = ids[0];
    let dst = ids[n - 1];
    let routable = sim.can_route(src, dst);
    info!(%src, %dst, routable, "Routing check src→dst");

    // Partition test.
    info!("Simulating partition: first 10 vs last 10 nodes…");
    sim.partition(&ids[..10], &ids[10..]);
    sim.run_steps(5);
    let still_routes = sim.can_route(ids[0], ids[n - 1]);
    info!("Can route across partition: {still_routes} (expected: false or degraded)");

    info!("Healing partition…");
    sim.heal_partition();
    sim.repair_all();
    sim.run_steps(20);
    let healed = sim.count_healthy();
    info!("Healthy after healing: {healed}/{n}");
}
