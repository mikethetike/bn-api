#!/usr/bin/env bash

set -e

if [[ "$TRAVIS_BRANCH" != "master" ]];then
    echo "Skipping tag-version because branch is not master (but is '$TRAVIS_BRANCH')"
    exit 0
fi

new_version=""

function bump_patch {
    local file="$1"
    local version=`sed -En 's/version[[:space:]]*=[[:space:]]*"([[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+)"/\1/p' < $file`
    new_version=`echo $version | awk -F. -v OFS=. 'NF==1{print ++$NF}; NF>1{$NF=sprintf("%0*d", length($NF), ($NF+1)); print}'`
    local search='^(version[[:space:]]*=[[:space:]]*).+'
    local replace="\1\"${new_version}\""

    sed -i ".tmp" -E "s/${search}/${replace}/g" "$1"
    echo "$file bumped from $version to $new_version"
    rm "$1.tmp"
}

FILES=( "db/Cargo.toml" "api/Cargo.toml" )

for target in "${FILES[@]}"; do
    bump_patch "$target"
    if [[ $1 == "--with-git" ]]; then
        git add "$target"
    fi
done

if [[ $1 == "--with-git" ]]; then
    git commit -m  "Version bump to ${new_version}"
    git tag ${new_version}
    git push origin
    git push --tags
fi