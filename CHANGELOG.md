# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Implement basic interactive state
- Load bencode files
- Add show command to pretty-print data
- Support for raw byte data
- Access to inner values with selectors
- Add reload command
- Add bencode sample files
- Add traverse functionality to bencode values
- Add set command
- Add save command
- Add remove command
- Add insert and append commands
- Add clear command

### Changed

- Limit recursion depth when viewing values
- Fixed empty dicts not being recognized properly
- Fixed recursion depth not being properly tracked
- Truncate long lists when viewing values
- Allow pretty-printing to be configurable
- Show bytes left on truncated bytes when viewing values
- Make bencode dictionary keys exclusively Strings
- Move all bencode logic to a [separate library]

[separate library]: https://github.com/manokara/bencode-rs

[Unreleased]: https://github.com/manokara/bencedit/

