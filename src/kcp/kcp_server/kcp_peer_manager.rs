use super::kcp_peer::KcpPeer;
use ahash::AHashMap;
use std::cell::UnsafeCell;
use std::collections::hash_map::{Keys, Values};
use std::sync::Arc;

pub struct KcpPeerManager<S> {
    pub kcp_peers: UnsafeCell<AHashMap<u32, Arc<KcpPeer<S>>>>,
}

unsafe impl<S> Send for KcpPeerManager<S> {}
unsafe impl<S> Sync for KcpPeerManager<S> {}

impl<S: Send> KcpPeerManager<S> {
    pub fn new() -> KcpPeerManager<S> {
        KcpPeerManager {
            kcp_peers: UnsafeCell::new(AHashMap::new()),
        }
    }

    pub fn values(&self) -> Values<'_, u32, Arc<KcpPeer<S>>> {
        unsafe { (*self.kcp_peers.get()).values() }
    }

    pub fn keys(&self) -> Keys<'_, u32, Arc<KcpPeer<S>>> {
        unsafe { (*self.kcp_peers.get()).keys() }
    }

    pub fn get(&self, conv: &u32) -> Option<Arc<KcpPeer<S>>> {
        let peers = self.kcp_peers.get();
        unsafe {
            if let Some(value) = (*peers).get(conv) {
                return Some(value.clone());
            }
        }
        None
    }

    pub fn insert(&self, conv: u32, peer: Arc<KcpPeer<S>>) -> Option<Arc<KcpPeer<S>>> {
        unsafe { (*self.kcp_peers.get()).insert(conv, peer) }
    }

    pub fn remove(&self, conv: &u32) -> Option<Arc<KcpPeer<S>>> {
        unsafe { (*self.kcp_peers.get()).remove(conv) }
    }
}
