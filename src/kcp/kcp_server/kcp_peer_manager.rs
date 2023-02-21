use super::kcp_peer::KcpPeer;
use ahash::AHashMap;
use std::cell::UnsafeCell;
use std::collections::hash_map::{Keys, Values};
use std::sync::Arc;
use std::time::Instant;

pub struct KcpKey {
    pub key: Vec<u8>,
    pub time: Instant,
}

pub struct KcpPeerManager<S> {
    pub kcp_peers: UnsafeCell<AHashMap<u32, Arc<KcpPeer<S>>>>,
    pub kcp_keys: UnsafeCell<AHashMap<u32, KcpKey>>,
}

unsafe impl<S> Send for KcpPeerManager<S> {}
unsafe impl<S> Sync for KcpPeerManager<S> {}

impl<S: Send> KcpPeerManager<S> {
    pub fn new() -> KcpPeerManager<S> {
        KcpPeerManager {
            kcp_peers: UnsafeCell::new(AHashMap::new()),
            kcp_keys: UnsafeCell::new(Default::default()),
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

    pub fn insert_key(&self, conv: u32, key: Vec<u8>) {
        unsafe {
            (*self.kcp_keys.get()).insert(
                conv,
                KcpKey {
                    key,
                    time: Instant::now(),
                },
            );
        }
    }

    pub fn get_key(&self, conv: &u32) -> Option<KcpKey> {
        unsafe { (*self.kcp_keys.get()).remove(conv) }
    }

    pub fn clean_key(&self) {
        unsafe {
            (*self.kcp_keys.get()).retain(|_, k| k.time.elapsed().as_secs() < 10);
        }
    }
}
