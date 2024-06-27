# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]


### Added

* Now keeping a change log in the `CHANGELOG.md` file.
* Created Dockerfile for creation of Container Images
* Added a Sequence diagram
* Added the possibility to connect `via` upstream Proxy [5737e29](https://github.com/git001/tls-proxy-tunnel/commit/5737e29743d814c81fcd91a62ff660f3899a5e08)
* Add simple HTTP/1.1 health check [1f533cc](https://github.com/git001/tls-proxy-tunnel/commit/1f533cc8fb576ff8e4bab4027dc7ebc2662ccec6)
* Add parsing and getting of environment variables [8e01d2c](https://github.com/git001/tls-proxy-tunnel/commit/8e01d2cc78dd1895583517f596982e64df51683a)
* Add simple k6 tests [4fd5eee](https://github.com/git001/tls-proxy-tunnel/commit/4fd5eee5c9b2e18e0a9b53865309080b19c395b2)
* Add Global connection counter [ec8db36](https://github.com/git001/tls-proxy-tunnel/commit/ec8db36d9365dfc3970ac53effa2ac77a7be0f8f)
* Add Arc Semaphore to limit clients [2a9c5f1]( https://github.com/git001/tls-proxy-tunnel/commit/2a9c5f1353af131d118bee2077848791a95c9fc7 , [9adb9b9]https://github.com/git001/tls-proxy-tunnel/commit/9adb9b999152d013de27a1851d142e75336101ba)
* Fix Global connection counter for health checks [548d75d](https://github.com/git001/tls-proxy-tunnel/commit/548d75ded78941120122c41619c2827549aeff58)
* Switch to [jemallocator](https://crates.io/crates/jemallocator)

### Changed

* Rename the forked project `layer4-proxy` to `tls-proxy-tunnel`

-------

## Previous versions from layer4-proxy

[0.1.7]: https://code.kiers.eu/jjkiers/layer4-proxy/compare/v0.1.1...v0.1.7



Types of changes:

* `Added` for new features.
* `Changed` for changes in existing functionality.
* `Deprecated` for soon-to-be removed features.
* `Removed` for now removed features.
* `Fixed` for any bug fixes.
* `Security` in case of vulnerabilities.
