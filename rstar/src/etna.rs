//! ETNA workload properties for the rstar crate.
//!
//! Each `property_<name>` is pure, deterministic, and takes owned concrete
//! inputs. Framework adapters (proptest, quickcheck, crabcheck, hegel) and the
//! `etna_runner` binary all delegate to these functions.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::aabb::AABB;
use crate::params::RTreeParams;
use crate::rtree::RTree;
use crate::Envelope;

/// Three-way property outcome shared by every framework.
#[derive(Debug, Clone)]
pub enum PropertyResult {
    /// The invariant held for these inputs.
    Pass,
    /// The invariant was violated; the string carries a human-readable diagnostic.
    Fail(String),
    /// Inputs are outside the property's intended domain; treat as no-op.
    Discard,
}

fn dist_sq(a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

fn finite(p: &[f64; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

fn dedup_points(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    points.dedup();
    points
}

// ============================================================================
// bulk_load_size_correct — d574780, 0139255a, f93071f, 47efe51
// Invariant: bulk_load(N distinct points) yields a tree with size() == N and
// every input point is found by locate_at_point.
// ============================================================================

/// After `bulk_load(points)` (with all-distinct inputs), the tree size equals
/// the input count and every input point can be found by `locate_at_point`.
///
/// Catches OMT transcription error (d574780), bulk-load infinite recursion
/// (0139255a), and similar correctness regressions in bulk loading.
pub fn property_bulk_load_size_correct(points: Vec<[f64; 2]>) -> PropertyResult {
    if points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    if points.is_empty() {
        return PropertyResult::Discard;
    }
    let unique = dedup_points(points.clone());
    if unique.len() != points.len() {
        // Input contained duplicates (e.g. -0.0 / 0.0). Trees may or may not
        // collapse them; outside this property's domain.
        return PropertyResult::Discard;
    }

    let tree: RTree<[f64; 2]> = RTree::bulk_load(points.clone());
    if tree.size() != points.len() {
        return PropertyResult::Fail(format!(
            "bulk_load size mismatch: tree.size()={}, inputs={}",
            tree.size(),
            points.len()
        ));
    }
    for p in &points {
        if tree.locate_at_point(*p).is_none() {
            return PropertyResult::Fail(format!(
                "bulk_load lost point {:?}: locate_at_point returned None",
                p
            ));
        }
    }
    PropertyResult::Pass
}

// ============================================================================
// nearest_neighbor_correct — 895f8493, b7c66d0
// Invariant: nearest_neighbor(q) returns a point whose distance to q is at
// most the distance from q to every other point in the tree.
// ============================================================================

/// `tree.nearest_neighbor(q)` is at least as close to `q` as every other
/// inserted point. Catches min-max heuristic and floating-point consistency
/// bugs in the nearest-neighbor search.
pub fn property_nearest_neighbor_correct(
    points: Vec<[f64; 2]>,
    query: [f64; 2],
) -> PropertyResult {
    if !finite(&query) || points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    let unique = dedup_points(points);
    if unique.is_empty() {
        return PropertyResult::Discard;
    }
    let tree: RTree<[f64; 2]> = RTree::bulk_load(unique.clone());
    let nn = match tree.nearest_neighbor(query) {
        Some(p) => *p,
        None => {
            return PropertyResult::Fail(
                "nearest_neighbor returned None on a non-empty tree".into(),
            );
        }
    };
    let nn_d = dist_sq(&nn, &query);
    let true_min = unique
        .iter()
        .map(|p| dist_sq(p, &query))
        .fold(f64::INFINITY, f64::min);
    // Use a tiny tolerance so legitimate floating-point reorderings pass.
    if nn_d > true_min * (1.0 + 1e-9) + 1e-12 {
        return PropertyResult::Fail(format!(
            "nearest_neighbor returned {:?} (d^2={}) but min d^2 was {}",
            nn, nn_d, true_min
        ));
    }
    PropertyResult::Pass
}

// ============================================================================
// drain_returns_all — df8f740
// Invariant: drain() yields every inserted point exactly once.
// ============================================================================

/// Tight tree params (MAX_SIZE = 4) used to drive deep trees for the drain
/// witness. With the default MAX_SIZE = 6, even thousands of points produce a
/// shallow tree where the buggy `return self.next()` recursion in
/// `DrainIterator::next` does not overflow the stack.
#[allow(missing_docs)]
pub struct TightParams;
impl RTreeParams for TightParams {
    const MIN_SIZE: usize = 2;
    const MAX_SIZE: usize = 4;
    const REINSERTION_COUNT: usize = 1;
    type DefaultInsertionStrategy = crate::RStarInsertionStrategy;
}

/// Calling `drain()` on a tree built from `points` (with tight params to drive
/// depth) yields every input point exactly once and leaves the tree empty.
/// Catches the recursive-`self.next()` stack-overflow bug.
pub fn property_drain_returns_all(points: Vec<[f64; 2]>) -> PropertyResult {
    if points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    if points.is_empty() {
        return PropertyResult::Discard;
    }
    let unique = dedup_points(points.clone());
    if unique.len() != points.len() {
        return PropertyResult::Discard;
    }
    let mut tree: RTree<[f64; 2], TightParams> = RTree::bulk_load_with_params(points.clone());
    let mut drained: Vec<[f64; 2]> = tree.drain().collect();
    drained.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let mut expected = points;
    expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    if drained != expected {
        return PropertyResult::Fail(format!(
            "drain mismatch: got {} elements, expected {}",
            drained.len(),
            expected.len()
        ));
    }
    if tree.size() != 0 {
        return PropertyResult::Fail(format!(
            "drain left tree non-empty: size()={}",
            tree.size()
        ));
    }
    PropertyResult::Pass
}

// ============================================================================
// min_size_one_no_panic — 44e1bf5
// Invariant: building a tree with RTreeParams MIN_SIZE = 1 does not panic.
// ============================================================================

/// A tree configured with `MIN_SIZE = 1` accepts inserts and lookups without
/// panicking. Catches the `log(1.0)` capacity-calculation panic.
#[allow(missing_docs)]
pub struct MinSizeOneParams;
impl RTreeParams for MinSizeOneParams {
    const MIN_SIZE: usize = 1;
    const MAX_SIZE: usize = 4;
    const REINSERTION_COUNT: usize = 1;
    type DefaultInsertionStrategy = crate::RStarInsertionStrategy;
}

/// Building and using an RTree configured with `MIN_SIZE = 1` does not panic.
pub fn property_min_size_one_no_panic(points: Vec<[f64; 2]>) -> PropertyResult {
    if points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    if points.is_empty() {
        return PropertyResult::Discard;
    }
    let unique = dedup_points(points);
    if unique.is_empty() {
        return PropertyResult::Discard;
    }
    let mut tree: RTree<[f64; 2], MinSizeOneParams> =
        RTree::bulk_load_with_params(unique.clone());
    // Provoke the buggy max_depth calc in remove via a removal call.
    if let Some(first) = unique.first() {
        let _ = tree.remove(first);
    }
    PropertyResult::Pass
}

// ============================================================================
// from_points_contains_all — 7634435
// Invariant: AABB::from_points(P) contains every point in P.
// ============================================================================

/// `AABB::from_points(P)` contains every point in `P`. Catches the bug where
/// `from_points` relied on `new_empty()` returning max/min and silently
/// produced an empty AABB after the new_empty representation was changed.
pub fn property_from_points_contains_all(points: Vec<[f64; 2]>) -> PropertyResult {
    if points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    if points.is_empty() {
        return PropertyResult::Discard;
    }
    let aabb = AABB::<[f64; 2]>::from_points(points.iter());
    for p in &points {
        if !aabb.contains_point(p) {
            return PropertyResult::Fail(format!(
                "AABB::from_points returned {:?} which does not contain input {:?}",
                (aabb.lower(), aabb.upper()),
                p
            ));
        }
    }
    PropertyResult::Pass
}

// ============================================================================
// empty_tree_query_no_panic — 4b44c0346
// Invariant: queries on an empty tree do not panic / overflow.
// ============================================================================

/// Common queries on an empty `RTree<[i64; 2]>` (integer coordinates that
/// would otherwise overflow when computing distance to a max/min-bounded
/// empty AABB) do not panic. Catches the empty-tree overflow regression that
/// the empty-children guards in `iterators.rs` are meant to prevent.
pub fn property_empty_tree_query_no_panic(query: [i64; 2], dist: i64) -> PropertyResult {
    let dist = dist.saturating_abs();
    let tree: RTree<[i64; 2]> = RTree::new();
    // These calls iterate selection functions that previously computed the
    // distance to an empty AABB, overflowing for integer coordinates.
    let _: Vec<_> = tree.locate_within_distance(query, dist).collect();
    let env: AABB<[i64; 2]> = AABB::from_corners([0i64, 0i64], [10i64, 10i64]);
    let _: Vec<_> = tree.locate_in_envelope(env).collect();
    PropertyResult::Pass
}

// ============================================================================
// reinsert_split_size_correct — 47efe51
// Invariant: repeated inserts that trigger reinsert + split keep size() in
// sync with the number of inserted points.
// ============================================================================

/// After a sequence of `insert` calls (which exercise the R*-tree's reinsert
/// path), `tree.size()` equals the count of unique inserted points and every
/// inserted point can be located. Catches the batched-reinsert regression
/// that caused multiple root splits to corrupt size accounting.
pub fn property_reinsert_split_size_correct(points: Vec<[f64; 2]>) -> PropertyResult {
    if points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    if points.is_empty() {
        return PropertyResult::Discard;
    }
    let unique = dedup_points(points);
    if unique.is_empty() {
        return PropertyResult::Discard;
    }
    let mut tree: RTree<[f64; 2]> = RTree::new();
    for p in &unique {
        tree.insert(*p);
    }
    if tree.size() != unique.len() {
        return PropertyResult::Fail(format!(
            "after {} inserts, tree.size() = {}, expected {}",
            unique.len(),
            tree.size(),
            unique.len()
        ));
    }
    for p in &unique {
        if tree.locate_at_point(*p).is_none() {
            return PropertyResult::Fail(format!(
                "after inserts, locate_at_point({:?}) returned None",
                p
            ));
        }
    }
    PropertyResult::Pass
}

// ============================================================================
// locate_in_envelope_correct — covers selection-iterator regressions
// Invariant: locate_in_envelope returns exactly the inserted points whose
// coordinates lie within (or on the border of) the query envelope.
// ============================================================================

/// `tree.locate_in_envelope(&env)` returns exactly the points inside `env`.
/// Sanity property used as cross-check for selection iterators.
pub fn property_locate_in_envelope_correct(
    points: Vec<[f64; 2]>,
    lower: [f64; 2],
    upper: [f64; 2],
) -> PropertyResult {
    if !finite(&lower) || !finite(&upper) || points.iter().any(|p| !finite(p)) {
        return PropertyResult::Discard;
    }
    let env = AABB::from_corners(lower, upper);
    let unique = dedup_points(points);
    let tree: RTree<[f64; 2]> = RTree::bulk_load(unique.clone());

    let mut from_tree: Vec<[f64; 2]> = tree.locate_in_envelope(env).copied().collect();
    from_tree.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let mut expected: Vec<[f64; 2]> = unique
        .iter()
        .filter(|p| env.contains_point(p))
        .copied()
        .collect();
    expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    if from_tree != expected {
        return PropertyResult::Fail(format!(
            "locate_in_envelope: got {} elements, expected {}",
            from_tree.len(),
            expected.len()
        ));
    }
    PropertyResult::Pass
}

// ============================================================================
// Witness tests — concrete, deterministic.
// Each variant has at least one witness; passes on base, fails on variant.
// ============================================================================

#[cfg(test)]
mod witnesses {
    use super::*;
    use alloc::vec;

    fn assert_pass(r: PropertyResult, label: &str) {
        match r {
            PropertyResult::Pass => {}
            PropertyResult::Discard => panic!("witness {label} unexpectedly discarded"),
            PropertyResult::Fail(m) => panic!("witness {label} failed: {m}"),
        }
    }

    // --- bulk_load_size_correct (d574780) ---------------------------------
    /// Witness that exercises the OMT bulk-loading path with enough points
    /// to enter the multi-cluster branch where the ceil/floor mistake bites.
    #[test]
    fn witness_bulk_load_size_correct_case_omt_grid() {
        // 25 points in a 5x5 grid — enough to require multi-axis clustering.
        let mut pts = Vec::with_capacity(25);
        for x in 0..5 {
            for y in 0..5 {
                pts.push([x as f64, y as f64]);
            }
        }
        assert_pass(
            property_bulk_load_size_correct(pts),
            "bulk_load_size_correct/omt_grid",
        );
    }

    // --- bulk_load_size_correct (0139255a) — infinite-recursion case ------
    /// A specific point count where `(n as f32).log(MAX_SIZE)` returns a
    /// value just barely above an integer because of floating-point
    /// imprecision, leading the buggy bulk_load into infinite recursion.
    #[test]
    fn witness_bulk_load_size_correct_case_log_imprecision() {
        // n = MAX_SIZE^k for default MAX_SIZE=6: 6^3 = 216.
        // The buggy calc has log(216, 6) = 3 + tiny eps, ceil = 4, depth - 1 = 3,
        // which when you do (n / max^(depth-1)) ceil yields 1 cluster → infinite loop.
        let mut pts = Vec::with_capacity(216);
        for i in 0..216 {
            // Spread so duplicates are unlikely.
            pts.push([(i as f64) * 0.5, ((i * 7) % 216) as f64 * 0.25]);
        }
        assert_pass(
            property_bulk_load_size_correct(pts),
            "bulk_load_size_correct/log_imprecision",
        );
    }

    // --- nearest_neighbor_correct (895f8493, b7c66d0) ---------------------
    /// Five points at integer coordinates plus a query that lies between
    /// two of them — this exercises the min/max-distance heuristic.
    #[test]
    fn witness_nearest_neighbor_correct_case_grid_query() {
        let pts = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [0.0, 10.0],
            [10.0, 10.0],
            [5.0, 5.0],
        ];
        assert_pass(
            property_nearest_neighbor_correct(pts, [4.0, 4.0]),
            "nearest_neighbor_correct/grid_query",
        );
    }

    /// A ring of 16 points around the origin: the query is the origin so the
    /// answer must be the nearest ring point. This stresses the heuristic
    /// when many candidates have similar bounding-box distances.
    #[test]
    fn witness_nearest_neighbor_correct_case_ring_origin() {
        let mut pts = Vec::new();
        for i in 0..16 {
            let theta = (i as f64) * core::f64::consts::PI / 8.0;
            pts.push([10.0 * theta.cos(), 10.0 * theta.sin()]);
        }
        // Add a closer point — the witness only passes if the search finds it.
        pts.push([1.0, 0.0]);
        assert_pass(
            property_nearest_neighbor_correct(pts, [0.0, 0.0]),
            "nearest_neighbor_correct/ring_origin",
        );
    }

    // --- drain_returns_all (df8f740) --------------------------------------
    /// A 5000-point tree under tight params (MAX_SIZE=4) — depth 6-7 — built
    /// to drive the buggy recursive `self.next()` deeply enough that drain
    /// either overflows the debug stack or, after fix, returns all 5000.
    #[test]
    fn witness_drain_returns_all_case_deep_tree() {
        let mut pts = Vec::with_capacity(5000);
        for i in 0..5000 {
            // Use sequential x so the tree's bounding boxes stay overlapping
            // along that axis, yielding worst-case depth.
            pts.push([i as f64, (i % 17) as f64]);
        }
        assert_pass(
            property_drain_returns_all(pts),
            "drain_returns_all/deep_tree",
        );
    }

    // --- min_size_one_no_panic (44e1bf5) ----------------------------------
    /// Building and removing from a tree configured with MIN_SIZE = 1.
    /// The buggy capacity calculation panics during `remove`'s setup.
    #[test]
    fn witness_min_size_one_no_panic_case_two_points() {
        let pts = vec![[0.0, 0.0], [1.0, 1.0]];
        assert_pass(
            property_min_size_one_no_panic(pts),
            "min_size_one_no_panic/two_points",
        );
    }

    // --- from_points_contains_all (7634435) -------------------------------
    /// Two positive-coordinate points: with the buggy `new_empty()` returning
    /// `(one, zero)` (i.e. `(1, 0)`), the fold's lower/upper start at
    /// (1, 0) and never expands lower past 1, so the AABB excludes (3, 3).
    #[test]
    fn witness_from_points_contains_all_case_positive_pair() {
        let pts = vec![[3.0, 3.0], [4.0, 4.0]];
        assert_pass(
            property_from_points_contains_all(pts),
            "from_points_contains_all/positive_pair",
        );
    }

    /// Two negative-coordinate points: similar reasoning, the buggy init
    /// `(1, 0)` clips the AABB and (-3, -3) is outside.
    #[test]
    fn witness_from_points_contains_all_case_negative_pair() {
        let pts = vec![[-3.0, -3.0], [-4.0, -4.0]];
        assert_pass(
            property_from_points_contains_all(pts),
            "from_points_contains_all/negative_pair",
        );
    }

    // --- empty_tree_query_no_panic (4b44c0346) ----------------------------
    /// The motivating regression test: locate_within_distance on an empty
    /// integer-coordinate tree previously panicked computing distance to
    /// the max/min-bounded empty AABB.
    #[test]
    fn witness_empty_tree_query_no_panic_case_origin() {
        assert_pass(
            property_empty_tree_query_no_panic([0, 0], 10),
            "empty_tree_query_no_panic/origin",
        );
    }

    // --- reinsert_split_size_correct (47efe51) ----------------------------
    /// The original #45 reproducer (3D), reduced to a 2D shape via the y=1080
    /// / y=1060 plane separation that made the original test flake. The
    /// buggy batched reinsert path leaves the tree in an inconsistent state
    /// where size() drifts from the inserted count.
    #[test]
    fn witness_reinsert_split_size_correct_case_bulk_then_insert() {
        let bulk_nodes: Vec<[f64; 3]> = vec![
            [570.0, 1080.0, 89.0], [30.0, 1080.0, 627.0], [1916.0, 1080.0, 68.0],
            [274.0, 1080.0, 790.0], [476.0, 1080.0, 895.0], [1557.0, 1080.0, 250.0],
            [1546.0, 1080.0, 883.0], [1512.0, 1080.0, 610.0], [1729.0, 1080.0, 358.0],
            [1841.0, 1080.0, 434.0], [1752.0, 1080.0, 696.0], [1674.0, 1080.0, 705.0],
            [136.0, 1080.0, 22.0], [1593.0, 1080.0, 71.0], [586.0, 1080.0, 272.0],
            [348.0, 1080.0, 373.0], [502.0, 1080.0, 2.0], [1488.0, 1080.0, 1072.0],
            [31.0, 1080.0, 526.0], [1695.0, 1080.0, 559.0], [1663.0, 1080.0, 298.0],
            [316.0, 1080.0, 417.0], [1348.0, 1080.0, 731.0], [784.0, 1080.0, 126.0],
            [225.0, 1080.0, 847.0], [79.0, 1080.0, 819.0], [320.0, 1080.0, 504.0],
            [1714.0, 1080.0, 1026.0], [264.0, 1080.0, 229.0], [108.0, 1080.0, 158.0],
            [1665.0, 1080.0, 604.0], [496.0, 1080.0, 231.0], [1813.0, 1080.0, 865.0],
            [1200.0, 1080.0, 326.0], [1661.0, 1080.0, 818.0], [135.0, 1080.0, 229.0],
            [424.0, 1080.0, 1016.0], [1708.0, 1080.0, 791.0], [1626.0, 1080.0, 682.0],
            [442.0, 1080.0, 895.0],
        ];
        let extras: Vec<[f64; 3]> = vec![
            [1916.0, 1060.0, 68.0], [1664.0, 1060.0, 298.0], [1594.0, 1060.0, 71.0],
            [225.0, 1060.0, 846.0], [1841.0, 1060.0, 434.0], [502.0, 1060.0, 2.0],
            [1625.5852, 1060.0122, 682.0], [1348.5273, 1060.0029, 731.08124],
            [316.36127, 1060.0298, 418.24515], [1729.3253, 1060.0023, 358.50134],
        ];
        let mut tree: RTree<[f64; 3]> = RTree::bulk_load(bulk_nodes.clone());
        for p in &extras {
            tree.insert(*p);
        }
        let total_inserted = bulk_nodes.len() + extras.len();
        assert_eq!(
            tree.size(),
            total_inserted,
            "size after bulk_load + inserts"
        );
        for p in bulk_nodes.iter().chain(extras.iter()) {
            assert!(
                tree.locate_at_point(*p).is_some(),
                "missing point {:?} after bulk_load + inserts",
                p
            );
        }
    }

    // --- locate_in_envelope_correct ---------------------------------------
    /// Sanity witness: 9 points in a 3x3 grid, query box covers the central
    /// 4 points only.
    #[test]
    fn witness_locate_in_envelope_correct_case_grid_center() {
        let mut pts = Vec::with_capacity(9);
        for x in 0..3 {
            for y in 0..3 {
                pts.push([x as f64, y as f64]);
            }
        }
        assert_pass(
            property_locate_in_envelope_correct(pts, [0.5, 0.5], [2.5, 2.5]),
            "locate_in_envelope_correct/grid_center",
        );
    }
}

