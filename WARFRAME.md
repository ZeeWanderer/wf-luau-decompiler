# Warframe Frontend

Based on Tovek v2.6.1. `wf-luau-decompiler` reads opcode-normalized Warframe Luau
from a file or standard input and writes reconstructed source to standard output.

```sh
wf-luau-decompiler normalized.wflu
wf-luau-decompiler - --diagnostics json < normalized.wflu
wf-luau-decompiler --describe
wf-luau-decompiler normalized.wflu --symbol 2fdd25fc=module
```

`--diagnostics json` writes one protocol-versioned result to standard error.
`--describe` reports the interface and capabilities. `--dialect luau` accepts
ordinary Luau bytecode; the default dialect is Warframe.

Warframe constant tag 1 contains a four-byte value: 0 and 1 are booleans, other
values are atoms. Unknown atoms retain their identities as `__wf_atom_XXXXXXXX`.
Repeat `--symbol HEX=NAME` for verified names. These are presentation labels,
not proof that native bindings match between game versions.

## Maintenance

Game resource extraction and opcode mapping belong to the caller. Warframe
presentation conversion lives in `luau-lifter/src/warframe.rs`; the shared parser
only handles the different constant width. The reconstruction pipeline accepts
a parser callback so the adapter also works during upstream retries. Upstream
lifting, validation and control-flow passes remain unchanged.

The adapter retains optimized-call markers and their fallback calls but does not
assign standard Luau identities to Warframe host functions. Specialized iterator
preparations use the generic iterator path. Instruction positions stay unchanged;
an optimized call without its fallback is rejected. Original instructions and atom
identities remain available through the caller's disassembler.

Keep frontend tests in `tests/warframe_cli.rs` and dialect tests in `warframe.rs`.
Update through upstream merges, retaining upstream tests and documentation.
Validate generated fixtures and a local, untracked game-script corpus before
updating a consumer's submodule pin. Decompiled source remains a research aid,
not proof of semantic equivalence with the game.

Run the fork's tests with `RUSTC_BOOTSTRAP=1 cargo test --workspace --locked`.
Use an ordinary Luau compiler to check that reconstructed corpus sources compile;
this checks source validity, not equivalence with native game bindings.
