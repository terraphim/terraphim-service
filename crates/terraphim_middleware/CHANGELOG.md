# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.21.0] - 2026-08-28

### Changed
- Consume `terraphim_types` and `terraphim_automata` 1.21.0 from the Gitea `terraphim` registry.
- Depend on `terraphim_file_search` 1.21.0 (`find_matches` takes `&Thesaurus`).
- Pin config, persistence, and rolegraph to the `terraphim` registry so a published crate shares a single `ConfigState`/`Thesaurus` type with Gitea consumers.

## [0.1.0](https://github.com/terraphim/terraphim-ai/releases/tag/terraphim_middleware-v0.1.0) - 2024-04-29

### Other
- Prefix all crates with `terraphim_` for consistency
