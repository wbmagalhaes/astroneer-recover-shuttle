# Astroneer Save Editor: Force-Land Shuttle

A client-side Astroneer `.savegame` decoder, based on
[ricky-davis' astro_save_parser](https://github.com/ricky-davis/astro_save_parser) and re-implemented in Rust -> WebAssembly.

On top of decoding, it also **fixes a shuttle stuck in orbit**: it moves the shuttle onto a
landing pad, clears the lost/orbit state, and resets `ExitSuppressionCount` so you can exit it
again. Everything runs in your browser, so your save never leaves your device.

**Use it here:** https://wbmagalhaes.github.io/astroneer-recover-shuttle/

## Does this describe your bug?

This is a fix for an Astroneer bug where a **shuttle gets stuck / lost in orbit** and you can't bring
it back (most often in **multiplayer/co-op** after a lag spike or a player disconnecting
mid-flight).

> ⚠️ **Back up your save first** and keep the original: this was tested on my own save and my own
> specific bug, so there's no guarantee it works for every save. Use at your own risk.

## Build Locally

```bash
./build-wasm.sh                      # Rust -> wasm, outputs web/pkg/
python3 -m http.server -d web 8000   # then open http://localhost:8000
```
