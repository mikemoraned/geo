use overture_mirror::listing::served_in;
use overture_mirror::release::{Release, find_superseded};

#[test]
fn a_captured_listing_reads_as_its_releases_with_one_superseded() {
    let listing = include_str!("data/listing-2026-10-03.json");

    let served = served_in(listing).expect("the captured listing");

    let names: Vec<String> = served.iter().map(Release::to_string).collect();
    assert_eq!(names, ["2026-08-19.0", "2026-09-23.0", "2026-09-23.1"]);
    assert_eq!(find_superseded(&served).len(), 1);
}
