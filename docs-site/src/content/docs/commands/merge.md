---
title: git-med merge
description: Merge one or more branches with automated conflict tie-breakers
---

## How to use
`git-med merge <branch> [--left|--right] [-m "<message>"]`

## What
Merges one or more branches into the currently checked-out branch, supporting multi-branch octo-merges and automated conflict resolution strategies for medical datasets.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Invocation | `git-med merge <branch>...` | `git merge <branch>...` | Both support single-branch merges and multi-branch octopus merges. |
| Conflict Resolution | Deterministic `--left` (keep current branch) or `--right` (keep incoming branch) CLI flags | Interactive conflict markers in text files or merge strategies (`-X ours`, `-X theirs`) | `git-med` provides deterministic automated conflict tie-breakers designed for non-textual medical imaging data. |
| History Representation | Creates a multi-parent commit referencing all source heads | Creates a merge commit with multiple parent hashes | Identical multi-parent DAG commit graph representation. |
| Commit Message | Configurable via `-m` or `--message`; generates default merge message if omitted | Configurable via `-m`; generates default message or opens editor | Both generate informative multi-branch merge commit messages. |
