# rstar — Injected Bugs

rstar — n-dimensional R*-tree spatial index. ETNA workload mining bug-fix commits in georust/rstar.

Total mutations: 3

## Bug Index

| # | Variant | Name | Location | Injection | Fix Commit |
|---|---------|------|----------|-----------|------------|
| 1 | `bulk_load_clusters_clamp_0139255a_1` | `bulk_load_clusters_clamp` | `rstar/src/algorithm/bulk_load/bulk_load_sequential.rs:31` | `marauders` | `0139255a78ada92277ce0d1025c009254ea5b298` |
| 2 | `empty_tree_iter_no_overflow_4b44c03_1` | `empty_tree_iter_no_overflow` | `rstar/src/algorithm/iterators.rs:58` | `patch` | `4b44c0346c030802d5a092cf9001af303423bce4` |
| 3 | `min_size_one_log_clamp_44e1bf5_1` | `min_size_one_log_clamp` | `rstar/src/algorithm/removal.rs:102` | `marauders` | `44e1bf54192ad96672a465fcf95025180e540c3d` |

## Property Mapping

| Variant | Property | Witness(es) |
|---------|----------|-------------|
| `bulk_load_clusters_clamp_0139255a_1` | `BulkLoadSizeCorrect` | `witness_bulk_load_size_correct_case_log_imprecision` |
| `empty_tree_iter_no_overflow_4b44c03_1` | `EmptyTreeQueryNoPanic` | `witness_empty_tree_query_no_panic_case_origin` |
| `min_size_one_log_clamp_44e1bf5_1` | `MinSizeOneNoPanic` | `witness_min_size_one_no_panic_case_two_points` |

## Framework Coverage

| Property | proptest | quickcheck | crabcheck | hegel |
|----------|---------:|-----------:|----------:|------:|
| `BulkLoadSizeCorrect` | ✓ | ✓ | ✓ | ✓ |
| `EmptyTreeQueryNoPanic` | ✓ | ✓ | ✓ | ✓ |
| `MinSizeOneNoPanic` | ✓ | ✓ | ✓ | ✓ |

## Bug Details

### 1. bulk_load_clusters_clamp

- **Variant**: `bulk_load_clusters_clamp_0139255a_1`
- **Location**: `rstar/src/algorithm/bulk_load/bulk_load_sequential.rs:31` (inside `bulk_load_recursive`)
- **Property**: `BulkLoadSizeCorrect`
- **Witness(es)**:
  - `witness_bulk_load_size_correct_case_log_imprecision` — N = 216 (= 6^3 with default MAX_SIZE = 6) hits the log-imprecision case.
- **Source**: [#166](https://github.com/georust/rstar/pull/166) — Defend against numerical instability when computing number of clusters (#166)
  > OMT bulk loading recursively partitions the input into `number_of_clusters_on_axis` clusters per dimension. For specific input sizes (e.g. `N = MAX_SIZE^k`), the depth calculation `(N as f32).log(MAX_SIZE)` returns `k + eps` instead of `k` exactly; ceil pushes depth one too far, the per-cluster count rounds down to 1, and the recursion never terminates — eventually overflowing the stack.
- **Fix commit**: `0139255a78ada92277ce0d1025c009254ea5b298` — Defend against numerical instability when computing number of clusters (#166)
- **Invariant violated**: `RTree::bulk_load(points)` terminates and returns a tree containing every input point, even at sizes that hit floating-point imprecision in the cluster-count calculation (notably `N = MAX_SIZE^k` for small k).
- **How the mutation triggers**: The mutation removes the `.max(2)` clamp from the cluster count, so when log-imprecision yields `number_of_clusters = 1`, the recursion partitions one input slab into one slab and recurses on the same set; depth grows unbounded and the thread stack overflows.

### 2. empty_tree_iter_no_overflow

- **Variant**: `empty_tree_iter_no_overflow_4b44c03_1`
- **Location**: `rstar/src/algorithm/iterators.rs:58` (inside `SelectionIterator::new`)
- **Property**: `EmptyTreeQueryNoPanic`
- **Witness(es)**:
  - `witness_empty_tree_query_no_panic_case_origin` — Regression test #161: `locate_within_distance` on an empty `RTree<[i64; 3]>` previously panicked.
- **Source**: [#184](https://github.com/georust/rstar/pull/184), [#183](https://github.com/georust/rstar/issues/183) — Revert back to min/max representation of empty AABB (#184)
  > After PR #162 changed `AABB::new_empty()` from `(max_value, min_value)` to `(one, zero)`, downstream code regressed when merging empty AABBs with negative-coordinate envelopes. PR #184 reverts the empty representation but adds a guard inside the selection iterators: when the root has no children, do not call `should_unpack_parent` (which would compute `distance_2` from the query to the empty AABB and overflow integer coordinates).
- **Fix commit**: `4b44c0346c030802d5a092cf9001af303423bce4` — Revert back to min/max representation of empty AABB (#184)
- **Invariant violated**: Selection-based queries on an empty `RTree` (e.g. `locate_within_distance` with integer coordinates) do not arithmetic-overflow when computing the distance from the query to the root's empty AABB.
- **How the mutation triggers**: The patch removes the `!root.children.is_empty()` short-circuit at every entry to `should_unpack_parent`. With an empty tree, the root's AABB is `[i64::MAX..i64::MIN]`; `distance_2` subtracts those (already an overflow) and then squares the difference. Overflow checks (enabled in this workload's release profile) trip a panic; without overflow checks the wrapped distance still differs from the true zero result.

### 3. min_size_one_log_clamp

- **Variant**: `min_size_one_log_clamp_44e1bf5_1`
- **Location**: `rstar/src/algorithm/removal.rs:102` (inside `DrainIterator::new`)
- **Property**: `MinSizeOneNoPanic`
- **Witness(es)**:
  - `witness_min_size_one_no_panic_case_two_points`
- **Source**: [#92](https://github.com/georust/rstar/issues/92) — Fix panic when setting RTreeParams MIN_SIZE to 1
  > DrainIterator::new pre-allocates a node stack with `(N as f32).log(MIN_SIZE as f32)` levels, but `log(_, 1.0)` is undefined and yields NaN, which casts to `usize::MAX`-ish — `Vec::with_capacity` panics with `capacity overflow`. The fix clamps the log base to `MIN_SIZE.max(2)` so a single-element minimum doesn't poison the depth estimate.
- **Fix commit**: `44e1bf54192ad96672a465fcf95025180e540c3d` — Fix panic when setting RTreeParams MIN_SIZE to 1
- **Invariant violated**: Removing an element from an `RTree` configured with `MIN_SIZE = 1` does not panic. The depth-estimate inside `DrainIterator::new` must remain finite for any legal `MIN_SIZE`.
- **How the mutation triggers**: The mutation reverts the clamp `m.max(2)` to plain `m`. With `Params::MIN_SIZE = 1`, `(N as f32).log(1.0)` evaluates to NaN/Inf; the subsequent `as usize` cast saturates to a huge value that `Vec::with_capacity` rejects with `capacity overflow`, panicking on the first call to `tree.remove(_)`.

## Dropped Candidates

- `d574780` (Fix transcription error in implementation of OMT bulk loading (#221)) — no observable public invariant: bug only oversizes internal nodes; tree.size() and locate_at_point still succeed
- `dde5abd` (Fix broken loop break condition) — surface removed: remove_recursive replaced by DrainIterator in 2023
- `84d12654` (Fix overflows applying selection iterators to empty trees by choosing a more tame value for AABB::new_empty (#162)) — fix later reverted in 4b44c0346; the overflow defense at HEAD lives in iterators.rs (covered by 4b44c0346 variant)
- `f93071f` (Fix excessive memory usage in bulk_load due to Vec over-capacity (#220)) — no observable public invariant: bug only inflates allocator pressure; tree behaviour is unchanged
- `9aa17e6` (SelectionIteratorMut should respect should_unpack_leaf) — fix subsumed by 4b44c0346's iterators.rs rewrite; would conflict with empty_tree_iter_no_panic variant
- `df8f740` (Fix stack overflow error in DrainIterator) — no observable public invariant under normal data: recursion depth = tree depth = log(N), too shallow to overflow even in debug mode; the buggy and fixed forms are behaviorally equivalent for any tree built via bulk_load or insert
- `895f8493` (Fix nearest neighbor min-max heuristic) — surface removed: the original `min_max_dist_2` body that constructed `p = min/max` then mutated one component was rewritten in b7c66d0 to a per-axis formulation; the bug site no longer exists at HEAD
- `b7c66d0` (Aabb: fix min_max_dist_2 consistency with distance_2) — no observable public invariant under normal coordinates: the fix is a floating-point order-of-operations adjustment that only manifests at extreme magnitudes (~1e57) where unit tests check `min_max_dist_2 == distance_2`; the public `nearest_neighbor` query still returns the correct point because the heuristic only prunes branches whose lower bound would exclude the true nearest
- `7634435` (Fix AABB::from_points which relied on implementation details of AABB::new_empty (#171)) — no observable invariant at HEAD: the fix decoupled from_points from new_empty's (one, zero) representation, but 4b44c0346 reverted new_empty back to (max, min) so the buggy form (`new_empty().add_point(p)`) and the fixed form (explicit max/min fold init) are now behaviorally equivalent
- `47efe51` (Bugfix: Fixes #45 (batched reinsert)) — no public-API repro at HEAD: the original reproducer (from PR #45) used 3D points where bulk_load + 10 follow-up inserts triggered a tree-internal `unreachable!()` only caught by `sanity_check`, which is `#[cfg(test)]`. Replaying the same input with the buggy form at HEAD does not re-trigger the bug — surrounding code (envelope_for_children, choose_subtree heuristic) drifted enough that the same input no longer hits the multi-reinsert path.
