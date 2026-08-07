# lava-presets

Typed `Profile` overlays for the [lava](https://github.com/pleme-io) +
tatara-lisp ecosystem. The analog of Pangea's
`Pangea::Presets.apply(config, PROFILES)`.

A **preset** is a named bag of bindings that layers on top of an architecture
call. `PresetRegistry::apply` resolves one precedence chain:

```text
architecture defaults  <  preset overlay  <  operator bindings
```

so an operator's explicit value always wins, and a preset only fills what the
architecture left at its default.

## Form

```lisp
(deflava-preset public-dns/production
  :bindings (:dnssec-enabled "true"
             :query-logging-enabled "true"
             :retention-days "365"))
```

Presets can be authored inline through `PresetRegistry`, or loaded from
`(deflava-preset …)` `.tlisp` forms.

## Usage

```toml
[dependencies]
lava-presets = "0.1"
```

## License

MIT
