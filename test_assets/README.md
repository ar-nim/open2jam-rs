# Test Assets

This directory contains test files for OJN/OJM parser unit tests.

## Required Files

The test suite expects at least one valid OJN chart file. The recommended test file is:

- **`o2ma100.ojn`** - A standard O2Jam chart file
- **`o2ma100.ojm`** - Corresponding audio file (optional, for integration tests)

## Where to Get Test Files

1. **Your O2Jam installation** - Copy any `.ojn` and `.ojm` files from your O2Jam song library
2. **Community chart packs** - Download from O2Jam community sites
3. **Sample charts** - Any valid O2Jam format chart will work

## File Naming

Tests look for `o2ma100.ojn` by default. You can:
- Rename your test file to `o2ma100.ojn`, OR
- Update the test paths in `crates/open2jam-parsers/src/ojn.rs`

## Note

These files are **not tracked in git** (see `.gitignore`). Each developer should provide their own test assets.
