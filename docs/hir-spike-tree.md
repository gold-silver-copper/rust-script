# Exact HIR spike crate graph

Generated on 2026-07-18 from an ephemeral Rust 1.95 crate with exact direct
dependencies `ra_ap_base_db = "=0.0.342"`, `ra_ap_hir = "=0.0.342"`, and
`ra_ap_hir_ty = "=0.0.342"` using:

```text
cargo +1.95.0 generate-lockfile
cargo +1.95.0 tree --target wasm32-unknown-unknown --edges normal,build --locked
```

Cargo locked 137 packages to Rust 1.95-compatible versions. The exact resolved
normal/build dependency edges are:

```text
rustscript-hir-spike v0.0.0 (/private/tmp/rustscript-hir-spike)
├── ra_ap_base_db v0.0.342
│   ├── dashmap v6.2.1
│   │   ├── cfg-if v1.0.4
│   │   ├── crossbeam-utils v0.8.22
│   │   ├── hashbrown v0.14.5
│   │   ├── lock_api v0.4.14
│   │   │   └── scopeguard v1.2.0
│   │   ├── once_cell v1.21.4
│   │   └── parking_lot_core v0.9.12
│   │       ├── cfg-if v1.0.4
│   │       └── smallvec v1.15.2
│   ├── indexmap v2.14.0
│   │   ├── equivalent v1.0.2
│   │   ├── hashbrown v0.17.1
│   │   │   ├── allocator-api2 v0.2.21
│   │   │   ├── equivalent v1.0.2
│   │   │   └── foldhash v0.2.0
│   │   └── serde_core v1.0.229
│   ├── la-arena v0.3.1
│   ├── ra_ap_cfg v0.0.342
│   │   ├── ra_ap_intern v0.0.342
│   │   │   ├── arrayvec v0.7.8
│   │   │   ├── dashmap v6.2.1 (*)
│   │   │   ├── hashbrown v0.14.5
│   │   │   ├── rayon v1.12.0
│   │   │   │   ├── either v1.16.0
│   │   │   │   └── rayon-core v1.13.0
│   │   │   │       ├── crossbeam-deque v0.8.7
│   │   │   │       │   ├── crossbeam-epoch v0.9.20
│   │   │   │       │   │   └── crossbeam-utils v0.8.22
│   │   │   │       │   └── crossbeam-utils v0.8.22
│   │   │   │       └── crossbeam-utils v0.8.22
│   │   │   ├── rustc-hash v2.1.3
│   │   │   └── triomphe v0.1.16
│   │   ├── ra_ap_syntax v0.0.342
│   │   │   ├── either v1.16.0
│   │   │   ├── itertools v0.15.0
│   │   │   │   └── either v1.16.0
│   │   │   ├── ra_ap_parser v0.0.342
│   │   │   │   ├── drop_bomb v0.1.5
│   │   │   │   ├── ra-ap-rustc_lexer v0.165.0
│   │   │   │   │   ├── memchr v2.8.3
│   │   │   │   │   ├── unicode-ident v1.0.24
│   │   │   │   │   └── unicode-properties v0.1.4
│   │   │   │   ├── ra_ap_edition v0.0.342
│   │   │   │   ├── rustc-literal-escaper v0.0.7
│   │   │   │   ├── tracing v0.1.44
│   │   │   │   │   ├── pin-project-lite v0.2.17
│   │   │   │   │   ├── tracing-attributes v0.1.31 (proc-macro)
│   │   │   │   │   │   ├── proc-macro2 v1.0.107
│   │   │   │   │   │   │   └── unicode-ident v1.0.24
│   │   │   │   │   │   ├── quote v1.0.47
│   │   │   │   │   │   │   └── proc-macro2 v1.0.107 (*)
│   │   │   │   │   │   └── syn v2.0.119
│   │   │   │   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │   │   │   │       ├── quote v1.0.47 (*)
│   │   │   │   │   │       └── unicode-ident v1.0.24
│   │   │   │   │   └── tracing-core v0.1.36
│   │   │   │   │       └── once_cell v1.21.4
│   │   │   │   └── winnow v0.7.15
│   │   │   ├── ra_ap_stdx v0.0.342
│   │   │   │   ├── crossbeam-channel v0.5.16
│   │   │   │   │   └── crossbeam-utils v0.8.22
│   │   │   │   ├── crossbeam-utils v0.8.22
│   │   │   │   ├── itertools v0.15.0 (*)
│   │   │   │   ├── jod-thread v1.0.0
│   │   │   │   └── tracing v0.1.44 (*)
│   │   │   ├── rowan v0.15.19
│   │   │   │   ├── countme v3.0.1
│   │   │   │   ├── hashbrown v0.14.5
│   │   │   │   ├── memoffset v0.9.1
│   │   │   │   │   [build-dependencies]
│   │   │   │   │   └── autocfg v1.5.1
│   │   │   │   ├── rustc-hash v1.1.0
│   │   │   │   └── text-size v1.1.1
│   │   │   ├── rustc-hash v2.1.3
│   │   │   ├── rustc-literal-escaper v0.0.7
│   │   │   ├── smallvec v1.15.2
│   │   │   ├── smol_str v0.3.6
│   │   │   ├── tracing v0.1.44 (*)
│   │   │   └── triomphe v0.1.16
│   │   ├── ra_ap_tt v0.0.342
│   │   │   ├── arrayvec v0.7.8
│   │   │   ├── indexmap v2.14.0 (*)
│   │   │   ├── ra-ap-rustc_lexer v0.165.0 (*)
│   │   │   ├── ra_ap_intern v0.0.342 (*)
│   │   │   ├── ra_ap_span v0.0.342
│   │   │   │   ├── hashbrown v0.17.1 (*)
│   │   │   │   ├── la-arena v0.3.1
│   │   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   │   ├── ra_ap_syntax v0.0.342 (*)
│   │   │   │   ├── ra_ap_vfs v0.0.342
│   │   │   │   │   ├── crossbeam-channel v0.5.16 (*)
│   │   │   │   │   ├── fst v0.4.7
│   │   │   │   │   ├── indexmap v2.14.0 (*)
│   │   │   │   │   ├── nohash-hasher v0.2.0
│   │   │   │   │   ├── ra_ap_paths v0.0.342
│   │   │   │   │   │   └── camino v1.2.4
│   │   │   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   │   │   ├── rustc-hash v2.1.3
│   │   │   │   │   └── tracing v0.1.44 (*)
│   │   │   │   ├── rustc-hash v2.1.3
│   │   │   │   ├── salsa v0.27.2
│   │   │   │   │   ├── boxcar v0.2.14
│   │   │   │   │   ├── crossbeam-queue v0.3.13
│   │   │   │   │   │   └── crossbeam-utils v0.8.22
│   │   │   │   │   ├── crossbeam-utils v0.8.22
│   │   │   │   │   ├── hashbrown v0.17.1 (*)
│   │   │   │   │   ├── hashlink v0.12.1
│   │   │   │   │   │   └── hashbrown v0.17.1 (*)
│   │   │   │   │   ├── indexmap v2.14.0 (*)
│   │   │   │   │   ├── intrusive-collections v0.10.2
│   │   │   │   │   │   └── memoffset v0.9.1 (*)
│   │   │   │   │   ├── inventory v0.3.24
│   │   │   │   │   │   └── rustversion v1.0.23 (proc-macro)
│   │   │   │   │   ├── parking_lot v0.12.5
│   │   │   │   │   │   ├── lock_api v0.4.14 (*)
│   │   │   │   │   │   └── parking_lot_core v0.9.12 (*)
│   │   │   │   │   ├── portable-atomic v1.14.0
│   │   │   │   │   ├── rayon v1.12.0 (*)
│   │   │   │   │   ├── rustc-hash v2.1.3
│   │   │   │   │   ├── salsa-macro-rules v0.27.2
│   │   │   │   │   ├── salsa-macros v0.27.2 (proc-macro)
│   │   │   │   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   │   │   │   ├── quote v1.0.47 (*)
│   │   │   │   │   │   ├── syn v2.0.119 (*)
│   │   │   │   │   │   └── synstructure v0.13.2
│   │   │   │   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │   │   │   │       ├── quote v1.0.47 (*)
│   │   │   │   │   │       └── syn v2.0.119 (*)
│   │   │   │   │   ├── smallvec v1.15.2
│   │   │   │   │   ├── thin-vec v0.2.18
│   │   │   │   │   ├── tracing v0.1.44 (*)
│   │   │   │   │   └── typeid v1.0.3
│   │   │   │   └── text-size v1.1.1
│   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   ├── rustc-hash v2.1.3
│   │   │   └── text-size v1.1.1
│   │   ├── rustc-hash v2.1.3
│   │   └── tracing v0.1.44 (*)
│   ├── ra_ap_intern v0.0.342 (*)
│   ├── ra_ap_query-group-macro v0.0.342 (proc-macro)
│   │   ├── proc-macro2 v1.0.107 (*)
│   │   ├── quote v1.0.47 (*)
│   │   └── syn v2.0.119 (*)
│   ├── ra_ap_span v0.0.342 (*)
│   ├── ra_ap_syntax v0.0.342 (*)
│   ├── ra_ap_vfs v0.0.342 (*)
│   ├── rustc-hash v2.1.3
│   ├── salsa v0.27.2 (*)
│   ├── salsa-macros v0.27.2 (proc-macro) (*)
│   ├── semver v1.0.28
│   ├── tracing v0.1.44 (*)
│   └── triomphe v0.1.16
├── ra_ap_hir v0.0.342
│   ├── arrayvec v0.7.8
│   ├── either v1.16.0
│   ├── itertools v0.15.0 (*)
│   ├── la-arena v0.3.1
│   ├── ra-ap-rustc_type_ir v0.165.0
│   │   ├── arrayvec v0.7.8
│   │   ├── bitflags v2.13.1
│   │   ├── derive-where v1.6.1 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   └── syn v2.0.119 (*)
│   │   ├── ena v0.14.4
│   │   │   └── log v0.4.33
│   │   ├── indexmap v2.14.0 (*)
│   │   ├── ra-ap-rustc_abi v0.165.0
│   │   │   ├── bitflags v2.13.1
│   │   │   ├── ra-ap-rustc_hashes v0.165.0
│   │   │   │   └── rustc-stable-hash v0.1.2
│   │   │   ├── ra-ap-rustc_index v0.165.0
│   │   │   │   └── ra-ap-rustc_index_macros v0.165.0 (proc-macro)
│   │   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │   │       ├── quote v1.0.47 (*)
│   │   │   │       └── syn v2.0.119 (*)
│   │   │   └── tracing v0.1.44 (*)
│   │   ├── ra-ap-rustc_ast_ir v0.165.0
│   │   ├── ra-ap-rustc_index v0.165.0 (*)
│   │   ├── ra-ap-rustc_type_ir_macros v0.165.0 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   ├── syn v2.0.119 (*)
│   │   │   └── synstructure v0.13.2 (*)
│   │   ├── rustc-hash v2.1.3
│   │   ├── smallvec v1.15.2
│   │   ├── thin-vec v0.2.18
│   │   └── tracing v0.1.44 (*)
│   ├── ra_ap_base_db v0.0.342 (*)
│   ├── ra_ap_cfg v0.0.342 (*)
│   ├── ra_ap_hir_def v0.0.342
│   │   ├── arrayvec v0.7.8
│   │   ├── bitflags v2.13.1
│   │   ├── cov-mark v2.2.0
│   │   ├── drop_bomb v0.1.5
│   │   ├── either v1.16.0
│   │   ├── fst v0.4.7
│   │   ├── indexmap v2.14.0 (*)
│   │   ├── itertools v0.15.0 (*)
│   │   ├── la-arena v0.3.1
│   │   ├── ra-ap-rustc_abi v0.165.0 (*)
│   │   ├── ra-ap-rustc_parse_format v0.165.0
│   │   │   ├── ra-ap-rustc_lexer v0.165.0 (*)
│   │   │   └── rustc-literal-escaper v0.0.7
│   │   ├── ra_ap_base_db v0.0.342 (*)
│   │   ├── ra_ap_cfg v0.0.342 (*)
│   │   ├── ra_ap_hir_expand v0.0.342
│   │   │   ├── cov-mark v2.2.0
│   │   │   ├── either v1.16.0
│   │   │   ├── itertools v0.15.0 (*)
│   │   │   ├── ra_ap_base_db v0.0.342 (*)
│   │   │   ├── ra_ap_cfg v0.0.342 (*)
│   │   │   ├── ra_ap_intern v0.0.342 (*)
│   │   │   ├── ra_ap_mbe v0.0.342
│   │   │   │   ├── arrayvec v0.7.8
│   │   │   │   ├── bitflags v2.13.1
│   │   │   │   ├── cov-mark v2.2.0
│   │   │   │   ├── ra-ap-rustc_lexer v0.165.0 (*)
│   │   │   │   ├── ra_ap_intern v0.0.342 (*)
│   │   │   │   ├── ra_ap_parser v0.0.342 (*)
│   │   │   │   ├── ra_ap_span v0.0.342 (*)
│   │   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   │   ├── ra_ap_syntax-bridge v0.0.342
│   │   │   │   │   ├── ra_ap_intern v0.0.342 (*)
│   │   │   │   │   ├── ra_ap_parser v0.0.342 (*)
│   │   │   │   │   ├── ra_ap_span v0.0.342 (*)
│   │   │   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   │   │   ├── ra_ap_syntax v0.0.342 (*)
│   │   │   │   │   ├── ra_ap_tt v0.0.342 (*)
│   │   │   │   │   └── rustc-hash v2.1.3
│   │   │   │   ├── ra_ap_tt v0.0.342 (*)
│   │   │   │   ├── rustc-hash v2.1.3
│   │   │   │   ├── salsa v0.27.2 (*)
│   │   │   │   └── smallvec v1.15.2
│   │   │   ├── ra_ap_parser v0.0.342 (*)
│   │   │   ├── ra_ap_span v0.0.342 (*)
│   │   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   │   ├── ra_ap_syntax v0.0.342 (*)
│   │   │   ├── ra_ap_syntax-bridge v0.0.342 (*)
│   │   │   ├── ra_ap_tt v0.0.342 (*)
│   │   │   ├── rustc-hash v2.1.3
│   │   │   ├── salsa v0.27.2 (*)
│   │   │   ├── salsa-macros v0.27.2 (proc-macro) (*)
│   │   │   ├── smallvec v1.15.2
│   │   │   ├── thin-vec v0.2.18
│   │   │   ├── tracing v0.1.44 (*)
│   │   │   └── triomphe v0.1.16
│   │   ├── ra_ap_intern v0.0.342 (*)
│   │   ├── ra_ap_span v0.0.342 (*)
│   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   ├── ra_ap_syntax v0.0.342 (*)
│   │   ├── ra_ap_syntax-bridge v0.0.342 (*)
│   │   ├── ra_ap_tt v0.0.342 (*)
│   │   ├── rustc-hash v2.1.3
│   │   ├── rustc_apfloat v0.2.3+llvm-462a31f5a5ab
│   │   │   ├── bitflags v2.13.1
│   │   │   └── smallvec v1.15.2
│   │   ├── salsa v0.27.2 (*)
│   │   ├── salsa-macros v0.27.2 (proc-macro) (*)
│   │   ├── smallvec v1.15.2
│   │   ├── thin-vec v0.2.18
│   │   ├── tracing v0.1.44 (*)
│   │   └── triomphe v0.1.16
│   ├── ra_ap_hir_expand v0.0.342 (*)
│   ├── ra_ap_hir_ty v0.0.342
│   │   ├── arrayvec v0.7.8
│   │   ├── bitflags v2.13.1
│   │   ├── cov-mark v2.2.0
│   │   ├── either v1.16.0
│   │   ├── ena v0.14.4 (*)
│   │   ├── indexmap v2.14.0 (*)
│   │   ├── itertools v0.15.0 (*)
│   │   ├── la-arena v0.3.1
│   │   ├── oorandom v11.1.5
│   │   ├── petgraph v0.8.3
│   │   │   ├── fixedbitset v0.5.7
│   │   │   ├── hashbrown v0.15.5
│   │   │   │   └── foldhash v0.1.5
│   │   │   └── indexmap v2.14.0 (*)
│   │   ├── ra-ap-rustc_abi v0.165.0 (*)
│   │   ├── ra-ap-rustc_ast_ir v0.165.0
│   │   ├── ra-ap-rustc_index v0.165.0 (*)
│   │   ├── ra-ap-rustc_next_trait_solver v0.165.0
│   │   │   ├── derive-where v1.6.1 (proc-macro) (*)
│   │   │   ├── ra-ap-rustc_index v0.165.0 (*)
│   │   │   ├── ra-ap-rustc_type_ir v0.165.0 (*)
│   │   │   ├── ra-ap-rustc_type_ir_macros v0.165.0 (proc-macro) (*)
│   │   │   └── tracing v0.1.44 (*)
│   │   ├── ra-ap-rustc_pattern_analysis v0.165.0
│   │   │   ├── ra-ap-rustc_index v0.165.0 (*)
│   │   │   ├── rustc-hash v2.1.3
│   │   │   ├── rustc_apfloat v0.2.3+llvm-462a31f5a5ab (*)
│   │   │   ├── smallvec v1.15.2
│   │   │   └── tracing v0.1.44 (*)
│   │   ├── ra-ap-rustc_type_ir v0.165.0 (*)
│   │   ├── ra_ap_base_db v0.0.342 (*)
│   │   ├── ra_ap_hir_def v0.0.342 (*)
│   │   ├── ra_ap_hir_expand v0.0.342 (*)
│   │   ├── ra_ap_intern v0.0.342 (*)
│   │   ├── ra_ap_macros v0.0.342 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   ├── syn v2.0.119 (*)
│   │   │   └── synstructure v0.13.2 (*)
│   │   ├── ra_ap_query-group-macro v0.0.342 (proc-macro) (*)
│   │   ├── ra_ap_span v0.0.342 (*)
│   │   ├── ra_ap_stdx v0.0.342 (*)
│   │   ├── ra_ap_syntax v0.0.342 (*)
│   │   ├── rustc-hash v2.1.3
│   │   ├── rustc_apfloat v0.2.3+llvm-462a31f5a5ab (*)
│   │   ├── salsa v0.27.2 (*)
│   │   ├── salsa-macros v0.27.2 (proc-macro) (*)
│   │   ├── serde v1.0.229
│   │   │   └── serde_core v1.0.229
│   │   ├── serde_derive v1.0.229 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   └── syn v3.0.0
│   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │       ├── quote v1.0.47 (*)
│   │   │       └── unicode-ident v1.0.24
│   │   ├── smallvec v1.15.2
│   │   ├── thin-vec v0.2.18
│   │   ├── tracing v0.1.44 (*)
│   │   ├── tracing-subscriber v0.3.23
│   │   │   ├── sharded-slab v0.1.7
│   │   │   │   └── lazy_static v1.5.0
│   │   │   ├── thread_local v1.1.10
│   │   │   │   └── cfg-if v1.0.4
│   │   │   ├── time v0.3.53
│   │   │   │   ├── deranged v0.5.8
│   │   │   │   ├── num-conv v0.2.2
│   │   │   │   ├── powerfmt v0.2.0
│   │   │   │   └── time-core v0.1.9
│   │   │   ├── tracing-core v0.1.36 (*)
│   │   │   └── tracing-log v0.2.0
│   │   │       ├── log v0.4.33
│   │   │       ├── once_cell v1.21.4
│   │   │       └── tracing-core v0.1.36 (*)
│   │   ├── tracing-tree v0.4.1
│   │   │   ├── nu-ansi-term v0.50.3
│   │   │   ├── tracing-core v0.1.36 (*)
│   │   │   ├── tracing-log v0.2.0 (*)
│   │   │   └── tracing-subscriber v0.3.23 (*)
│   │   ├── triomphe v0.1.16
│   │   └── typed-arena v2.0.2
│   ├── ra_ap_intern v0.0.342 (*)
│   ├── ra_ap_span v0.0.342 (*)
│   ├── ra_ap_stdx v0.0.342 (*)
│   ├── ra_ap_syntax v0.0.342 (*)
│   ├── ra_ap_tt v0.0.342 (*)
│   ├── rustc-hash v2.1.3
│   ├── serde_json v1.0.150
│   │   ├── itoa v1.0.18
│   │   ├── memchr v2.8.3
│   │   ├── serde_core v1.0.229
│   │   └── zmij v1.0.23
│   ├── smallvec v1.15.2
│   ├── tracing v0.1.44 (*)
│   └── triomphe v0.1.16
└── ra_ap_hir_ty v0.0.342 (*)
```
