// Fixed team-owned presence regression; filesystem behavior is checked externally.
#[test]
fn build_archive_helper_is_present() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../release/archive-build-artifacts.ps1");
    assert!(source.is_file(), "release/archive-build-artifacts.ps1 must exist");
}
