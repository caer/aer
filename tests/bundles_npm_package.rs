use std::path::PathBuf;

use aer::tool::js::JsModuleManager;

const TEST_PACKAGE: &str = "@tiptap/core";
const TEST_PACKAGE_VERSION: &str = "3.13.0";

#[test]
#[ignore = "Requires network access and may be slow; run manually with `cargo test --test bundles_npm_package -- --ignored`"]
fn bundles_npm_package() {
    let temp_dir = PathBuf::from("target/tmp/tests/bundles_npm_package");

    // Fetch the NPM package.
    let mut manager = JsModuleManager::new(&temp_dir.join("compressed"));
    manager
        .fetch(TEST_PACKAGE, Some(TEST_PACKAGE_VERSION))
        .expect("failed to fetch package");

    // Extract the package contents.
    manager
        .extract_modules(temp_dir.join("extracted"))
        .expect("failed to extract modules");

    // Copy the JavaScript entry file into the extracted directory so bundling can resolve node_modules.
    let js_entry_src = PathBuf::from("tests/bundles_npm_package.js");

    // Bundle the JS entry using the manager (which uses the js_bundle processor under the hood).
    let bundled = manager
        .bundle(&js_entry_src, &temp_dir.join("extracted"))
        .expect("failed to bundle JS entry");

    // If the bundler applies aggressive tree-shaking, the output may be small.
    // We only assert that bundling succeeded without panic.
    assert!(!bundled.is_empty(), "bundled output is empty");

    // Cleanup.
    // std::fs::remove_dir_all(&temp_dir).expect("failed to clean up temporary directory");
}
