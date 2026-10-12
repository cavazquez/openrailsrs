//! Immutable mesh sharing. The index contains weak handles, never asset owners.
use bevy::prelude::*;
use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
type Entry = Weak<bevy::asset::StrongHandle>;
fn pool() -> &'static Mutex<HashMap<(u64, u64), Vec<Entry>>> {
    static P: OnceLock<Mutex<HashMap<(u64, u64), Vec<Entry>>>> = OnceLock::new();
    P.get_or_init(Default::default)
}
fn key(mesh: &Mesh) -> Option<(u64, u64)> {
    if mesh.try_has_morph_targets().ok()? {
        return None;
    }
    let mut h = 0xcbf29ce484222325u64;
    let mut len = 0u64;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        len += bytes.len() as u64;
    };
    feed(format!("{:?}{:?}", mesh.primitive_topology(), mesh.asset_usage).as_bytes());
    for (attribute, values) in mesh.try_attributes().ok()? {
        feed(format!("{:?}{:?}", attribute.id, attribute.format).as_bytes());
        feed(values.get_bytes());
    }
    if let Some(indices) = mesh.indices() {
        feed(b"indices");
        for i in indices.iter() {
            feed(&(i as u64).to_le_bytes());
        }
    }
    Some((h, len))
}
fn equivalent(a: &Mesh, b: &Mesh) -> bool {
    if a.primitive_topology() != b.primitive_topology() || a.asset_usage != b.asset_usage {
        return false;
    }
    let (Ok(aa), Ok(bb)) = (a.try_attributes(), b.try_attributes()) else {
        return false;
    };
    let aa: Vec<_> = aa.collect();
    let bb: Vec<_> = bb.collect();
    aa.len() == bb.len()
        && aa.iter().zip(&bb).all(|((a, av), (b, bv))| {
            a.id == b.id && a.format == b.format && av.get_bytes() == bv.get_bytes()
        })
        && match (a.indices(), b.indices()) {
            (Some(a), Some(b)) => a.iter().eq(b.iter()),
            (None, None) => true,
            _ => false,
        }
}
pub fn add(assets: &mut Assets<Mesh>, mesh: Mesh) -> Handle<Mesh> {
    let Some(key) = key(&mesh) else {
        return assets.add(mesh);
    };
    let mut cache = pool().lock().unwrap_or_else(|e| e.into_inner());
    static ADDS: AtomicU64 = AtomicU64::new(0);
    if ADDS.fetch_add(1, Ordering::Relaxed).is_multiple_of(128) || cache.len() > 32768 {
        cache.retain(|_, refs| {
            refs.retain(|r| r.strong_count() > 0);
            !refs.is_empty()
        });
        if cache.len() > 32768 {
            cache.clear();
        }
    }
    if let Some(refs) = cache.get_mut(&key) {
        refs.retain(|r| r.strong_count() > 0);
    }
    let candidate = cache.get(&key).and_then(|refs| {
        refs.iter()
            .filter_map(Weak::upgrade)
            .map(Handle::Strong)
            .find(|h| {
                assets
                    .get(h)
                    .is_some_and(|existing| equivalent(existing, &mesh))
            })
    });
    if let Some(candidate) = candidate
        && let Some(handle) = assets.get_strong_handle(candidate.id())
    {
        // Acquire through this Assets collection; another App can reuse an ID.
        if let Handle::Strong(ref strong) = handle {
            cache
                .entry(key)
                .or_default()
                .push(std::sync::Arc::downgrade(strong));
        }
        return handle;
    }
    let handle = assets.add(mesh);
    if let Handle::Strong(ref strong) = handle {
        cache
            .entry(key)
            .or_default()
            .push(std::sync::Arc::downgrade(strong));
    }
    handle
}
pub fn report() -> serde_json::Value {
    let cache = pool().lock().unwrap_or_else(|e| e.into_inner());
    serde_json::json!({"keys":cache.len(),"live_references":cache.values().flatten().filter(|r|r.strong_count()>0).count(),"weak_index":true,"limit":32768})
}
pub fn owns(handle: &Handle<Mesh>) -> bool {
    let Handle::Strong(strong) = handle else {
        return false;
    };
    pool()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .flatten()
        .any(|r| r.ptr_eq(&std::sync::Arc::downgrade(strong)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_meshes_share_only_geometry_and_do_not_cross_asset_stores() {
        let mut assets = Assets::<Mesh>::default();
        let a = add(&mut assets, Mesh::from(Cuboid::default()));
        let b = add(&mut assets, Mesh::from(Cuboid::default()));
        assert_eq!(a.id(), b.id());
        assert_eq!(assets.len(), 1);
        let different = add(&mut assets, Mesh::from(Cuboid::new(2., 1., 1.)));
        assert_ne!(a.id(), different.id());
        let mut other = Assets::<Mesh>::default();
        let othermesh = add(&mut other, Mesh::from(Cuboid::new(8., 1., 1.)));
        let target = add(&mut other, Mesh::from(Cuboid::default()));
        assert_ne!(othermesh.id(), target.id());
        assert_eq!(other.len(), 2);
        let weak = if let Handle::Strong(s) = &a {
            std::sync::Arc::downgrade(s)
        } else {
            unreachable!()
        };
        drop(a);
        assert!(weak.upgrade().is_none());
        assert!(assets.contains(b.id()));
        let last = if let Handle::Strong(s) = &b {
            std::sync::Arc::downgrade(s)
        } else {
            unreachable!()
        };
        drop(b);
        assert!(last.upgrade().is_none());
    }
}
