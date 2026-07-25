# Instruction Stepping Feature Matrix Reference

This document cross-references instruction-level stepping mechanics in the Soroban Debugger with the master feature support matrix.

---

## Feature Support Matrix Link

- **Master Matrix:** [`docs/FEATURE_MATRIX.md`](FEATURE_MATRIX.md)
- **Instruction Stepping Spec:** [`docs/instruction-stepping.md`](instruction-stepping.md)

---

## Stepping Support Summary

| Debugger Capability | WASM Opcodes Supported | Status |
| :--- | :--- | :--- |
| **Single Step (In)** | `call`, `call_indirect`, `block`, `loop` | ✅ Supported |
| **Step Over** | `call` frame boundary skip | ✅ Supported |
| **Step Out** | Return from current WASM frame | ✅ Supported |
| **Breakpoint Hit** | Contract storage key write/read breakpoint | ✅ Supported |

---

## References

- Issue reference: Fixes #934
