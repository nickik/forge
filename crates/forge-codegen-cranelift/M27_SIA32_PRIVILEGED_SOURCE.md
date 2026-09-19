# M27 SIA32-P source operations

Forge's freestanding SIA32 path reserves the following source-level operation names for protected Cosmic bring-up:

- `sia_trap`
- `sia_sread`
- `sia_swrite`
- `sia_sswap_scratch`
- `sia_sret`
- `sia_sretctx`
- `sia_tlbfence`
- `sia_tlbfence_va`
- `sia_tlbfence_asid`
- `sia_wfi`
- `sia_sync_i`
- `sia_fence`

The authoritative binary encodings live in `sia32_privileged.rs`.  The typed operation/source-name contract lives in `sia32_privileged_source.rs`.

This checkpoint intentionally freezes the Forge-owned vocabulary and its one-to-one mapping to normative SIA32-P operations before changing the language frontend.  The next checkpoint wires these names through frontend/FIR and the SIA32 backend. Non-SIA targets must reject them rather than assign host behavior.
