# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2021-12-24
- Remove deprecated code
- Update to 2021 rust edition

## [0.4.4] - 2021-02-20

### Changed
- Fix POSIX path translation for shared memory FD [@calebzulawski](https://github.com/calebzulawski).
- Remove dependency on `rand` [@calebzulawski](https://github.com/calebzulawski).

## [0.4.3] - 2020-11-06
### Added
- Testing for Android, FreeBSD, and Solaris by [@calebzulawski](https://github.com/calebzulawski).
- Volatile and unaligned reads and writes for Span and SpanMut
- Start a CHANGELOG

### Changed
- Improve FreeBSD shared memory FD for ring buffers by [@calebzulawski](https://github.com/calebzulawski).
- Fix Solaris and BSDs temp paths for shared memory FD [@calebzulawski](https://github.com/calebzulawski).
- Fix Windows 32-bit size handling

## [0.4.2] - 2020-10-06
### Added
- Add `os` and `io` optional features

### Changes
- Documentation improvements
- Stopped using deprecated examples where possible
- Return the `File` object when `open`ing from a path

[Unreleased]: https://github.com/kalamay/vmap-rs/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/kalamay/vmap-rs/compare/v0.4.4...v0.5.0
[0.4.4]: https://github.com/kalamay/vmap-rs/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/kalamay/vmap-rs/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/kalamay/vmap-rs/compare/v0.4.1...v0.4.2
