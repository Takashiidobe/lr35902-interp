# LR35902 interpreter

This project analyzes LR35902/SM83 asm, reporting byte size and cycle count
while executing the instructions in a small virtual machine.

Run the CLI with inline assembly:

```sh
cargo run -- --asm "ld a, 1"
```

## On the Web

Build and serve the web version:

```sh
wasm-pack build --target web --out-dir web/pkg --release
cd web
npm install
npm run build
```

And then serve with an http server of your choice in `web/`

## Preprocessing Gaps

- Broader RGBDS expression support, including string constants, built-in
  functions, and label or address expressions.
- Additional macro argument forms such as `\<n>` and `_NARG`.
- `INCLUDE` expansion and accounting for the contents of `INCBIN` files.
- Symbol lifecycle directives such as `PURGE`, plus stricter redefinition rules.
- More precise diagnostics that retain macro definition and included-file
  locations.
