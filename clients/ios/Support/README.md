# Synveil iOS Support & Tooling (`clients/ios/Support/`)

## Purpose
`clients/ios/Support/` contains repository tools, static source validators, build support scripts, and reference configurations for the Synveil iOS client (`v0.1`).

## Contents
- **`validate_ios_sources.py`**: Deterministic Python static source validator enforcing ADR-058 and `IOS_ARCHITECTURE.md` invariants.
- **`tests/test_validate_ios_sources.py`**: Unit self-test suite for `validate_ios_sources.py` using Python `unittest`.

## Running Static Validation Locally

### 1. Execute Static Source Validator
```bash
python3 clients/ios/Support/validate_ios_sources.py
```

### 2. Execute Validator Self-Tests
```bash
python3 -m unittest discover -s clients/ios/Support/tests
```
