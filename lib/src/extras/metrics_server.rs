use std::net::SocketAddr;
use std::sync::Arc;

use nimiq_blockchain_proxy::BlockchainProxy;
use nimiq_consensus::ConsensusProxy;
use nimiq_mempool::mempool::Mempool;
use nimiq_network_interface::network::Network;

pub use nimiq_metrics_server::NimiqTaskMonitor;

pub fn start_metrics_server<TNetwork: Network>(
    addr: SocketAddr,
    blockchain_proxy: BlockchainProxy,
    mempool: Option<Arc<Mempool>>,
    consensus_proxy: ConsensusProxy<TNetwork>,
    network: Arc<nimiq_network_libp2p::Network>,
    task_monitors: &[NimiqTaskMonitor],
) {
    nimiq_metrics_server::start_metrics_server(
        addr,
        blockchain_proxy,
        mempool,
        consensus_proxy,
        network,
        task_monitors,
    );
}
