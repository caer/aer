use aer::tool::npm_fetch::NpmFetcher;

const TEST_PACKAGE: &str = "@lexical/rich-text";
const TEST_PACKAGE_VERSION: &str = "0.39.0";

#[test]
#[ignore = "Requires network access and may be slow; run manually with `cargo test --test bundles_npm_package -- --ignored`"]
fn bundles_npm_package() {

    let temp_dir = std::env::temp_dir().join("bundles_npm_package");

    // Fetch the NPM package.
    let mut fetcher = NpmFetcher::new(&temp_dir);
    fetcher.fetch(TEST_PACKAGE, Some(TEST_PACKAGE_VERSION)).expect("failed to fetch package");

    // Cleanup.
    std::fs::remove_dir_all(&temp_dir).expect("failed to clean up temporary directory");
}