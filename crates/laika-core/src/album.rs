//! V30: album ordering (pure). Collections store a `position` per member;
//! these helpers compute the new order for drag moves and sorting.

use std::collections::{HashMap, HashSet};

/// Move `moving` (in their current relative order) to sit immediately
/// before `before`, or at the end when `before` is None or itself moving.
/// Ids not in `order` are ignored.
pub fn reorder(order: &[i64], moving: &[i64], before: Option<i64>) -> Vec<i64> {
    let set: HashSet<i64> = moving.iter().copied().collect();
    let block: Vec<i64> = order
        .iter()
        .copied()
        .filter(|id| set.contains(id))
        .collect();
    let rest: Vec<i64> = order
        .iter()
        .copied()
        .filter(|id| !set.contains(id))
        .collect();
    let at = before
        .filter(|b| !set.contains(b))
        .and_then(|b| rest.iter().position(|&x| x == b))
        .unwrap_or(rest.len());
    let mut out = Vec::with_capacity(order.len());
    out.extend_from_slice(&rest[..at]);
    out.extend(block);
    out.extend_from_slice(&rest[at..]);
    out
}

/// Grid drop onto the cell at `target` (index into the visible `cells`):
/// the dragged photos take that cell's slot. Moving forward they land
/// just after it, moving backward just before it, so any slot — the first
/// cell of a row included — is reachable. Returns (forward, the id to
/// insert before or None for the end); None when the target is itself
/// being moved or out of range.
pub fn drop_before(cells: &[i64], moving: &[i64], target: usize) -> Option<(bool, Option<i64>)> {
    let t = *cells.get(target)?;
    if moving.contains(&t) {
        return None;
    }
    let first_moving = cells.iter().position(|id| moving.contains(id))?;
    let forward = first_moving < target;
    if !forward {
        return Some((false, Some(t)));
    }
    let after = cells[target + 1..]
        .iter()
        .copied()
        .find(|id| !moving.contains(id));
    Some((true, after))
}

/// Sort `ids` by album position; photos outside the album follow in their
/// incoming order (stable).
pub fn sort_by_album(ids: &mut [i64], order: &[i64]) {
    let rank: HashMap<i64, usize> = order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    ids.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_blocks_before_a_target() {
        let order = [1, 2, 3, 4, 5, 6];
        assert_eq!(reorder(&order, &[5], Some(2)), vec![1, 5, 2, 3, 4, 6]);
        // A multi-selection moves together, keeping its own order.
        assert_eq!(reorder(&order, &[6, 2], Some(4)), vec![1, 3, 2, 6, 4, 5]);
        // To the end, or when the target is itself moving.
        assert_eq!(reorder(&order, &[1, 3], None), vec![2, 4, 5, 6, 1, 3]);
        assert_eq!(reorder(&order, &[3], Some(3)), vec![1, 2, 4, 5, 6, 3]);
        // Unknown ids are ignored; nothing lost or duplicated.
        let out = reorder(&order, &[9, 2], Some(1));
        assert_eq!(out, vec![2, 1, 3, 4, 5, 6]);
    }

    #[test]
    fn two_hundred_reorders_stay_a_permutation() {
        let mut order: Vec<i64> = (1..=200).collect();
        let mut seed = 42u32;
        for _ in 0..500 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let a = (seed >> 8) as i64 % 200 + 1;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let b = (seed >> 8) as i64 % 201;
            order = reorder(&order, &[a, (a % 200) + 1], (b > 0).then_some(b));
        }
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (1..=200).collect::<Vec<_>>());
    }

    #[test]
    fn grid_drops_take_the_target_slot() {
        // Three columns: [1 2 3 | 4 5 6 | 7 8 9].
        let cells: Vec<i64> = (1..=9).collect();
        let place = |moving: &[i64], target: usize| {
            let (_, before) = drop_before(&cells, moving, target).unwrap();
            reorder(&cells, moving, before)
        };
        // Forward onto the first cell of row 2 / row 3: it lands there.
        assert_eq!(place(&[1], 3)[3], 1);
        assert_eq!(place(&[2], 6)[6], 2);
        // Backward onto the first cell of a row, and onto the very first.
        assert_eq!(place(&[9], 3)[3], 9);
        assert_eq!(place(&[5], 0)[0], 5);
        // Onto the last cell: the end.
        assert_eq!(place(&[1], 8)[8], 1);
        // A block moving forward starts at the target slot.
        let out = place(&[1, 2], 6);
        assert_eq!(&out[5..7], &[1, 2]);
        assert!(drop_before(&cells, &[4], 3).is_none());
        assert!(drop_before(&cells, &[4], 20).is_none());
    }

    #[test]
    fn sorting_puts_non_members_last_stably() {
        let mut ids = vec![10, 3, 7, 1, 8];
        sort_by_album(&mut ids, &[7, 1, 3]);
        assert_eq!(ids, vec![7, 1, 3, 10, 8]);
    }
}
