use super::*;

/// GUI-WGPU-PRESENT-1: LRU eviction honours both the entry-count limit and
/// the byte budget, always keeping the freshly admitted entry.
#[test]
fn pool_core_admits_touches_and_evicts_lru() {
    let mut core = PoolCore::new(2, u64::MAX);
    // 4x4 entry = 4*4*(4+2) = 96 bytes.
    assert_eq!(PoolCore::entry_bytes(4, 4), 96);
    assert!(core.admit((0, 0)).is_empty());
    assert!(core.admit((0, 1)).is_empty());
    // Touch (0,0) so (0,1) becomes the LRU entry.
    core.touch(&(0, 0));
    let evicted = core.admit((0, 2));
    assert_eq!(evicted, vec![(0, 1)], "the least-recently-used entry goes");
    assert!(core.contains(&(0, 0)));
    assert!(!core.contains(&(0, 1)));
    assert!(core.contains(&(0, 2)));

    // Re-admitting an existing key is a caller bug; touch is the API.
    core.touch(&(0, 2));
    assert_eq!(core.len(), 2);
}

/// Byte-budget eviction: entries are dropped until resident bytes fit,
/// but a lone oversized entry is never evicted by itself.
#[test]
fn pool_core_budget_eviction_keeps_last_entry() {
    // All three keys cost exactly 96 bytes (w*h*6).
    assert_eq!(PoolCore::entry_bytes(4, 4), PoolCore::entry_bytes(8, 2));
    assert_eq!(PoolCore::entry_bytes(4, 4), PoolCore::entry_bytes(16, 1));
    let mut core = PoolCore::new(8, 200);
    core.admit((4, 4));
    core.admit((8, 2));
    assert_eq!(core.len(), 2);
    assert_eq!(core.resident_bytes, 192);
    // This admission pushes past the budget → evict the LRU entry.
    let evicted = core.admit((16, 1));
    assert_eq!(evicted, vec![(4, 4)]);
    assert_eq!(core.resident_bytes, 192);

    // A single huge entry stays (must render) even over budget…
    let mut solo = PoolCore::new(8, 10);
    let evicted = solo.admit((1000, 1));
    assert!(evicted.is_empty(), "the last remaining entry is kept");
    assert_eq!(solo.len(), 1);
}
