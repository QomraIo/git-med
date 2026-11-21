---
title: git-med pull
description: Fetch changes and medical binary objects to fast-forward local branch
---

## How to use
`git-med pull [remote] [branch]`

## What
Fetches recent commits, metadata trees, and binary medical assets from the remote repository and fast-forwards the local branch while updating working directory files.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Syntax & Defaults | `git-med pull [remote] [branch]` (defaults to `origin` and current branch) | `git pull [remote] [branch]` | Identical command structure and defaults. |
| Update Strategy | Performs fast-forward update of local branch and materializes missing files | Runs `git fetch` followed by `git merge` or `git rebase` | `git-med pull` fast-forwards and synchronizes local workspace to remote head. |
| Asset Hydration | Automatically detects, downloads, and hashes missing LFS binary files | Requires Git LFS smudge/pull hooks | Single unified step downloads both metadata and heavy medical images. |
| Status Output | Reports detailed old/new commit hashes or `Already up to date.` | Reports fast-forward stats or `Already up to date.` | Similar user-facing status feedback. |
