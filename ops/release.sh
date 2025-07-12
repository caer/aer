#!/bin/sh
#
# Usage: ./ops/release.sh [Release Type]
#
# Where [Release Type] is one of:
declare -a releaseTypes=("patch" "minor" "major")
#
# This script will fail if any of the released crates
# do not contain a preexisting CHANGELOG.md.

# Check arguments.
if [ "$#" -lt 1 ]; then
    echo "please provide a release type. examples:\n"
    echo "  ./ops/release.sh patch"
    echo "  ./ops/release.sh minor"
    echo "  ./ops/release.sh major"
    exit 1
fi

# Extract arguments.
releaseType=$1

# Only allow supported release types.
if [[ ! " ${releaseTypes[*]} " =~ [[:space:]]${releaseType}[[:space:]] ]]; then
    echo "${releaseType} is not one of: ${releaseTypes[*]}"
    exit 1
fi

# Only allow releases on a clean working directory.
if [ ! -z "$(git status --porcelain)" ]; then
    echo "aborting release: uncommited changes present in repository"
    exit 1
fi

# Verify workspace.
cargo fmt --check
cargo clippy
cargo test

# Install release tooling if necessary.
echo "installing release tooling..."
cargo install -q cargo-release git-cliff
echo "release tooling installed."

# Dry-run release.
echo "beginning release dry-run..."
cargo release $releaseType --config ./ops/release.toml
echo "cleaning up dry-run changes..."
git stash

# Prompt for commit confirmation.
read -p "execute release (y / N)? " -n 1 -r
echo

# Execute release if yes.
if [[ $REPLY =~ ^[Yy]$ ]]
then
    # Normalize release dates.
    UTC_DAY_BEGIN=$(TZ=0 date +%F)T00:00:00+0000
    GIT_AUTHOR_DATE=$UTC_DAY_BEGIN GIT_COMMITTER_DATE=$UTC_DAY_BEGIN cargo release $releaseType --config ./ops/release.toml --execute
# Abort commit if no.
else
    echo "release aborted"
fi