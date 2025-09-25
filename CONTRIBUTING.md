# Contributing With Caer: A Guide

This doc is a general guide on how to contribute to projects owned by
[@caer](https://www.github.com/caer) and [@with-caer](https://www.github.com/with-caer)  ("our projects").

## Contributing Code

The vast majority of our projects are built with [Rust](https://www.rust-lang.org) and organized
as [Cargo Virtual Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html#virtual-workspace).

Because our projects use similar tech stacks, we maintain a
central repository for configuring developer environments
and dev containers: [`OwC-workbench`](https://github.com/with-caer/OwC-workbench).

### Before Committing Changes

...run:

1. `cargo fmt` to format all code changes.
2. `cargo clippy` to statically analyze all code changes.
3. `cargo test` to test all code changes.

### When Committing Changes

...use `owc-commit` instead of `git commit`. This script is automatically installed
by most of our projects' Dev Containers, but can be manually installed by cloning
[`OwC-workbench`](https://github.com/with-caer/OwC-workbench)
and running [`install-tools.sh`](https://github.com/with-caer/OwC-workbench/blob/main/install-tools.sh).

## Contributors' License Agreement

The _vast_ majority of our projects are licensed under one of the following licenses:

- The [MIT License](https://mit-license.org), which is a free and open source license.

- The [Functional Source License (FSL-1.1-MIT)](https://fsl.software), which is a ["Fair Source"](https://fair.io)  license, but _not_ an open source license.

While contributions to open source projects are typically covered by the ["inbound=outbound" clause in GitHub's term of service](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#6-contributions-under-repository-license), contributions to "Fair Source" projects might not be.

So that we can adopt and maintain "Fair Source" licensing for our projects, we ask contributors to agree to our [Contributors' License Agreement](.github/pull_request_template.md#with-caer-contributors-license-agreement-version-10) as part of each pull request.

Our pull request template automatically attaches this agreement: As a contributor, all you have to do is check the box confirming you've read and agree to the terms of the agreement.