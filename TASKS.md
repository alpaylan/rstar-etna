# rstar — ETNA Tasks

Total tasks: 12

## Task Index

| Task | Variant | Framework | Property | Witness |
|------|---------|-----------|----------|---------|
| 001 | `bulk_load_clusters_clamp_0139255a_1` | proptest | `BulkLoadSizeCorrect` | `witness_bulk_load_size_correct_case_log_imprecision` |
| 002 | `bulk_load_clusters_clamp_0139255a_1` | quickcheck | `BulkLoadSizeCorrect` | `witness_bulk_load_size_correct_case_log_imprecision` |
| 003 | `bulk_load_clusters_clamp_0139255a_1` | crabcheck | `BulkLoadSizeCorrect` | `witness_bulk_load_size_correct_case_log_imprecision` |
| 004 | `bulk_load_clusters_clamp_0139255a_1` | hegel | `BulkLoadSizeCorrect` | `witness_bulk_load_size_correct_case_log_imprecision` |
| 005 | `empty_tree_iter_no_overflow_4b44c03_1` | proptest | `EmptyTreeQueryNoPanic` | `witness_empty_tree_query_no_panic_case_origin` |
| 006 | `empty_tree_iter_no_overflow_4b44c03_1` | quickcheck | `EmptyTreeQueryNoPanic` | `witness_empty_tree_query_no_panic_case_origin` |
| 007 | `empty_tree_iter_no_overflow_4b44c03_1` | crabcheck | `EmptyTreeQueryNoPanic` | `witness_empty_tree_query_no_panic_case_origin` |
| 008 | `empty_tree_iter_no_overflow_4b44c03_1` | hegel | `EmptyTreeQueryNoPanic` | `witness_empty_tree_query_no_panic_case_origin` |
| 009 | `min_size_one_log_clamp_44e1bf5_1` | proptest | `MinSizeOneNoPanic` | `witness_min_size_one_no_panic_case_two_points` |
| 010 | `min_size_one_log_clamp_44e1bf5_1` | quickcheck | `MinSizeOneNoPanic` | `witness_min_size_one_no_panic_case_two_points` |
| 011 | `min_size_one_log_clamp_44e1bf5_1` | crabcheck | `MinSizeOneNoPanic` | `witness_min_size_one_no_panic_case_two_points` |
| 012 | `min_size_one_log_clamp_44e1bf5_1` | hegel | `MinSizeOneNoPanic` | `witness_min_size_one_no_panic_case_two_points` |

## Witness Catalog

- `witness_bulk_load_size_correct_case_log_imprecision` — N = 216 (= 6^3 with default MAX_SIZE = 6) hits the log-imprecision case.
- `witness_empty_tree_query_no_panic_case_origin` — Regression test #161: `locate_within_distance` on an empty `RTree<[i64; 3]>` previously panicked.
- `witness_min_size_one_no_panic_case_two_points` — base passes, variant fails
