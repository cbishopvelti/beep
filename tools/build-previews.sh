#!/usr/bin/env bash
# Adds an install page for every open pull request to an already built
# loader (sh tools/build-loader.sh first), one folder per PR:
#
#   sh tools/build-previews.sh [outdir]     (default: _site)
#
# so PR #33 installs from <site>/pr-33/. Only PRs whose branch is in this
# repository get one, never forks. The PR's files are only copied, never run.
#
# Version: 1.00.<main's build>.<commits the PR is ahead of main>, e.g.
# 1.00.32.3. The App Loader offers it as an update over main's 1.00.32, and
# main's next build (1.00.33, once the PR is merged) as an update over it.
#
# Needs GITHUB_REPOSITORY (owner/name) and GH_TOKEN for the GitHub API, and
# full history with origin/main fetched. Writes <outdir>/previews.json.
set -eu

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-_site}
case "$OUT" in /*) ;; *) OUT="$REPO_DIR/$OUT" ;; esac
REPO=${GITHUB_REPOSITORY:?set GITHUB_REPOSITORY to owner/name}
BASE=${PREVIEW_BASE:-main}
WORK=$(mktemp -d)
trap 'git -C "$REPO_DIR" worktree prune; rm -rf "$WORK"' EXIT

cd "$REPO_DIR"
git fetch -q origin "+refs/heads/$BASE:refs/remotes/origin/$BASE"
MAIN_BUILD=$(git rev-list --count "origin/$BASE")
BASE_VERSION=$(node -p 'require("./metadata.json").version')

gh api "repos/$REPO/pulls?state=open&base=$BASE&per_page=100" \
  --jq ".[] | select(.head.repo.full_name == \"$REPO\") | [.number, .head.sha, .html_url, .head.ref] | @tsv" \
  > "$WORK/prs.tsv"

echo "[" > "$WORK/previews.json"
sep=""
while IFS="$(printf '\t')" read -r num sha url branch; do
  [ -n "$num" ] || continue
  case "$num" in *[!0-9]*) continue ;; esac
  git fetch -q origin "+refs/pull/$num/head:refs/remotes/pr/$num" || { echo "PR #$num: fetch failed, skipped"; continue; }
  sha=$(git rev-parse "refs/remotes/pr/$num")
  ahead=$(git rev-list --count "origin/$BASE..$sha")
  [ "$ahead" -gt 0 ] || ahead=1
  version="$BASE_VERSION.$MAIN_BUILD.$ahead"
  # A fresh worktree: the PR's own metadata.json says which files to take.
  git worktree add -q --detach "$WORK/pr-$num" "$sha"
  BEEP_SRC="$WORK/pr-$num" BEEP_VERSION="$version" \
    BEEP_PR="$num" BEEP_PR_URL="$url" BEEP_PR_BRANCH="$branch" \
    LOADER_FROM="$OUT" sh "$REPO_DIR/tools/build-loader.sh" "$WORK/site-$num" | tail -1
  mv "$WORK/site-$num" "$OUT/pr-$num"
  git worktree remove --force "$WORK/pr-$num"
  printf '%s{"number":%s,"sha":"%s","version":"%s","path":"pr-%s/"}\n' \
    "$sep" "$num" "$sha" "$version" "$num" >> "$WORK/previews.json"
  sep=","
  echo "PR #$num: $version at pr-$num/"
done < "$WORK/prs.tsv"
echo "]" >> "$WORK/previews.json"
cp "$WORK/previews.json" "$OUT/previews.json"
