## Move transforms to morphs

`trim`, `lower`, `upper`, `capitalize`, and Unicode normalization now belong in `#[morphs(...)]`. They run before all validators, in the order written. The former `StringValidator::Trim`-style paths are removed.

```rust
#[morphs(trim, lower)]
#[validators(non_empty)]
```

## Replace parse validators with types

| Former validator | Field type |
| --- | --- |
| `integer_parse`, `numeric_parse` | `FromText<T>` |
| `url_parse` | `FromText<url::Url>` |
| `date_iso_parse` | `IsoDate` |
| `date_epoch_parse` | `EpochMillis` |
| `json_parse` | `JsonText<T>` |

The loose `date_parse` is removed because its accepted formats do not round-trip. Codec parse failures are serde errors at the point parsing stops, rather than accumulated validator failures.

## Put checks at their positions

Container `#[validators(...)]` is accepted for newtypes, not named-field structs, enums, or multi-field tuple structs. Move checks to fields and tuple elements.

`starts_with`, `ends_with`, and `includes` take a text pattern: quoted text or a format. Rust path forms use `TextPattern`, for example `StringValidator::Includes("x".into())`.

## Remove obsolete defaults

ArkType no longer emits `default{Type}` objects. Foreign types no longer accept `default_value_ts`; remove the key from configuration.

## Update format names

`AppointmentDurationNs` becomes `duration_ns(min = "PT1H", max = "PT5H", step = "PT15M")`. A field format is now `MockFormat`, which wraps a `Format`.

Normalization validator variants use `NormalizeNfcPreformatted` and corresponding NFD/NFKC/NFKD names. `NumberValidator::NonNaN` is `NonNan`; its attribute name is `non_nan`.

## Select generators explicitly

For library and build-script generation, list the Cargo features for each generator. `typesync` and `build-typesync` provide infrastructure without enabling ArkType or Effect automatically.
