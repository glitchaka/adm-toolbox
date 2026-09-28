# Third-party components

Shell Shock Tool is implemented in Rust and uses Rust crates from the open-source ecosystem.

## brush

- Project: `reubeno/brush`
- Components used: `brush-core`, `brush-builtins`
- Purpose: Bash-compatible shell semantics and standard shell builtins.
- License: MIT.

Shell Shock Tool integrates brush as Rust library dependencies. It does not bundle an external Bash executable.

## snmp2

- Purpose: read-only SNMP queries for switch/FDB/IF-MIB integration.
- License: see the crate's distributed license metadata.

Other dependency licenses remain governed by their respective crate distributions and Cargo metadata.
