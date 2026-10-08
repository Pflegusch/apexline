# Contributing to Apexline

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an issue with the "Bug report" template. Helpful: your OS, how you run Apexline, the console
output (`journalctl --user -u apexline` on Linux) and – if it is about the coach or a measurement –
the session folder or lap file (`<data folder>/recordings/…`, see `apexline config`).

## Code

```sh
cargo build --release
cargo test
./target/release/apexline demo --web-dir web   # simulated data, dashboard served from disk
```

- Rust (stable), one binary; the dashboard is plain HTML/CSS/JS in `web/` without a build step or
  external libraries.
- Code comments in English; match the style of the surrounding code.
- User-facing texts exist in German and English: in Rust via `tr!("deutsch", "english")` and the unit
  helpers in `src/i18n.rs` (values stay metric internally); on the dashboard via `t('Deutscher Text')`
  with the English text added to `web/i18n.js`.
- Keep tests few and targeted: add one where a bug would be easy to reintroduce.
- Make sure `cargo test` passes.

## License

Apexline is dual-licensed under MIT and Apache-2.0. Unless you state otherwise, any contribution
you submit is licensed the same way, without additional terms or conditions.

Apexline is not affiliated with Sony Interactive Entertainment or Polyphony Digital.
"Gran Turismo" and "PlayStation" are their trademarks and are used here only to describe what the
software works with.
