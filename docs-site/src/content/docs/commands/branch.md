---
title: git-med branch
description: Create a new branch reference and immediately switch HEAD and working tree
---

## How to use
`git-med branch <branch_name>`

## What
Creates a new branch reference pointing to the current commit and immediately switches HEAD and the working directory to that branch.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Creation & Switching | `git-med branch <branch_name>` creates AND switches to branch | `git branch <name>` creates only; `git switch -c <name>` or `git checkout -b <name>` creates and switches | `git-med branch` unifies branch creation and checkout into a single command. |
| Ref Location | `.git-med/refs/heads/<branch_name>` | `.git/refs/heads/<branch_name>` | Identical ref storage structure containing the commit hash. |
| Workspace Synchronization | Materializes files in workspace corresponding to target branch | Updates working directory to tree of target commit | Both synchronize working copy files to match the commit tree of the checked-out branch. |
| HEAD Pointer | Updates `.git-med/HEAD` to `ref: refs/heads/<branch_name>` | Updates `.git/HEAD` to `ref: refs/heads/<branch_name>` | Both utilize symbolic ref format for HEAD tracking. |
