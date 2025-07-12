#!/bin/sh
#
# Usage: ./ops/commit.sh [Commit Type] "my commit message"
#
# Where [Commit Type] is one of:
declare -a commitTypes=("feat" "docs" "fix" "ops")

# Check arguments.
if [ "$#" -lt 2 ]; then
    echo "please provide a commit type and message. examples:\n"
    echo "  ./ops/commit.sh feat \"added a new feature\""
    echo "  ./ops/commit.sh docs \"edited some documentation\""
    echo "  ./ops/commit.sh fix \"fixed an issue\""
    echo "  ./ops/commit.sh ops \"improved the ci/cd pipeline\""
    exit 1
fi

# Extract arguments, merging all excess
# arguments into the commit message.
commitType=$1
commitMessage=$2
while shift && [ -n "$2" ]; do
    commitMessage="${commitMessage} $2"
done
commitMessage="${commitType}: ${commitMessage}"

# Only allow supported commit types.
if [[ ! " ${commitTypes[*]} " =~ [[:space:]]${commitType}[[:space:]] ]]; then
    echo "${commitType} is not one of: ${commitTypes[*]}"
    exit 1
fi

# Normalize commit dates.
UTC_DAY_BEGIN=$(TZ=0 date +%F)T00:00:00+0000 

# Update changelogs for all crates.
ls */Cargo.toml | while read; do
    cratePath=${REPLY%/*}

    # Only udpate changelogs for crate paths affected by this commit.
    if [ ! -z "$(git status --porcelain ${cratePath})" ]; then
        cd ${cratePath}
        git cliff --with-commit "${commitMessage}" --config ../ops/git-cliff.toml -o CHANGELOG.md
        cd ..
    fi
done | sort -u

# Stage all changes and show the staged changes to the user.
git add --all .

# Preview staged changes and commit message.
echo "\npreview of commit @ ${UTC_DAY_BEGIN}:\n"
git -c color.status=always status --short | grep '^\(\x1b\[[0-9]\{1,2\}m\)\{0,1\}[MARCD]'| sed -e 's/^/  /'
echo
echo "  ${commitMessage}\n"

# Prompt for commit confirmation.
read -p "commit (y / N)? " -n 1 -r
echo

# Execute commit if yes.
if [[ $REPLY =~ ^[Yy]$ ]]
then
    GIT_AUTHOR_DATE=$UTC_DAY_BEGIN GIT_COMMITTER_DATE=$UTC_DAY_BEGIN git commit -m "${commitMessage}"

# Abort commit if no.
else
    echo "commit aborted"
fi