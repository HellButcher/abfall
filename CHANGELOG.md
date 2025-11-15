# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/HellButcher/abfall/releases/tag/v0.1.0) - 2025-11-15

### Added

- gc assist
- simple assist with work stealing
- concurrent background collection job
- *(gc)* implement GcPtr/GcRoot split and thread-local context
- implement write barriers and interior mutability cells
- implement incremental marking (Phase 5)
- fix allocation safety race condition (Phase 3)
- add Trace trait system for object graph traversal
- implement concurrent tri-color tracing mark and sweep garbage collector

### Fixed

- fix typo
- ensure memory safety with repr(C) and offset calculation

### Other

- github action
- cleanup docs and add license
- add tests and benchmarks
- manual clenaup
- update PLAN.md to reflect Phase 8 completion
- consolidate and cleanup examples
- update plan
- *(gc_box)* extract GC object layout into separate module
- optimize PLAN.md for token efficiency
- update plan with Phase 5 completion
- fix all clippy warnings
- update plan with Phase 3 & 4 completion status
- add Go GC design inspiration to incremental marking phase
- add vtable-based memory management to plan
- update plan with allocation safety analysis
- implement lock-free intrusive linked list with type-erased headers
