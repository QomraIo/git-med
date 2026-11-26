# git-med CLI Commands

Comprehensive reference and guide to all commands available in the `git-med` CLI.

---

## Command Reference

| Category | Command | Syntax | Description |
| --- | --- | --- | --- |
| **Repository Setup** | [init](file:///root/git-med/docs/commands/init.md) | `git-med init <repository_name>` | Initialize a new `git-med` repository directory |
| **Workspace & Staging** | [add](file:///root/git-med/docs/commands/add.md) | `git-med add` | Stage changes into the index and extract medical metadata |
| **Workspace & Staging** | [commit](file:///root/git-med/docs/commands/commit.md) | `git-med commit -m "<message>"` | Record staged metadata tree into a new commit |
| **Branching & Merging** | [branch](file:///root/git-med/docs/commands/branch.md) | `git-med branch <branch_name>` | Create and switch to a branch reference |
| **Branching & Merging** | [merge](file:///root/git-med/docs/commands/merge.md) | `git-med merge <branch>... [--left\|--right]` | Merge branches with automated conflict tie-breakers |
| **Medical Formats & Views** | [view](file:///root/git-med/docs/commands/view.md) | `git-med view <smart\|dicom\|brats>` | Display or transform workspace between medical imaging formats |
| **Cohort Composition** | [mix](file:///root/git-med/docs/commands/mix.md) | `git-med mix <result_branch> <spec1> <spec2>...` | Synthesize cohorts across multiple branches/repos with zero-copy linking |
| **Remote Collaboration** | [remote](file:///root/git-med/docs/commands/remote.md) | `git-med remote add <name> <url>` | Manage remote repository URLs in configuration |
| **Remote Collaboration** | [clone](file:///root/git-med/docs/commands/clone.md) | `git-med clone <url> [destination]` | Clone remote repository and hydrate medical LFS assets |
| **Remote Collaboration** | [push](file:///root/git-med/docs/commands/push.md) | `git-med push [remote] [branch]` | Push commits and binary imaging assets to remote server |
| **Remote Collaboration** | [pull](file:///root/git-med/docs/commands/pull.md) | `git-med pull [remote] [branch]` | Fetch changes and binary assets to fast-forward local branch |
| **Authentication & Identity** | [login](file:///root/git-med/docs/commands/login.md) | `git-med login --server <url> --username <u> --password <p>` | Authenticate against server and store Bearer token |
| **Authentication & Identity** | [logout](file:///root/git-med/docs/commands/logout.md) | `git-med logout --server <url>` | Remove stored credentials for server origin |
| **Authentication & Identity** | [whoami](file:///root/git-med/docs/commands/whoami.md) | `git-med whoami --server <url>` | Report currently authenticated identity |
| **Test Utilities** | [generate](file:///root/git-med/docs/commands/generate.md) | `git-med generate <type> <output_dir> [...]` | Generate synthetic medical datasets for testing |

---

## Detailed Command Documentation

1. [init](file:///root/git-med/docs/commands/init.md) - Repository initialization
2. [add](file:///root/git-med/docs/commands/add.md) - View-aware staging and LFS separation
3. [commit](file:///root/git-med/docs/commands/commit.md) - Commit creation and history recording
4. [branch](file:///root/git-med/docs/commands/branch.md) - Branch creation and checkout
5. [remote](file:///root/git-med/docs/commands/remote.md) - Remote server management
6. [view](file:///root/git-med/docs/commands/view.md) - Medical format layout switching (SMART, DICOM, BraTS)
7. [clone](file:///root/git-med/docs/commands/clone.md) - Remote cloning with LFS asset hydration
8. [push](file:///root/git-med/docs/commands/push.md) - Remote metadata and LFS payload synchronization
9. [pull](file:///root/git-med/docs/commands/pull.md) - Fast-forward update and asset retrieval
10. [merge](file:///root/git-med/docs/commands/merge.md) - Multi-branch merging with conflict tie-breakers
11. [mix](file:///root/git-med/docs/commands/mix.md) - Multi-source cohort blending with zero-copy linking
12. [login](file:///root/git-med/docs/commands/login.md) - Forge server and Auth0 authentication
13. [logout](file:///root/git-med/docs/commands/logout.md) - Server session termination
14. [whoami](file:///root/git-med/docs/commands/whoami.md) - Active identity verification
15. [generate](file:///root/git-med/docs/commands/generate.md) - Synthetic medical dataset generator

---

## Related Documentation

* [Authentication Guide](file:///root/git-med/docs/authentication.md): Complete guide to tokens, credentials storage, Auth0, and role-based access control.
