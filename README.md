# Astroneer Save Editor: Force-Land Shuttle

A client-side Astroneer `.savegame` decoder, based on
[ricky-davis' astro_save_parser](https://github.com/ricky-davis/astro_save_parser) and re-implemented in Rust -> WebAssembly.

On top of decoding, it also **fixes a shuttle stuck in orbit**: it moves the shuttle onto a
landing pad, clears the lost/orbit state, and resets `ExitSuppressionCount` so you can exit it
again. Everything runs in your browser, so your save never leaves your device.

**Use it:** [PUBLISHED_PAGE_URL](PUBLISHED_PAGE_URL)

## Build Locally

```bash
./build-wasm.sh                      # Rust -> wasm, outputs web/pkg/
python3 -m http.server -d web 8000   # then open http://localhost:8000
```
