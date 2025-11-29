# Contributing to Browser-db

Thanks for your interest in contributing to Browser-db — we appreciate your help! This document explains how to report issues, propose changes, and submit pull requests so we can collaborate efficiently.

---

## Table of contents
- Who can contribute
- Code of conduct
- Getting started
- Branching & commit guidelines
- Developing & testing
- Submitting pull requests
- Reporting issues
- Security disclosures
- Communication & support
- License & copyright

---

## Who can contribute
Everyone is welcome. Whether you're fixing a typo, adding features, improving docs, or writing tests, contributions of all sizes help make Browser-db better.

---

## Code of conduct
Please follow common open-source community standards. Be respectful, inclusive, and apply common open-source community standards.

---

## Getting started

1. Fork the repository to your GitHub account.
2. Clone your fork:
   git clone https://github.com/<your-username>/Browser-db.git
   cd Browser-db
3. Add the upstream remote and keep your fork in sync:
   git remote add upstream https://github.com/Intro0siddiqui/Browser-db.git
   git fetch upstream

Prerequisites (for development)
- Zig 0.14+
- Rust 1.75+ 
- CMake 3.16+
- Cargo (for Rust examples/tests)
- Node.js / npm (only if JS bindings or web tooling are added)

For detailed setup and architecture, see DEVELOPER_GUIDE.md.

---

## Branching & commit guidelines

- Create a feature branch from main:
  git checkout -b feat/<short-description>
  or for fixes: git checkout -b fix/<short-description>

- Use clear, descriptive branch names and PR titles.

Commit message convention (recommended)
- Use a short imperative summary:
  feat(core): add heatmap eviction policy
  fix(rust-bindings): handle null pointers in open()
  docs: update QUICK_START.md for Zig build
- Include a longer description when needed in the commit body.
- Prefer small, focused commits that are easy to review.

We recommend following Conventional Commits, but maintainers may accept other clear formats.

---

## Developing & testing

Build core engine (Zig)
- cd core
- zig build

Run Zig tests
- zig build test

Build and run Rust bindings
- cd bindings
- cargo test
- cargo run --example basic_usage

Run all tests locally before opening a PR. Include new tests for bug fixes or new features.

Formatting & linting
- Keep code formatted according to the repository's conventions. If linters or formatters are added, run them before committing.
- For Rust: rustfmt and clippy are encouraged.

---

## Submitting pull requests

1. Ensure your branch is up to date with main:
   git fetch upstream
   git rebase upstream/main

2. Push your branch to your fork:
   git push origin <branch-name>

3. Open a Pull Request against Intro0siddiqui/Browser-db main branch.

PR checklist
- [ ] Clear, descriptive title and description explaining the motivation and changes.
- [ ] Linked issue (if applicable) or explanation why no issue was opened.
- [ ] Tests added or existing tests updated to cover the change.
- [ ] Build passes locally (core, bindings, examples).
- [ ] Code is formatted and linted.
- [ ] No sensitive information (secrets, keys) in the changes.

What to include in the PR description
- Summary of changes.
- Testing performed (commands and environment).
- Any backward-incompatible changes or migration steps.
- Related issues or PRs.

Pull requests will be reviewed by maintainers. Be ready to iterate on feedback. Small, focused PRs are faster to review.

---

## Reporting issues

Good issues help maintainers act faster. When opening an issue:
- Provide a clear title and description.
- Steps to reproduce (code snippets, commands).
- Expected vs actual behavior.
- Environment (OS, Zig version, Rust version, browser if relevant).
- Attach logs, backtraces, or a minimal reproducible example when possible.

Use the issue templates if the repository provides them.

---

## Security disclosures

If you discover a security vulnerability, do NOT open a public GitHub issue. Contact the maintainers privately:
- Preferred: send an email to the maintainers' security contact (add an address or link here if the project has one).
- If no private channel exists, create a confidential report via GitHub's security advisory (or contact repository owner directly).

Include:
- Clear description of the vulnerability
- Steps to reproduce or a proof-of-concept
- Impact and possible mitigations

Maintainers will acknowledge within a reasonable timeframe and coordinate a fix.

---

## Communication & support

- Issues: https://github.com/Intro0siddiqui/Browser-db/issues
- Discussions (if enabled): https://github.com/Intro0siddiqui/Browser-db/discussions
- For quick questions, check existing issues/discussions before opening new ones.

---

## License & copyright

By contributing, you confirm that your contributions are made under the project's BSD-3-Clause license (see LICENSE). If your contribution includes third-party code, ensure it is compatible with the project license and clearly attribute it.

---

## Thanks & acknowledgements

Thanks for taking the time to contribute — your efforts make Browser-db better for everyone. If you'd like help finding something to work on, check the issue tracker for "good first issue" or "help wanted" labels, or ask in Discussions.

Happy hacking!
