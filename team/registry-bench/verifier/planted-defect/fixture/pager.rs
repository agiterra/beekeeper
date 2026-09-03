//! A tiny pager. One of the two functions below has a boundary defect.

/// Pages `total` items `per_page` at a time, returning every page's start
/// index.
pub fn page_starts(total: usize, per_page: usize) -> Vec<usize> {
    if per_page == 0 {
        return Vec::new();
    }
    (0..)
        .map(|page| page * per_page)
        .take_while(|start| *start < total)
        .collect()
}

/// The index of the last page, 0-based.
pub fn last_page(total: usize, per_page: usize) -> usize {
    if per_page == 0 || total == 0 {
        return 0;
    }
    total / per_page
}
