---
title: git-med push
description: Upload commits, medical metadata trees, and LFS binary objects to remote server
---

## How to use
`git-med push [remote] [branch]`

## What
Uploads local commits, medical metadata trees, and staged LFS binary imaging objects to the remote `git-med-forge` server and advances remote branch references.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Syntax & Defaults | `git-med push [remote] [branch]` (defaults to `origin` and current branch) | `git push [remote] [branch]` (defaults to configured upstream) | Same parameter conventions and workflow. |
| Payload Handling | Uploads commit/tree objects and streams raw medical binary files to server/S3 LFS storage | Uploads commit and tree packfiles; requires Git LFS hooks for binary assets | Atomically handles both dataset metadata trees and high-volume binary medical files. |
| Authentication | Injects Authorization Bearer token resolved from credentials or environment variables | Uses SSH keys or HTTP basic authentication | Built-in token-based authentication against `git-med-forge`. |
