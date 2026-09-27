use super::{ProcessObservation, classify_unmanaged_memory_usage};

fn process(rss_kib: u64, swap_kib: u64, managed: bool) -> ProcessObservation {
    ProcessObservation {
        rss_kib,
        swap_kib,
        managed,
        ..ProcessObservation::default()
    }
}

#[test]
fn large_external_footprint_is_reported_as_usage() {
    let footprint_kib = 1024 * 1024;
    assert_eq!(
        classify_unmanaged_memory_usage(&[process(footprint_kib, 0, false)]),
        ("UNMANAGED_MEMORY", footprint_kib, 1)
    );
}

#[test]
fn managed_memory_is_not_reported_as_external_usage() {
    assert_eq!(
        classify_unmanaged_memory_usage(&[process(1024 * 1024, 0, true)]),
        ("NONE", 0, 0)
    );
}

#[test]
fn external_footprint_below_observation_floor_is_not_flagged() {
    assert_eq!(
        classify_unmanaged_memory_usage(&[process(512 * 1024 - 1, 0, false)]),
        ("NONE", 0, 0)
    );
}
